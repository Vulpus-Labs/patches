//! Planner-side structural rebuild tests (ticket 0740, ADR 0060).
//!
//! A surviving node whose `StructuralParams` differs across re-plans must be
//! reclassified as a fresh install: a new `Module` instance is constructed
//! via `Module::prepare` with the new structural blob, the previous slot is
//! tombstoned, and `BuildError` from `prepare` is surfaced to the caller
//! with the planner state left untouched so the running engine keeps the
//! prior plan.

use patches_core::build_error::BuildError;
use patches_core::cable_pool::CablePool;
use patches_core::param_frame::ParamView;
use patches_core::{
    AudioEnvironment, InstanceId, Module, ModuleDescriptor, ModuleGraph, ModuleShape, NodeId,
    ParameterMap, StructuralParams, StructuralValue,
};
use patches_registry::Registry;

use crate::builder::PatchBuilder;
use crate::state::PlannerState;

struct StructuralProbe {
    instance_id: InstanceId,
    descriptor: ModuleDescriptor,
    seen_path: String,
}

impl Module for StructuralProbe {
    fn describe(_shape: &ModuleShape) -> ModuleDescriptor {
        ModuleDescriptor::new("StructuralProbe", ModuleShape { channels: 0 })
            .structural_string_param("path", &[])
    }

    fn prepare(
        _env: &AudioEnvironment,
        descriptor: ModuleDescriptor,
        instance_id: InstanceId,
        structural: &StructuralParams,
    ) -> Result<Self, BuildError> {
        let path = structural.get_string("path", 0).unwrap_or("").to_string();
        if path == "fail" {
            return Err(BuildError::Custom {
                module: "StructuralProbe",
                message: "intentional structural-prepare failure".into(),
                origin: None,
            });
        }
        Ok(Self { instance_id, descriptor, seen_path: path })
    }

    fn update_validated_parameters(&mut self, _params: &ParamView<'_>) {}
    fn descriptor(&self) -> &ModuleDescriptor { &self.descriptor }
    fn instance_id(&self) -> InstanceId { self.instance_id }
    fn process(&mut self, _pool: &mut CablePool<'_>) {}
    fn as_any(&self) -> &dyn std::any::Any { self }
}

fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 44100.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.register::<StructuralProbe>();
    r
}

fn graph_with_probe(path: &str) -> ModuleGraph {
    let mut g = ModuleGraph::new();
    let desc = StructuralProbe::describe(&ModuleShape { channels: 0 });
    let mut sp = StructuralParams::new();
    sp.insert("path", 0, StructuralValue::String(path.into()));
    g.add_module_with_structural("probe", desc, &ParameterMap::new(), &sp).unwrap();
    g
}

#[test]
fn structural_change_rebuilds_instance() {
    let registry = registry();
    let env = env();
    let builder = PatchBuilder::new(64, 64);
    let graph = graph_with_probe("a.wav");

    let (plan_a, state_a) = builder
        .build_patch(&graph, &registry, &env, &PlannerState::empty())
        .expect("first build succeeds");

    assert_eq!(plan_a.new_modules.len(), 1, "first build installs the probe");
    let id_a = state_a.nodes[&NodeId::from("probe")].instance_id;
    let slot_a = state_a.module_alloc.pool_map[&id_a];
    let installed_path_a = plan_a.new_modules[0]
        .1
        .as_any()
        .downcast_ref::<StructuralProbe>()
        .unwrap()
        .seen_path
        .clone();
    assert_eq!(installed_path_a, "a.wav");

    // Second build: identical graph, identical realtime params, only the
    // structural value changes. Surviving-node check must reclassify as
    // Install; the old instance is tombstoned.
    let graph_b = graph_with_probe("b.wav");
    let (plan_b, state_b) = builder
        .build_patch(&graph_b, &registry, &env, &state_a)
        .expect("second build succeeds");

    assert_eq!(plan_b.new_modules.len(), 1, "structural change installs a fresh module");
    assert_eq!(plan_b.tombstones, vec![slot_a], "old slot is tombstoned");

    let id_b = state_b.nodes[&NodeId::from("probe")].instance_id;
    assert_ne!(id_a, id_b, "structural rebuild mints a new InstanceId");

    let installed_path_b = plan_b.new_modules[0]
        .1
        .as_any()
        .downcast_ref::<StructuralProbe>()
        .unwrap()
        .seen_path
        .clone();
    assert_eq!(installed_path_b, "b.wav", "new instance reflects the new structural blob");

    // Third build: structural unchanged → no rebuild (Update path).
    let graph_b2 = graph_with_probe("b.wav");
    let (plan_c, _state_c) = builder
        .build_patch(&graph_b2, &registry, &env, &state_b)
        .expect("third build succeeds");
    assert!(plan_c.new_modules.is_empty(), "no structural diff → no install");
    assert!(plan_c.tombstones.is_empty(), "no structural diff → no tombstone");
}

#[test]
fn structural_prepare_failure_surfaces_build_error() {
    let registry = registry();
    let env = env();
    let builder = PatchBuilder::new(64, 64);
    let graph = graph_with_probe("good.wav");

    // First build with a good path — establishes baseline state.
    let (_plan, state_a) = builder
        .build_patch(&graph, &registry, &env, &PlannerState::empty())
        .expect("baseline build succeeds");
    let id_a = state_a.nodes[&NodeId::from("probe")].instance_id;

    // Second build with the magic "fail" path — `prepare` returns Err.
    let graph_fail = graph_with_probe("fail");
    let result = builder
        .build_patch(&graph_fail, &registry, &env, &state_a);
    let err = match result {
        Ok(_) => panic!("structural prepare failure must surface as BuildError"),
        Err(e) => e,
    };
    let msg = format!("{err}");
    assert!(msg.contains("intentional"), "error message must propagate: {msg}");

    // The caller (planner) is responsible for retaining `state_a` on error;
    // verify that nothing in `state_a` was mutated in-place.
    assert_eq!(state_a.nodes[&NodeId::from("probe")].instance_id, id_a);
}
