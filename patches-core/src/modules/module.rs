use crate::audio_environment::AudioEnvironment;
use crate::build_error::BuildError;
use crate::cable_pool::CablePool;
use crate::cables::{InputPort, OutputPort};
use super::instance_id::InstanceId;
use super::descriptor_template::ModuleDescriptorTemplate;
use super::module_descriptor::{ModuleDescriptor, ModuleShape, ParameterKind};
use super::parameter_map::{ParameterMap, ParameterValue};
use super::structural_params::StructuralParams;
use crate::param_frame::{pack_into, ParamFrame, ParamView, ParamViewIndex};
use crate::param_layout::{compute_layout, defaults_from_descriptor, ParamLayout};

/// Validate `params` against `descriptor`.
///
/// Returns an error if:
/// - Any key in `params` is not declared in `descriptor.realtime_params`.
/// - Any supplied value has the wrong type for its parameter.
/// - Any supplied numeric value is outside the bounds declared in its [`ParameterKind`].
/// - Any supplied enum value is not among the declared variants.
///
/// Missing parameters are not an error; they are simply left unchanged (or filled with
/// defaults by [`Module::build`] before this is called for the first time).
pub fn validate_parameters(
    params: &ParameterMap,
    descriptor: &ModuleDescriptor,
) -> Result<(), BuildError> {
    // Reject any key not declared in the descriptor.
    for (name, idx, _) in params.iter() {
        if !descriptor.realtime_params.iter().any(|p| p.matches(name, idx)) {
            return Err(unknown_parameter_error(descriptor, name, idx));
        }
    }

    // Validate type and bounds for each supplied parameter.
    for param_desc in &descriptor.realtime_params {
        let Some(value) = params.get(param_desc.name, param_desc.index) else {
            continue;
        };
        match (&param_desc.parameter_type, value) {
            (ParameterKind::Float { min, max, .. }, ParameterValue::Float(v)) => {
                if *v < *min || *v > *max {
                    return Err(BuildError::ParameterOutOfRange {
                        module: descriptor.module_name,
                        parameter: param_desc.name,
                        min: *min,
                        max: *max,
                        found: *v, origin: None,
                    });
                }
            }
            (ParameterKind::Int { min, max, .. }, ParameterValue::Int(v)) => {
                if *v < *min || *v > *max {
                    return Err(BuildError::ParameterOutOfRange {
                        module: descriptor.module_name,
                        parameter: param_desc.name,
                        min: *min as f32,
                        max: *max as f32,
                        found: *v as f32, origin: None,
                    });
                }
            }
            (ParameterKind::Bool { .. }, ParameterValue::Bool(_)) => {}
            (ParameterKind::Enum { variants, .. }, ParameterValue::Enum(v)) => {
                if (*v as usize) >= variants.len() {
                    return Err(BuildError::Custom {
                        module: descriptor.module_name,
                        message: format!(
                            "parameter '{}' has out-of-range enum index {v} (variants: {})",
                            param_desc.name,
                            variants.len()
                        ), origin: None,
                    });
                }
            }
            (ParameterKind::SongName, ParameterValue::Int(_)) => {}
            _ => {
                return Err(BuildError::InvalidParameterType {
                    module: descriptor.module_name,
                    parameter: param_desc.name,
                    expected: param_desc.parameter_type.kind_name(),
                    found: value.kind_name(), origin: None,
                });
            }
        }
    }

    Ok(())
}

fn unknown_parameter_error(descriptor: &ModuleDescriptor, name: &str, idx: usize) -> BuildError {
    let key_display = if idx == 0 {
        name.to_string()
    } else {
        format!("{name}/{idx}")
    };
    let mut known: Vec<String> = descriptor
        .realtime_params
        .iter()
        .map(|p| {
            if p.index == 0 {
                p.name.to_string()
            } else {
                format!("{}/{}", p.name, p.index)
            }
        })
        .collect();
    known.sort();
    known.dedup();
    BuildError::Custom {
        module: descriptor.module_name,
        message: format!(
            "unknown parameter '{key_display}'; known parameters: {}",
            known.join(", ")
        ), origin: None,
    }
}

/// Validate `params` against `descriptor` and pack into a fresh
/// [`ParamFrame`] sized by the descriptor's realtime parameters.
///
/// Returns an error if validation fails or `pack_into` rejects the input.
/// Intended for control-thread use during initial build / param-edit
/// resolution (ADR 0060): the audio thread receives the resulting frame
/// and reads it via [`ParamView`].
pub fn validate_and_pack(
    descriptor: &ModuleDescriptor,
    params: &ParameterMap,
) -> Result<ParamFrame, BuildError> {
    validate_parameters(params, descriptor)?;
    let layout = compute_layout(descriptor);
    let mut frame = ParamFrame::with_layout(&layout);
    let defaults = defaults_from_descriptor(descriptor);
    pack_into(&layout, &defaults, params, &mut frame).map_err(|e| BuildError::Custom {
        module: descriptor.module_name,
        message: format!("pack_into failed: {e:?}"),
        origin: None,
    })?;
    Ok(frame)
}

/// A validated, packed realtime [`ParamFrame`] together with the
/// [`ParamLayout`] and [`ParamViewIndex`] needed to read it back as a
/// [`ParamView`].
///
/// Bundles the six-step setup (validate, pack, layout, view-index, view,
/// update) used in module construction and FFI plugin prepare so callers
/// can collapse it to two lines:
///
/// ```ignore
/// let validated = ValidatedParamFrame::new(&descriptor, &filled)?;
/// module.update_validated_parameters(&validated.view());
/// ```
///
/// Owns its three pieces so the `ParamView` borrow it hands out lives as
/// long as the bundle. Intended for control-thread use; the audio thread
/// reads only the [`ParamView`].
pub struct ValidatedParamFrame {
    frame: ParamFrame,
    layout: ParamLayout,
    index: ParamViewIndex,
}

impl ValidatedParamFrame {
    /// Validate `params` against `descriptor`, pack into a fresh frame,
    /// and precompute the layout and view index.
    pub fn new(
        descriptor: &ModuleDescriptor,
        params: &ParameterMap,
    ) -> Result<Self, BuildError> {
        let frame = validate_and_pack(descriptor, params)?;
        let layout = compute_layout(descriptor);
        let index = ParamViewIndex::from_layout(&layout);
        Ok(Self { frame, layout, index })
    }

    /// Borrow as a [`ParamView`] suitable for
    /// [`Module::update_validated_parameters`].
    pub fn view(&self) -> ParamView<'_> {
        ParamView::new(&self.index, &self.frame)
    }

    /// Borrow the underlying [`ParamFrame`].
    pub fn frame(&self) -> &ParamFrame {
        &self.frame
    }

    /// Borrow the underlying [`ParamLayout`].
    pub fn layout(&self) -> &ParamLayout {
        &self.layout
    }
}

/// Describes which input and output ports of a module are connected in the current patch.
///
/// `inputs[i]` is `true` if the i-th input port (as listed in [`ModuleDescriptor::inputs`])
/// has at least one incoming cable; `outputs[i]` is `true` if the i-th output port has at
/// least one outgoing cable.
///
/// ## Why this exists alongside the per-port `connected` field
///
/// Each concrete port type (`MonoInput`, `PolyInput`, etc.) also carries a `connected: bool`
/// field that reflects the same information at runtime.  `PortConnectivity` is a separate,
/// planner-internal snapshot taken at plan-build time.  The planner keeps the previous
/// build's `PortConnectivity` and diffs it against the new build's snapshot to decide
/// whether [`Module::set_ports`] needs to be called for a given module — without having
/// to re-inspect the individual port objects stored inside each live module instance.
/// This avoids any per-port iteration on the audio thread between builds.
///
/// Connectivity information is delivered to modules via port objects in
/// [`Module::set_ports`] rather than via this struct directly.
#[derive(Debug, Clone, PartialEq)]
pub struct PortConnectivity {
    pub inputs: Box<[bool]>,
    pub outputs: Box<[bool]>,
}

impl PortConnectivity {
    /// Create an all-`false` instance sized for a module with `n_inputs` input ports and
    /// `n_outputs` output ports.
    pub fn new(n_inputs: usize, n_outputs: usize) -> Self {
        Self {
            inputs: vec![false; n_inputs].into_boxed_slice(),
            outputs: vec![false; n_outputs].into_boxed_slice(),
        }
    }
}

/// The core trait all audio modules implement.
///
/// Construction follows a two-phase protocol:
///
/// 1. [`prepare`](Module::prepare) — allocates and initialises the instance with the audio
///    environment and descriptor. Other fields are set to their defaults. Infallible.
/// 2. [`update_validated_parameters`](Module::update_validated_parameters) — applies a
///    pre-validated [`ParameterMap`] to the instance.
///
/// [`build`](Module::build) has a default implementation that:
/// - Calls [`describe`](Module::describe) to get the descriptor.
/// - Calls [`prepare`](Module::prepare).
/// - Fills in any missing parameters from the descriptor's declared defaults.
/// - Calls [`update_parameters`](Module::update_parameters) (which validates then delegates).
///
/// [`update_parameters`](Module::update_parameters) has a default implementation that
/// validates via [`validate_parameters`] and, on success, calls
/// [`update_validated_parameters`](Module::update_validated_parameters).
///
/// `process` is called once per sample. Both `inputs` and `outputs` are indexed
/// according to the module's [`ModuleDescriptor`].
///
/// `as_any` enables downcasting from `&dyn Module` to a concrete type.
pub trait Module: Send {
    /// Compile-time-constant descriptor template for this module type
    /// (ADR 0066). All modules declare their full port/parameter layout
    /// via a `const TEMPLATE: ModuleDescriptorTemplate = ...;` returned
    /// from this method; runtime descriptors are produced via
    /// [`ModuleDescriptorTemplate::build_channels`].
    ///
    /// Modeled as a method rather than an associated `const` because
    /// `Module` is used as `dyn Module` and associated consts make a
    /// trait dyn-incompatible. The `where Self: Sized` clause keeps it
    /// out of the vtable.
    fn template() -> ModuleDescriptorTemplate
    where
        Self: Sized,
    {
        ModuleDescriptorTemplate::EMPTY
    }

    /// Allocate and initialise a new instance, storing `audio_environment`, `descriptor`,
    /// the externally-minted `instance_id`, and absorbing any structural parameter
    /// values declared by the descriptor (ADR 0060).
    ///
    /// Fallible: structural-driven init (file decode, IR partitioning, sizing
    /// from a structural int param) may fail. Realtime parameter validation
    /// is still deferred to
    /// [`update_validated_parameters`](Module::update_validated_parameters).
    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        structural: &StructuralParams,
    ) -> Result<Self, BuildError>
    where
        Self: Sized;

    /// Apply an already-validated `params` to this instance, updating fields derived from
    /// parameters.
    ///
    /// Only called by the default [`update_parameters`](Module::update_parameters) after
    /// validation passes. All keys are guaranteed to be declared in the descriptor and their
    /// values are guaranteed to be correctly typed and within bounds.
    ///
    /// Reads values from the packed, read-only `ParamView` borrowed over the
    /// pre-packed `ParamFrame` that the engine installed during plan
    /// adoption (ADR 0045 §4, Spike 5).
    fn update_validated_parameters(&mut self, params: &ParamView<'_>);

    /// Apply non-packable initial-build parameter payloads (e.g. file
    /// buffers) that travel in [`ParameterMap`] but not in [`ParamFrame`].
    ///
    /// Called once during [`Module::build`] after [`validate_and_pack`] has
    /// produced the realtime frame and before
    /// [`update_validated_parameters`] is dispatched. Default implementation
    /// is a no-op; override in modules that need the unpacked payload (the
    /// `File` / `FloatBuffer` route is the canonical case, retiring with
    /// 0737 / ADR 0060).
    fn apply_unpacked_params(&mut self, _params: &ParameterMap) -> Result<(), BuildError> {
        Ok(())
    }

    /// Construct a fully initialised instance from an audio environment, shape, parameters,
    /// and an externally-minted `instance_id`.
    ///
    /// The default implementation:
    /// 1. Calls [`describe`](Module::describe) to obtain the descriptor.
    /// 2. Calls [`prepare`](Module::prepare) with the given `instance_id`.
    /// 3. Fills any missing parameters using the defaults declared in the descriptor.
    /// 4. Validates and packs the realtime frame via [`validate_and_pack`].
    /// 5. Calls [`apply_unpacked_params`](Module::apply_unpacked_params).
    /// 6. Calls [`update_validated_parameters`](Module::update_validated_parameters).
    ///
    /// Module implementations should not need to override this.
    fn build(
        audio_environment: &AudioEnvironment,
        shape: &ModuleShape,
        params: &ParameterMap,
        structural: &StructuralParams,
        instance_id: InstanceId,
    ) -> Result<Self, BuildError>
    where
        Self: Sized,
    {
        let descriptor = Self::template().build_channels(shape.channels as u32);
        let mut instance = Self::prepare(audio_environment, descriptor, instance_id, structural)?;

        let unpacked = ParameterMap::with_overrides(
            &ParameterMap::declared_defaults(instance.descriptor()),
            params.iter().map(|(n, i, v)| (n.to_string(), i, v.clone())),
        );
        let packable = ParameterMap::with_overrides(
            &ParameterMap::defaults(instance.descriptor()),
            params.iter().map(|(n, i, v)| (n.to_string(), i, v.clone())),
        );

        let validated = ValidatedParamFrame::new(instance.descriptor(), &packable)?;
        instance.apply_unpacked_params(&unpacked)?;
        instance.update_validated_parameters(&validated.view());
        Ok(instance)
    }

    fn descriptor(&self) -> &ModuleDescriptor;

    /// The stable identity of this module instance.
    ///
    /// Must be assigned at construction time (e.g. via [`InstanceId::next()`]) and
    /// return the same value for the lifetime of the instance.
    fn instance_id(&self) -> InstanceId;

    /// Process one sample using the shared ping-pong cable buffer pool.
    ///
    /// `pool` wraps the full ping-pong buffer and the current write index. Modules
    /// read input values via [`CablePool::read_mono`] / [`CablePool::read_poly`] and
    /// write output values via [`CablePool::write_mono`] / [`CablePool::write_poly`],
    /// using the `cable_idx` fields on their stored port objects (delivered by
    /// [`set_ports`](Module::set_ports)).
    ///
    /// **Must not allocate, block, or perform I/O.**
    fn process(&mut self, pool: &mut CablePool<'_>);

    /// Deliver pre-resolved port objects to the module.
    ///
    /// Called by the engine whenever the patch topology changes (e.g. after a hot-reload).
    /// Each entry in `inputs` / `outputs` corresponds positionally to the module's declared
    /// [`ModuleDescriptor`] inputs / outputs. Connectivity information is carried by the
    /// `connected` field on each concrete port type.
    ///
    /// **Must not allocate, block, or perform I/O.** This method may be called on the
    /// audio thread immediately before the next `process` call.
    ///
    /// The default implementation is a no-op.
    fn set_ports(&mut self, _inputs: &[InputPort], _outputs: &[OutputPort]) {}

    fn as_any(&self) -> &dyn std::any::Any;

    /// Returns `Some(self)` if this module implements [`ReceivesTrackerData`], `None` otherwise.
    ///
    /// Override this to return `Some(self)` in modules that implement
    /// [`ReceivesTrackerData`]. The planner uses this during plan construction
    /// to build the `tracker_receiver_indices` list; the audio thread calls
    /// `receive_tracker_data` on each receiver at plan activation.
    ///
    /// The default implementation returns `None`.
    fn as_tracker_data_receiver(&mut self) -> Option<&mut dyn crate::tracker::ReceivesTrackerData> {
        None
    }

    /// Whether this module wants periodic coefficient updates.
    ///
    /// Queried once at plan-build time. Modules returning `true` are added to
    /// the engine's `periodic_indices` list and receive [`periodic_update`]
    /// calls every `periodic_update_interval` samples. Default: `false`.
    ///
    /// [`periodic_update`]: Module::periodic_update
    fn wants_periodic(&self) -> bool { false }

    /// Offset by which scratch-region cable indices are translated between
    /// the host's index space and the module's. Default `0`: the module
    /// addresses scratch directly, including the backplane region (ticket
    /// 0870).
    ///
    /// FFI plugins override this to [`crate::cables::BACKPLANE_SIZE`]: the
    /// loader hides the backplane region from their scratch slice and the
    /// planner refuses to wire any of their ports to a `cable_idx` below
    /// the offset. This isolates plugin SDKs from backplane-layout churn.
    fn scratch_base_offset(&self) -> usize { 0 }

    /// Called every `periodic_update_interval` samples for modules that
    /// returned `true` from [`wants_periodic`](Module::wants_periodic).
    ///
    /// Reads CV input values from the previous sample's cable pool snapshot
    /// (consistent with the 1-sample cable delay) and uses them to update
    /// interpolation ramps. Default: no-op.
    ///
    /// **Must not allocate, block, or perform I/O.**
    fn periodic_update(&mut self, _pool: &CablePool<'_>) {}
}

/// Build a [`ModuleDescriptor`] for module type `M` at the given shape,
/// via [`Module::template`] + [`ModuleDescriptorTemplate::build_channels`]
/// (ADR 0066). Replaces the retired `patches_core::describe_for::<Module>(shape)` trait method
/// (ticket 0790).
pub fn describe_for<M: Module>(shape: &ModuleShape) -> ModuleDescriptor {
    M::template().build_channels(shape.channels as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_error::BuildError;
    use crate::modules::{ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind, ParameterMap, ParameterValue};

    fn float_descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "TestFloatModule",
            shape: ModuleShape { channels: 0 },
            inputs: vec![],
            outputs: vec![],
            realtime_params: vec![ParameterDescriptor {
                name: "gain",
                index: 0,
                parameter_type: ParameterKind::Float { min: 0.0, max: 1.0, default: 0.5 },
            }],
            structural_params: vec![],
        }
    }

    #[test]
    fn bool_value_against_float_descriptor_returns_invalid_parameter_type() {
        let mut params = ParameterMap::new();
        params.insert("gain".to_string(), ParameterValue::Bool(true));
        let desc = float_descriptor();
        let err = validate_parameters(&params, &desc).unwrap_err();
        assert!(
            matches!(err, BuildError::InvalidParameterType { parameter: "gain", .. }),
            "expected InvalidParameterType, got: {err:?}"
        );
    }
}
