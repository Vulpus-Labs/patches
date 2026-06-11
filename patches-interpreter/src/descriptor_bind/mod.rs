//! Descriptor-level binding of a [`FlatPatch`] against the module registry.
//!
//! Produces a [`BoundPatch`] that pairs each [`FlatModule`] with the
//! [`ModuleDescriptor`] it resolves to (or marks it unresolved), and each
//! [`FlatConnection`] / [`FlatPortRef`] with the port descriptors on both
//! endpoints. No module instances are built and no
//! [`AudioEnvironment`](patches_core::AudioEnvironment) is required — this
//! is the pure descriptor-level pass ADR 0038 calls stage 3b's "partial
//! bound graph".
//!
//! Unlike [`crate::build`] this pass **never short-circuits**: every error
//! is appended to [`BoundPatch::errors`] and the walk continues, so
//! downstream consumers (LSP feature handlers under
//! accumulate-and-continue) see as much resolved information as possible
//! even when some modules fail to bind.
//!
//! Concerns covered here:
//!
//! - Module type resolution (registry lookup).
//! - Shape argument validation (delegated to `Registry::describe`).
//! - Parameter name/type/range validation (via [`crate::convert_params`]).
//! - Connection port existence on both endpoints.
//! - Cable kind, poly-layout, and mono-layout agreement.
//! - Cable-map scale range (`[-10.0, 10.0]`).
//! - Orphan port-ref existence against the resolved descriptor.
//!
//! Concerns that stay in [`crate::build`]:
//!
//! - Module instantiation (needs [`AudioEnvironment`](patches_core::AudioEnvironment)).
//! - Song/pattern shape checks, MasterSequencer song references.
//! - Relative file-path resolution against the patch's base dir.
//!
//! Fan-in to a single input is coalesced into an auto-sum node here
//! (`coalesce_fan_in`), so `a.out -> c.in; b.out -> c.in` is merged, not
//! rejected. (There is no duplicate-input error — see the retired BN0009
//! in `errors.rs`.)

pub mod connections;
pub mod errors;
mod fan_in;
mod kind_conv;
pub mod modules;

pub use connections::{
    BoundConnection, BoundPortRef, ResolvedConnection, ResolvedPortRef, UnresolvedConnection,
    UnresolvedPortRef,
};
pub use errors::{BindError, BindErrorCode, ParamConversionError, ParamConversionKind};
pub use modules::{BoundModule, ResolvedModule, UnresolvedModule};

use std::collections::{HashMap, HashSet};

use patches_core::QName;
use patches_core::registry::Registry;
use patches_dsl::flat::{FlatPatch, SongData};

use connections::{bind_connection, bind_port_ref};
use modules::bind_module;

/// Graph-relevant half of a [`BoundPatch`]: bound modules, connections,
/// port refs, and accumulated [`BindError`]s. Mirrors
/// [`patches_dsl::flat::FlatGraph`]'s structure with each element tagged
/// `Resolved` or `Unresolved`.
#[derive(Debug, Clone, Default)]
pub struct BoundGraph {
    pub modules: Vec<BoundModule>,
    pub connections: Vec<BoundConnection>,
    pub port_refs: Vec<BoundPortRef>,
    pub errors: Vec<BindError>,
}

impl BoundGraph {
    /// Look up a bound module by id.
    pub fn find_module(&self, id: &QName) -> Option<&BoundModule> {
        self.modules.iter().find(|m| m.id() == id)
    }
}

/// The result of descriptor-level binding.
///
/// Decomposes into a [`BoundGraph`] (modules/connections/port refs/errors)
/// and a [`SongData`] threaded through from the [`FlatPatch`] unchanged.
/// Pattern/song-based tracker data is built later by [`crate::build`].
#[derive(Debug, Clone, Default)]
pub struct BoundPatch {
    pub graph: BoundGraph,
    pub song_data: SongData,
}

impl std::ops::Deref for BoundPatch {
    type Target = BoundGraph;
    fn deref(&self) -> &BoundGraph {
        &self.graph
    }
}

impl std::ops::DerefMut for BoundPatch {
    fn deref_mut(&mut self) -> &mut BoundGraph {
        &mut self.graph
    }
}

impl BoundPatch {
    pub fn is_clean(&self) -> bool {
        self.graph.errors.is_empty()
    }

    /// Look up a bound module by id.
    pub fn find_module(&self, id: &QName) -> Option<&BoundModule> {
        self.graph.find_module(id)
    }
}

impl patches_dsl::pipeline::PipelineAudit for BoundPatch {
    /// Audit stage-3b [`BindError`]s for layering violations and return
    /// one [`LayeringWarning`](patches_dsl::pipeline::LayeringWarning)
    /// per violation. Called automatically by the pipeline orchestrator
    /// (`run_all` / `run_accumulate`) after bind so every consumer sees
    /// the same warnings without explicit opt-in.
    ///
    /// Currently flags [`BindErrorCode::UnknownModule`] — an unknown
    /// module reference reaching stage 3b means expansion (stage 3a)
    /// let a reference slip past its own unknown-module check. Future
    /// `PV####` codes land here without a signature change.
    fn layering_warnings(&self) -> Vec<patches_dsl::pipeline::LayeringWarning> {
        self.graph
            .errors
            .iter()
            .filter_map(|e| match e.code {
                BindErrorCode::UnknownModule => {
                    Some(patches_dsl::pipeline::LayeringWarning {
                        code: "PV0001",
                        message: format!(
                            "stage 3b descriptor_bind reported '{}'; stage 3a expansion should have \
                             rejected this reference",
                            e.message
                        ),
                        span: e.span(),
                    })
                }
                _ => None,
            })
            .collect()
    }
}

/// Bind a [`FlatPatch`] against `registry`, producing a [`BoundPatch`].
///
/// Never returns `Err`: all failures are folded into
/// [`BoundPatch::errors`], with the raw flat elements preserved as
/// `Unresolved` variants so downstream passes can keep walking the graph.
pub fn bind(flat: &FlatPatch, registry: &Registry) -> BoundPatch {
    bind_with_base_dir(flat, registry, None)
}

/// Bind variant that resolves relative `File` / `String(path)` parameters
/// against `base_dir` while converting each module's params. Used by
/// consumers (player, CLAP) that load a patch from a `.patches` file on
/// disk; the LSP passes `None`.
pub fn bind_with_base_dir(
    flat: &FlatPatch,
    registry: &Registry,
    base_dir: Option<&std::path::Path>,
) -> BoundPatch {
    let song_name_to_index: HashMap<String, usize> = {
        let mut names: Vec<String> =
            flat.song_data.songs.iter().map(|s| s.name.to_string()).collect();
        names.sort();
        names.into_iter().enumerate().map(|(i, n)| (n, i)).collect()
    };

    let mut errors: Vec<BindError> = Vec::new();
    let mut modules: Vec<BoundModule> = Vec::with_capacity(flat.modules.len());

    for fm in &flat.modules {
        modules.push(bind_module(fm, registry, base_dir, &song_name_to_index, &mut errors));
    }

    // Index resolved modules by id for connection / port-ref lookup. Only
    // resolved modules participate — unresolved references downstream are
    // caught below via the `missing` branch.
    let by_id: HashMap<QName, &ResolvedModule> = modules
        .iter()
        .filter_map(|m| m.as_resolved())
        .map(|r| (r.id.clone(), r))
        .collect();

    // Every module id the patch *declared*, resolved or not. A connection
    // endpoint missing from `by_id` but present here is a knock-on of an
    // unresolved module, not an unknown-module reference — see
    // `record_missing_endpoint`.
    let declared: HashSet<QName> = flat.modules.iter().map(|m| m.id.clone()).collect();

    let port_aliases: HashMap<QName, HashMap<u32, String>> = flat
        .modules
        .iter()
        .map(|m| (m.id.clone(), m.port_aliases.iter().cloned().collect()))
        .collect();

    let mut connections: Vec<BoundConnection> = Vec::with_capacity(flat.connections.len());
    for conn in &flat.connections {
        connections.push(bind_connection(conn, &by_id, &declared, &port_aliases, &mut errors));
    }

    let mut port_refs: Vec<BoundPortRef> = Vec::with_capacity(flat.port_refs.len());
    for pr in &flat.port_refs {
        port_refs.push(bind_port_ref(pr, &by_id, &declared, &port_aliases, &mut errors));
    }

    // Drop the borrow of `modules` held by `by_id` so the fan-in pass can
    // mutate `modules` (it appends synthesized Sum / PolySum / StereoSum
    // nodes).
    drop(by_id);

    // Auto-sum multi-fan-in: when several connections target the same input
    // port, group them and route through a synthesized Sum module of the
    // matching kind. The runtime graph builder still enforces single-source
    // per input (RT0001) — coalescing here ensures the rewritten edges
    // satisfy that invariant.
    fan_in::coalesce_fan_in(&mut modules, &mut connections, registry, &mut errors);

    // Auto-convert accepted mono↔poly Audio kind mismatches (ADR 0074)
    // by inserting synthesised `MonoToPoly` / `PolyToMono` instances.
    // Runs *after* fan-in so a fan-in sum whose output kind disagrees
    // with the target also gets a converter inserted on its sum→target
    // edge.
    kind_conv::coalesce_kind_conv(&mut modules, &mut connections, registry, &mut errors);

    BoundPatch {
        graph: BoundGraph { modules, connections, port_refs, errors },
        song_data: flat.song_data.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::Provenance as CoreProv;
    use patches_dsl::ast::{Scalar, SourceId, Span, Value};
    use patches_dsl::flat::{FlatConnection, FlatModule, FlatPatch};

    fn syn() -> Span {
        Span::synthetic()
    }

    fn registry() -> Registry {
        patches_modules::default_registry()
    }

    fn empty_flat() -> FlatPatch {
        FlatPatch::default()
    }

    fn osc(id: &str) -> FlatModule {
        FlatModule {
            id: id.into(),
            type_name: "Osc".into(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
            param_block_span: None,
            type_name_span: None,
        }
    }

    fn sum(id: &str, channels: i64) -> FlatModule {
        FlatModule {
            id: id.into(),
            type_name: "Sum".into(),
            shape: vec![("channels".into(), Scalar::Int(channels))],
            params: vec![],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
            param_block_span: None,
            type_name_span: None,
        }
    }

    fn conn(from: &str, fp: &str, to: &str, tp: &str) -> FlatConnection {
        let prov = CoreProv::root(syn());
        FlatConnection {
            from_module: from.into(),
            from_port: fp.into(),
            from_index: 0,
            to_module: to.into(),
            to_port: tp.into(),
            to_index: 0,
            map: patches_dsl::CableMap::scalar(1.0),
            provenance: prov.clone(),
            from_provenance: prov.clone(),
            to_provenance: prov,
        }
    }

    #[test]
    fn clean_patch_has_no_errors() {
        let mut flat = empty_flat();
        flat.modules = vec![osc("o1"), sum("mix", 1)];
        flat.connections = vec![conn("o1", "sine", "mix", "in")];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);
        assert!(matches!(bound.modules[0], BoundModule::Resolved(_)));
        assert!(matches!(bound.connections[0], BoundConnection::Resolved(_)));
    }

    #[test]
    fn unknown_type_produces_unresolved_module() {
        let mut flat = empty_flat();
        flat.modules = vec![FlatModule {
            id: "x".into(),
            type_name: "NoSuch".into(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: CoreProv::root(Span::new(SourceId::SYNTHETIC, 5, 10)),
            param_block_span: None,
            type_name_span: None,
        }];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 1);
        assert_eq!(bound.errors[0].code, BindErrorCode::UnknownModuleType);
        assert!(matches!(bound.modules[0], BoundModule::Unresolved(_)));
    }

    #[test]
    fn unknown_port_produces_unresolved_connection() {
        let mut flat = empty_flat();
        flat.modules = vec![osc("o1"), sum("mix", 1)];
        flat.connections = vec![conn("o1", "no_such_out", "mix", "in")];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 1);
        assert_eq!(bound.errors[0].code, BindErrorCode::UnknownPort);
        assert!(matches!(bound.connections[0], BoundConnection::Unresolved(_)));
    }

    #[test]
    fn unknown_module_in_connection_classified() {
        let mut flat = empty_flat();
        flat.modules = vec![sum("mix", 1)];
        flat.connections = vec![conn("ghost", "out", "mix", "in")];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 1);
        assert_eq!(bound.errors[0].code, BindErrorCode::UnknownModule);
    }

    /// A connection endpoint that names a *declared* module whose type failed
    /// to resolve must not also surface an `UnknownModule` error: the module
    /// already carries its own `UnknownModuleType`, and a spurious
    /// `UnknownModule` would trip the pipeline layering audit into a false
    /// `PV0001` (blaming stage 3a for a reference it correctly accepted).
    #[test]
    fn connection_to_unresolved_module_does_not_emit_unknown_module() {
        use patches_dsl::pipeline::PipelineAudit;
        let mut flat = empty_flat();
        // `verb` is declared, but its type is unknown → it resolves to an
        // Unresolved module (UnknownModuleType), absent from `by_id`.
        let bad = FlatModule {
            id: "verb".into(),
            type_name: "NoSuch".into(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
            param_block_span: None,
            type_name_span: None,
        };
        flat.modules = vec![bad, sum("mix", 1)];
        flat.connections = vec![conn("verb", "out", "mix", "in")];
        let bound = bind(&flat, &registry());

        // Exactly one error — the module's own UnknownModuleType — and no
        // knock-on UnknownModule for the connection.
        assert_eq!(
            bound.errors.len(),
            1,
            "expected only the module-type error, got {:?}",
            bound.errors
        );
        assert_eq!(bound.errors[0].code, BindErrorCode::UnknownModuleType);
        assert!(
            !bound.errors.iter().any(|e| e.code == BindErrorCode::UnknownModule),
            "declared-but-unresolved endpoint must not emit UnknownModule"
        );
        // And the layering audit must stay silent — no false PV0001.
        assert!(
            bound.layering_warnings().is_empty(),
            "declared-but-unresolved endpoint must not trip PV0001: {:?}",
            bound.layering_warnings()
        );
    }

    #[test]
    fn accumulates_multiple_errors() {
        let mut flat = empty_flat();
        flat.modules = vec![
            FlatModule {
                id: "a".into(),
                type_name: "NoSuch1".into(),
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: CoreProv::root(syn()),
                param_block_span: None,
                type_name_span: None,
            },
            FlatModule {
                id: "b".into(),
                type_name: "NoSuch2".into(),
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: CoreProv::root(syn()),
                param_block_span: None,
                type_name_span: None,
            },
        ];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 2);
    }

    #[test]
    fn unknown_parameter_reports_code() {
        let mut flat = empty_flat();
        flat.modules = vec![FlatModule {
            id: "o1".into(),
            type_name: "Osc".into(),
            shape: vec![],
            params: vec![("no_such_param".into(), Value::Scalar(Scalar::Float(1.0)))],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
            param_block_span: None,
            type_name_span: None,
        }];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 1);
        assert_eq!(bound.errors[0].code, BindErrorCode::UnknownParameter);
    }

    // ── Typed parameter error classification (ticket 0441) ──────────────

    #[test]
    fn param_conversion_unknown_maps_to_unknown_parameter() {
        let err = ParamConversionError::unknown("unknown parameter 'x'");
        assert_eq!(err.bind_code(), BindErrorCode::UnknownParameter);
    }

    #[test]
    fn param_conversion_type_mismatch_maps_to_invalid_parameter_type() {
        let err = ParamConversionError::type_mismatch("expected float, found string");
        assert_eq!(err.bind_code(), BindErrorCode::InvalidParameterType);
    }

    #[test]
    fn param_conversion_out_of_range_maps_to_parameter_conversion() {
        let err = ParamConversionError::out_of_range("invalid enum variant 'foo'");
        assert_eq!(err.bind_code(), BindErrorCode::ParameterConversion);
    }

    #[test]
    fn bind_classifies_type_mismatch_without_substring_matching() {
        // Osc's `frequency` is a float; passing a bool triggers the
        // `expected …, found …` branch of convert_value.
        let mut flat = empty_flat();
        flat.modules = vec![FlatModule {
            id: "o1".into(),
            type_name: "Osc".into(),
            shape: vec![],
            params: vec![(
                "frequency".into(),
                patches_dsl::ast::Value::Scalar(patches_dsl::ast::Scalar::Bool(true)),
            )],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
            param_block_span: None,
            type_name_span: None,
        }];
        let bound = bind(&flat, &registry());
        assert_eq!(bound.errors.len(), 1);
        assert_eq!(bound.errors[0].code, BindErrorCode::InvalidParameterType);
    }

    // ── Auto-summing of multi-fan-in ────────────────────────────────────

    #[test]
    fn two_mono_sources_into_one_input_synthesize_sum() {
        let mut flat = empty_flat();
        flat.modules = vec![osc("a"), osc("b"), sum("mix", 1)];
        flat.connections = vec![conn("a", "sine", "mix", "in"), conn("b", "sine", "mix", "in")];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);
        let sums: Vec<_> = bound
            .modules
            .iter()
            .filter_map(|m| match m {
                BoundModule::Resolved(r) if r.type_name == "Sum" => Some(r),
                _ => None,
            })
            .collect();
        let synth = sums
            .iter()
            .find(|r| r.id.name.starts_with("__autosum_"))
            .expect("auto-sum module synthesized");
        assert_eq!(synth.descriptor.shape.channels, 2);
        // 2 source→sum + 1 sum→mix.
        assert_eq!(bound.connections.len(), 3);
    }

    #[test]
    fn three_sources_synthesize_three_channel_sum() {
        let mut flat = empty_flat();
        flat.modules = vec![osc("a"), osc("b"), osc("c"), sum("mix", 1)];
        flat.connections = vec![
            conn("a", "sine", "mix", "in"),
            conn("b", "sine", "mix", "in"),
            conn("c", "sine", "mix", "in"),
        ];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);
        let synth = bound
            .modules
            .iter()
            .find_map(|m| match m {
                BoundModule::Resolved(r) if r.id.name.starts_with("__autosum_") => Some(r),
                _ => None,
            })
            .expect("auto-sum module synthesized");
        assert_eq!(synth.descriptor.shape.channels, 3);
        assert_eq!(bound.connections.len(), 4);
    }

    #[test]
    fn single_connection_is_not_rewritten() {
        let mut flat = empty_flat();
        flat.modules = vec![osc("o"), sum("mix", 1)];
        flat.connections = vec![conn("o", "sine", "mix", "in")];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty());
        assert_eq!(bound.modules.len(), 2);
        assert_eq!(bound.connections.len(), 1);
    }

    #[test]
    fn fan_in_preserves_per_source_cable_map() {
        // Each source carries its own scale; the rewrite must keep them on
        // the source→sum edge. The sum→target edge is identity.
        let prov = || CoreProv::root(syn());
        let scaled = |from: &str, scale: f64| FlatConnection {
            from_module: from.into(),
            from_port: "sine".into(),
            from_index: 0,
            to_module: "mix".into(),
            to_port: "in".into(),
            to_index: 0,
            map: patches_dsl::CableMap::scalar(scale),
            provenance: prov(),
            from_provenance: prov(),
            to_provenance: prov(),
        };
        let mut flat = empty_flat();
        flat.modules = vec![osc("a"), osc("b"), sum("mix", 1)];
        flat.connections = vec![scaled("a", 0.25), scaled("b", 0.75)];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);

        let mut source_scales: Vec<f32> = Vec::new();
        let mut sum_to_mix_scale: Option<f32> = None;
        for bc in &bound.connections {
            let BoundConnection::Resolved(rc) = bc else { continue };
            if rc.to_module.name.starts_with("__autosum_") {
                source_scales.push(rc.map.scale as f32);
            } else if rc.from_module.name.starts_with("__autosum_") {
                sum_to_mix_scale = Some(rc.map.scale as f32);
            }
        }
        source_scales.sort_by(|x, y| x.partial_cmp(y).unwrap());
        assert_eq!(source_scales, vec![0.25, 0.75]);
        assert_eq!(sum_to_mix_scale, Some(1.0));
    }

    // ── Auto-conversion of mono↔poly Audio (ADR 0074, ticket 0892) ────

    fn poly_osc(id: &str) -> FlatModule {
        FlatModule {
            id: id.into(),
            type_name: "PolyOsc".into(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
            param_block_span: None,
            type_name_span: None,
        }
    }

    fn mono_to_poly(id: &str) -> FlatModule {
        FlatModule {
            id: id.into(),
            type_name: "MonoToPoly".into(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
            param_block_span: None,
            type_name_span: None,
        }
    }

    #[test]
    fn mono_audio_to_poly_audio_synthesizes_monotopoly() {
        let mut flat = empty_flat();
        flat.modules = vec![osc("src"), poly_osc("dst")];
        flat.connections = vec![conn("src", "sine", "dst", "voct")];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);

        let conv = bound
            .modules
            .iter()
            .find_map(|m| match m {
                BoundModule::Resolved(r) if r.id.is_autoconv() => Some(r),
                _ => None,
            })
            .expect("auto-conv module synthesised");
        assert_eq!(conv.type_name, "MonoToPoly");
        assert!(conv.id.name.starts_with("__autoconv_dst_voct"));
        // Original edge replaced by src→conv and conv→dst.
        assert_eq!(bound.connections.len(), 2);
        let mut saw_src_to_conv = false;
        let mut saw_conv_to_dst = false;
        for bc in &bound.connections {
            let BoundConnection::Resolved(rc) = bc else { panic!() };
            if rc.from_module == QName::bare("src") && rc.to_module == conv.id {
                saw_src_to_conv = true;
            }
            if rc.from_module == conv.id && rc.to_module == QName::bare("dst") {
                saw_conv_to_dst = true;
            }
        }
        assert!(saw_src_to_conv && saw_conv_to_dst);
    }

    #[test]
    fn poly_audio_to_mono_audio_synthesizes_polytomono() {
        // PolyOsc.sine (poly Audio) → Sum.in (mono Audio).
        let mut flat = empty_flat();
        flat.modules = vec![poly_osc("voices"), sum("mix", 1)];
        flat.connections = vec![conn("voices", "sine", "mix", "in")];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);

        let conv = bound
            .modules
            .iter()
            .find_map(|m| match m {
                BoundModule::Resolved(r) if r.id.is_autoconv() => Some(r),
                _ => None,
            })
            .expect("auto-conv module synthesised");
        assert_eq!(conv.type_name, "PolyToMono");
        assert!(conv.id.name.starts_with("__autoconv_mix_in"));
        assert_eq!(bound.connections.len(), 2);
    }

    #[test]
    fn mono_to_poly_trigger_still_rejected() {
        // Mono Audio → poly Trigger is not Audio↔Audio; stays rejected.
        let mut flat = empty_flat();
        flat.modules = vec![osc("src"), poly_osc("dst")];
        flat.connections = vec![conn("src", "sine", "dst", "sync")];
        let bound = bind(&flat, &registry());
        assert!(!bound.errors.is_empty());
        assert_eq!(bound.errors[0].code, BindErrorCode::CableKindMismatch);
        let synth = bound
            .modules
            .iter()
            .any(|m| matches!(m, BoundModule::Resolved(r) if r.id.is_autoconv()));
        assert!(!synth, "no auto-conv module should be created");
    }

    #[test]
    fn explicit_monotopoly_does_not_double_convert() {
        // User-written MonoToPoly between mono src and poly dst is two
        // kind-matching edges; auto-conv must not fire.
        let mut flat = empty_flat();
        flat.modules = vec![osc("src"), mono_to_poly("m2p"), poly_osc("dst")];
        flat.connections = vec![
            conn("src", "sine", "m2p", "in"),
            conn("m2p", "out", "dst", "voct"),
        ];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);
        let synth_count = bound
            .modules
            .iter()
            .filter(|m| matches!(m, BoundModule::Resolved(r) if r.id.is_autoconv()))
            .count();
        assert_eq!(synth_count, 0);
        assert_eq!(bound.connections.len(), 2);
    }

    #[test]
    fn poly_fanin_into_mono_inserts_polysum_then_autoconv() {
        // Two poly Audio sources fan into a mono Audio input.
        // fan_in synthesises PolySum (output poly Audio); kind_conv then
        // inserts PolyToMono on the sum→target edge.
        let mut flat = empty_flat();
        flat.modules = vec![poly_osc("a"), poly_osc("b"), sum("mix", 1)];
        flat.connections = vec![
            conn("a", "sine", "mix", "in"),
            conn("b", "sine", "mix", "in"),
        ];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);
        let has_polysum = bound.modules.iter().any(|m| matches!(
            m, BoundModule::Resolved(r) if r.type_name == "PolySum" && r.id.is_autosum()
        ));
        let has_polytomono = bound.modules.iter().any(|m| matches!(
            m, BoundModule::Resolved(r) if r.type_name == "PolyToMono" && r.id.is_autoconv()
        ));
        assert!(has_polysum, "expected synth PolySum from fan-in");
        assert!(has_polytomono, "expected synth PolyToMono from kind-conv after fan-in");
    }

    #[test]
    fn two_consumers_each_get_their_own_converter() {
        // mono src drives two poly dst voct inputs. Each edge converts
        // independently — ADR 0074 doesn't share converters across
        // consumers (fusion makes the duplication free).
        let mut flat = empty_flat();
        flat.modules = vec![osc("src"), poly_osc("a"), poly_osc("b")];
        flat.connections = vec![
            conn("src", "sine", "a", "voct"),
            conn("src", "sine", "b", "voct"),
        ];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);
        let synth_count = bound
            .modules
            .iter()
            .filter(|m| matches!(m, BoundModule::Resolved(r) if r.id.is_autoconv()))
            .count();
        assert_eq!(synth_count, 2);
    }

    #[test]
    fn poly_audio_to_stereo_synthesizes_polytomono() {
        // PolyOsc.sine (poly Audio) → FdnReverb.in (stereo): ADR 0074 §
        // poly→stereo case. Auto-conv inserts PolyToMono; the remaining
        // mono→stereo edge is broadcast-coerced at runtime ModuleGraph
        // construction.
        let verb = FlatModule {
            id: "verb".into(),
            type_name: "FdnReverb".into(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: CoreProv::root(syn()),
            param_block_span: None,
            type_name_span: None,
        };
        let mut flat = empty_flat();
        flat.modules = vec![poly_osc("voices"), verb];
        flat.connections = vec![conn("voices", "sine", "verb", "in")];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);

        let conv = bound
            .modules
            .iter()
            .find_map(|m| match m {
                BoundModule::Resolved(r) if r.id.is_autoconv() => Some(r),
                _ => None,
            })
            .expect("auto-conv module synthesised");
        assert_eq!(conv.type_name, "PolyToMono");
        assert!(conv.id.name.starts_with("__autoconv_verb_in"));
        assert_eq!(bound.connections.len(), 2);

        // The synthesised conv→target edge is Mono→Stereo (broadcast).
        let mut saw_conv_to_stereo = false;
        for bc in &bound.connections {
            let BoundConnection::Resolved(rc) = bc else { panic!() };
            if rc.from_module == conv.id {
                assert_eq!(rc.from_kind, patches_core::cables::CableKind::Mono);
                assert_eq!(rc.to_kind, patches_core::cables::CableKind::Stereo);
                saw_conv_to_stereo = true;
            }
        }
        assert!(saw_conv_to_stereo);
    }

    #[test]
    fn two_independent_fan_ins_each_synthesize_a_sum() {
        let mut flat = empty_flat();
        flat.modules = vec![
            osc("a"),
            osc("b"),
            osc("c"),
            osc("d"),
            sum("mixL", 2),
        ];
        // mixL.in/0 driven by a,b ; mixL.in/1 driven by c,d.
        let mut c_a = conn("a", "sine", "mixL", "in"); c_a.to_index = 0;
        let mut c_b = conn("b", "sine", "mixL", "in"); c_b.to_index = 0;
        let mut c_c = conn("c", "sine", "mixL", "in"); c_c.to_index = 1;
        let mut c_d = conn("d", "sine", "mixL", "in"); c_d.to_index = 1;
        flat.connections = vec![c_a, c_b, c_c, c_d];
        let bound = bind(&flat, &registry());
        assert!(bound.errors.is_empty(), "unexpected errors: {:?}", bound.errors);
        let synth_count = bound
            .modules
            .iter()
            .filter(|m| matches!(m, BoundModule::Resolved(r) if r.id.name.starts_with("__autosum_")))
            .count();
        assert_eq!(synth_count, 2);
    }
}
