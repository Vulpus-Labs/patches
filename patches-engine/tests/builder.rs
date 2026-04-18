//! Integration tests for `patches_planner::builder`.
//!
//! Moved from an in-crate `#[cfg(test)]` module when the builder
//! was extracted to `patches-planner` (ticket 0513); the tests keep
//! living here because they exercise `ModulePool` from `patches-engine`.

mod builder {
    pub mod graph_build;
    pub mod planner;
    pub mod pool;

    use patches_core::{
        AudioEnvironment, AUDIO_OUT_L, CablePool, CableValue, InstanceId, Module, ModuleGraph,
        ModuleShape, NodeId, PortRef,
    };
    use patches_core::parameter_map::{ParameterMap, ParameterValue};
    use patches_modules::{AudioOut, Oscillator, Sum};
    use patches_planner::{
        BuildError, BuildErrorKind, ExecutionPlan, ModuleAllocState, PatchBuilder, PlanError,
        PlannerState,
    };
    use patches_registry::Registry;
    use patches_engine::ModulePool;
    pub(crate) use std::collections::HashSet;

    pub(crate) fn p(name: &'static str) -> PortRef {
        PortRef { name, index: 0 }
    }

    pub(crate) fn hz_to_voct(hz: f32) -> f32 {
        (hz / 16.351_598_f32).log2()
    }

    pub(crate) fn pool_index_for(state: &PlannerState, node_id: &NodeId) -> usize {
        let ns = &state.nodes[node_id];
        state.module_alloc.pool_map[&ns.instance_id]
    }

    pub(crate) fn sine_to_audio_out_graph() -> ModuleGraph {
        let mut graph = ModuleGraph::new();
        let sine_desc = Oscillator::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let out_desc = AudioOut::describe(&ModuleShape { channels: 0, length: 0, ..Default::default() });
        let mut sine_params = ParameterMap::new();
        sine_params.insert("frequency".to_string(), ParameterValue::Float(hz_to_voct(440.0)));
        graph.add_module("a_sine", sine_desc, &sine_params).unwrap();
        graph.add_module("b_out", out_desc, &ParameterMap::new()).unwrap();
        graph
            .connect(&NodeId::from("a_sine"), p("sine"), &NodeId::from("b_out"), p("in_left"), 1.0)
            .unwrap();
        graph
            .connect(&NodeId::from("a_sine"), p("sine"), &NodeId::from("b_out"), p("in_right"), 1.0)
            .unwrap();
        graph
    }

    pub(crate) fn make_buffer_pool(capacity: usize) -> Vec<[CableValue; 2]> {
        (0..capacity).map(|_| [CableValue::Mono(0.0), CableValue::Mono(0.0)]).collect()
    }

    pub(crate) fn default_registry() -> Registry {
        patches_modules::default_registry()
    }

    pub(crate) fn default_env() -> AudioEnvironment {
        AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32, hosted: false }
    }

    pub(crate) fn default_builder() -> PatchBuilder {
        PatchBuilder::new(256, 256)
    }

    pub(crate) fn default_build(graph: &ModuleGraph) -> (ExecutionPlan, PlannerState, ModulePool) {
        let registry = default_registry();
        let env = default_env();
        let (mut plan, state) = default_builder()
            .build_patch(graph, &registry, &env, &PlannerState::empty())
            .expect("build should succeed");
        let mut module_pool = ModulePool::new(256);
        for (idx, m) in plan.new_modules.drain(..) {
            module_pool.install(idx, m);
        }
        (plan, state, module_pool)
    }
}
