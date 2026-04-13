use crate::audio_environment::AudioEnvironment;
use crate::build_error::BuildError;
use crate::cable_pool::CablePool;
use crate::cables::{InputPort, OutputPort};
use super::instance_id::InstanceId;
use super::module_descriptor::{ModuleDescriptor, ModuleShape, ParameterKind};
use super::parameter_map::{ParameterMap, ParameterValue};

/// Trait for modules that need periodic coefficient recalculation.
///
/// Modules implementing this trait receive a call to [`periodic_update`] every
/// [`COEFF_UPDATE_INTERVAL`] samples, before the main per-sample processing loop.
/// The callback reads CV input values from the previous sample's cable pool snapshot
/// (consistent with the 1-sample cable delay) and uses them to update interpolation ramps.
///
/// [`periodic_update`]: PeriodicUpdate::periodic_update
/// [`COEFF_UPDATE_INTERVAL`]: crate::COEFF_UPDATE_INTERVAL
pub trait PeriodicUpdate {
    fn periodic_update(&mut self, pool: &CablePool<'_>);
}


/// Validate `params` against `descriptor`.
///
/// Returns an error if:
/// - Any key in `params` is not declared in `descriptor.parameters`.
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
    for (name, idx) in params.keys() {
        if !descriptor.parameters.iter().any(|p| p.matches(name, idx)) {
            return Err(unknown_parameter_error(descriptor, name, idx));
        }
    }

    // Validate type and bounds for each supplied parameter.
    for param_desc in &descriptor.parameters {
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
                        found: *v,
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
                        found: *v as f32,
                    });
                }
            }
            (ParameterKind::Bool { .. }, ParameterValue::Bool(_)) => {}
            (ParameterKind::Enum { variants, .. }, ParameterValue::Enum(v)) => {
                if !variants.contains(v) {
                    return Err(BuildError::Custom {
                        module: descriptor.module_name,
                        message: format!(
                            "parameter '{}' has unrecognised value '{v}'",
                            param_desc.name
                        ),
                    });
                }
            }
            (ParameterKind::String { .. }, ParameterValue::String(_)) => {}
            (ParameterKind::SongName, ParameterValue::Int(_)) => {}
            (ParameterKind::File { .. }, ParameterValue::File(_)) => {}
            (ParameterKind::File { .. }, ParameterValue::FloatBuffer(_)) => {}
            (ParameterKind::Array { length, .. }, ParameterValue::Array(v)) => {
                if v.len() > *length {
                    return Err(BuildError::Custom {
                        module: descriptor.module_name,
                        message: format!(
                            "parameter '{}' has {} elements but capacity is {}",
                            param_desc.name,
                            v.len(),
                            length,
                        ),
                    });
                }
            }
            _ => {
                return Err(BuildError::InvalidParameterType {
                    module: descriptor.module_name,
                    parameter: param_desc.name,
                    expected: param_desc.parameter_type.kind_name(),
                    found: value.kind_name(),
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
        .parameters
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
        ),
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
    /// Return the static descriptor for this module type, computed from the given shape.
    fn describe(shape: &ModuleShape) -> ModuleDescriptor
    where
        Self: Sized;

    /// Allocate and initialise a new instance, storing `audio_environment`, `descriptor`,
    /// and the externally-minted `instance_id`. All other fields should be set to their
    /// default/zero values.
    ///
    /// This is infallible; parameter validation is deferred to
    /// [`update_validated_parameters`](Module::update_validated_parameters).
    fn prepare(
        audio_environment: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
    ) -> Self
    where
        Self: Sized;

    /// Apply an already-validated `params` to this instance, updating fields derived from
    /// parameters.
    ///
    /// Only called by the default [`update_parameters`](Module::update_parameters) after
    /// validation passes. All keys are guaranteed to be declared in the descriptor and their
    /// values are guaranteed to be correctly typed and within bounds.
    ///
    /// Takes `&mut ParameterMap` so that modules can destructively read values
    /// (via [`ParameterMap::take_scalar`] etc.) without cloning on the audio
    /// thread.  The plan retains ownership of the map; any values left behind
    /// are deallocated on the cleanup thread when the plan is dropped.
    fn update_validated_parameters(&mut self, params: &mut ParameterMap);

    /// Validate `params` against the module's descriptor, then apply them.
    ///
    /// The default implementation calls [`validate_parameters`] and, on success, forwards
    /// to [`update_validated_parameters`](Module::update_validated_parameters). Override only
    /// if custom validation beyond what [`validate_parameters`] provides is needed.
    fn update_parameters(&mut self, params: &ParameterMap) -> Result<(), BuildError> {
        validate_parameters(params, self.descriptor())?;
        self.update_validated_parameters(&mut params.clone());
        Ok(())
    }

    /// Construct a fully initialised instance from an audio environment, shape, parameters,
    /// and an externally-minted `instance_id`.
    ///
    /// The default implementation:
    /// 1. Calls [`describe`](Module::describe) to obtain the descriptor.
    /// 2. Calls [`prepare`](Module::prepare) with the given `instance_id`.
    /// 3. Fills any missing parameters using the defaults declared in the descriptor.
    /// 4. Calls [`update_parameters`](Module::update_parameters) (validates then applies).
    ///
    /// Module implementations should not need to override this.
    fn build(
        audio_environment: &AudioEnvironment,
        shape: &ModuleShape,
        params: &ParameterMap,
        instance_id: InstanceId,
    ) -> Result<Self, BuildError>
    where
        Self: Sized,
    {
        let descriptor = Self::describe(shape);
        let mut instance = Self::prepare(audio_environment, descriptor, instance_id);

        // Fill in any missing parameters using the descriptor's declared defaults.
        let mut filled = params.clone();
        for param_desc in instance.descriptor().parameters.iter() {
            filled.get_or_insert(param_desc.name, param_desc.index, || {
                param_desc.parameter_type.default_value()
            });
        }

        instance.update_parameters(&filled)?;  // control thread — clone in default impl is fine
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

    /// Returns `Some(self)` if this module implements [`PeriodicUpdate`], `None` otherwise.
    ///
    /// Override this to return `Some(self)` in modules that implement [`PeriodicUpdate`].
    /// The execution plan uses this during plan activation to build `periodic_indices`.
    ///
    /// The default implementation returns `None`.
    fn as_periodic(&mut self) -> Option<&mut dyn PeriodicUpdate> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_error::BuildError;
    use crate::modules::{ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind, ParameterMap, ParameterValue};

    fn array_descriptor() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "TestArrayModule",
            shape: ModuleShape { channels: 0, length: 16, ..Default::default() },
            inputs: vec![],
            outputs: vec![],
            parameters: vec![ParameterDescriptor {
                name: "steps",
                index: 0,
                parameter_type: ParameterKind::Array { default: &[], length: 16 },
            }],
        }
    }

    #[test]
    fn array_value_passes_validation_for_array_parameter() {
        let mut params = ParameterMap::new();
        params.insert(
            "steps".to_string(),
            ParameterValue::Array(vec!["C3".to_string()].into()),
        );
        let desc = array_descriptor();
        assert!(validate_parameters(&params, &desc).is_ok());
    }

    #[test]
    fn array_value_exceeding_length_returns_error() {
        let mut params = ParameterMap::new();
        params.insert(
            "steps".to_string(),
            ParameterValue::Array(vec!["C3".to_string(); 17].into()),
        );
        let desc = array_descriptor();
        let err = validate_parameters(&params, &desc).unwrap_err();
        assert!(
            matches!(err, BuildError::Custom { module: "TestArrayModule", .. }),
            "expected Custom error for capacity exceeded, got: {err:?}"
        );
    }

    #[test]
    fn float_value_against_array_descriptor_returns_invalid_parameter_type() {
        let mut params = ParameterMap::new();
        params.insert("steps".to_string(), ParameterValue::Float(1.0));
        let desc = array_descriptor();
        let err = validate_parameters(&params, &desc).unwrap_err();
        assert!(
            matches!(err, BuildError::InvalidParameterType { parameter: "steps", .. }),
            "expected InvalidParameterType, got: {err:?}"
        );
    }
}
