//! E107 ticket 0623 — 10 000 `process` cycles through the FFI gain
//! path under `audio-thread-allocator-trap`. When the feature is off
//! the guard is a no-op; the test still runs and exercises the path.

#[global_allocator]
static A: patches_alloc_trap::TrappingAllocator = patches_alloc_trap::TrappingAllocator;

use patches_alloc_trap::{trap_hits, NoAllocGuard};
use patches_core::cable_pool::CablePool;
use patches_core::cables::{CableValue, InputPort, MonoInput, MonoOutput, OutputPort};
use patches_core::modules::{InstanceId, ModuleShape, ParameterMap, ParameterValue};
use patches_core::param_frame::{pack_into, ParamFrame, ParamView, ParamViewIndex};
use patches_core::param_layout::{compute_layout, defaults_from_descriptor};
use patches_core::AudioEnvironment;
use patches_ffi::loader::load_plugin;
use patches_integration_tests::dylib_path;
use patches_registry::ModuleBuilder;

fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 48_000.0,
        poly_voices: 1,
        periodic_update_interval: 64,
        hosted: false,
    }
}

#[test]
fn gain_ffi_ten_thousand_cycles_no_alloc() {
    let hits_before = trap_hits();
    let mut builders =
        load_plugin(&dylib_path("test-gain-plugin")).expect("load gain");
    let builder = builders.remove(0);
    let shape = ModuleShape { channels: 1, length: 0, ..Default::default() };
    let mut module = builder
        .build(&env(), &shape, &ParameterMap::new(), InstanceId::next())
        .expect("build");
    let inputs = vec![InputPort::Mono(MonoInput {
        cable_idx: 0,
        scale: 1.0,
        connected: true,
    })];
    let outputs = vec![OutputPort::Mono(MonoOutput {
        cable_idx: 1,
        connected: true,
    })];
    module.set_ports(&inputs, &outputs);

    // Pre-build a validated param frame so the update path itself
    // does not allocate inside the guard.
    let descriptor = module.descriptor().clone();
    let layout = compute_layout(&descriptor);
    let defaults = defaults_from_descriptor(&descriptor);
    let index = ParamViewIndex::from_layout(&layout);
    let mut param_frame = ParamFrame::with_layout(&layout);
    {
        let mut p = ParameterMap::new();
        p.insert_param("gain", 0, ParameterValue::Float(0.5));
        pack_into(&layout, &defaults, &p, &mut param_frame).unwrap();
    }

    let mut pool = vec![
        [CableValue::Mono(0.25); 2],
        [CableValue::Mono(0.0); 2],
    ];

    // Warm-up outside the guard.
    for _ in 0..32 {
        let mut cp = CablePool::new(&mut pool, 0);
        module.process(&mut cp);
    }

    {
        let _g = NoAllocGuard::enter();
        for i in 0..10_000 {
            {
                let mut cp = CablePool::new(&mut pool, (i & 1) as usize);
                module.process(&mut cp);
            }
            // Every 128 iterations interleave a param update.
            if i % 128 == 0 {
                let view = ParamView::new(&index, &param_frame);
                module.update_validated_parameters(&view);
            }
        }
    }

    drop(module);
    drop(builders);
    assert_eq!(trap_hits(), hits_before);
}
