//! Live-reload fast path for host-control manifest changes (ticket 0812).
//!
//! Drives `compile_only` repeatedly through a single `HostRuntime`, then
//! inspects the resulting `ExecutionPlan` to verify the audio-side
//! reload semantics:
//!
//! - Metadata-only change (range / default on an unchanged set of names):
//!   the synth `~host_control` instance stays put — no tombstone, no
//!   reinstall.
//! - Rename within a fixed channel count: same as above; the synth
//!   module's `slot_offset` / `kind` arrays are positional and unchanged
//!   under a same-cardinality alias rename, so audio continuity is
//!   preserved.
//! - Adding a new control: channel count changes → drop+replace of the
//!   `~host_control` instance via the planner's existing size-change path.
//!
//! CLAP-side parameter republish is exercised separately by
//! `patches_plugin_common::host_control_registry` unit tests; this test
//! is the audio-thread complement.

use patches_core::AudioEnvironment;
use patches_host::{HostBuilder, InMemorySource};

fn env() -> AudioEnvironment {
    AudioEnvironment {
        sample_rate: 44_100.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    }
}

const SYNTH_HOST_CONTROL: &str = "~host_control";

fn patch_with_one_knob(name: &str, low: &str, high: &str) -> String {
    format!(
        r#"
patch {{
    knob {name} {{ low: {low}, high: {high} }}
    module osc : Osc()
    module out : AudioOut
    ~{name} -> osc.voct
    osc.sine -> out.in
}}
"#
    )
}

fn patch_with_two_knobs() -> String {
    r#"
patch {
    knob a { low: 1Hz, high: 2Hz }
    knob z { low: 1Hz, high: 2Hz }
    module osc : Osc()
    module out : AudioOut
    ~a -> osc.voct
    ~z -> osc.fm
    osc.sine -> out.in
}
"#
    .to_string()
}

/// Find the pool slot holding `~host_control` from the prior plan's
/// monitor meta. Returns `None` if no such slot exists.
fn host_control_slot(meta: &patches_planner::MonitorMeta) -> Option<usize> {
    meta.names.iter().enumerate().find_map(|(i, n)| match n {
        Some(s) if s.as_ref() == SYNTH_HOST_CONTROL => Some(i),
        _ => None,
    })
}

/// Returns true if `pool_index` is in any of `plan.tombstones` or
/// `plan.new_modules`.
fn touched_in_plan(plan: &patches_planner::ExecutionPlan, pool_index: usize) -> bool {
    plan.tombstones.contains(&pool_index)
        || plan.new_modules.iter().any(|(i, _)| *i == pool_index)
}

#[test]
fn metadata_only_change_does_not_drop_replace_host_control_instance() {
    let registry = patches_modules::default_registry();
    let mut runtime = HostBuilder::new()
        .build(env())
        .expect("cleanup thread spawn");
    runtime.set_monitor(true);

    // Compile #1: install the synth module for the first time.
    let src1 = InMemorySource::new(patch_with_one_knob("knob_a", "1Hz", "2Hz"));
    let out1 = runtime.compile_only(&src1, &registry).expect("compile #1");
    let meta1 = out1.common.monitor.as_ref().expect("monitor meta enabled");
    let hc_slot = host_control_slot(meta1).expect("synth instance present in plan #1");
    assert!(
        out1.common
            .plan
            .new_modules
            .iter()
            .any(|(i, _)| *i == hc_slot),
        "first compile must install ~host_control",
    );

    // Compile #2: same name, only metadata (range) changed.
    let src2 = InMemorySource::new(patch_with_one_knob("knob_a", "100Hz", "200Hz"));
    let out2 = runtime.compile_only(&src2, &registry).expect("compile #2");
    assert!(
        !touched_in_plan(&out2.common.plan, hc_slot),
        "metadata-only manifest change must not drop+replace ~host_control \
         (slot {hc_slot}, tombstones={:?}, new_modules={:?})",
        out2.common.plan.tombstones,
        out2.common
            .plan
            .new_modules
            .iter()
            .map(|(i, _)| *i)
            .collect::<Vec<_>>(),
    );
}

#[test]
fn rename_only_does_not_drop_replace_host_control_instance() {
    let registry = patches_modules::default_registry();
    let mut runtime = HostBuilder::new()
        .build(env())
        .expect("cleanup thread spawn");
    runtime.set_monitor(true);

    // Compile #1: knob `freq` → osc.voct.
    let src1 = InMemorySource::new(patch_with_one_knob("freq", "1Hz", "2Hz"));
    let out1 = runtime.compile_only(&src1, &registry).expect("compile #1");
    let meta1 = out1.common.monitor.as_ref().expect("monitor meta");
    let hc_slot = host_control_slot(meta1).expect("synth instance");

    // Compile #2: same shape (1 knob), renamed.
    let src2 = InMemorySource::new(patch_with_one_knob("tone", "1Hz", "2Hz"));
    let out2 = runtime.compile_only(&src2, &registry).expect("compile #2");
    assert!(
        !touched_in_plan(&out2.common.plan, hc_slot),
        "rename-only must not drop+replace ~host_control \
         (slot {hc_slot}, tombstones={:?}, new_modules={:?})",
        out2.common.plan.tombstones,
        out2.common
            .plan
            .new_modules
            .iter()
            .map(|(i, _)| *i)
            .collect::<Vec<_>>(),
    );
}

#[test]
fn shape_change_drops_and_replaces_host_control_instance() {
    let registry = patches_modules::default_registry();
    let mut runtime = HostBuilder::new()
        .build(env())
        .expect("cleanup thread spawn");
    runtime.set_monitor(true);

    // Compile #1: one knob wired to voct.
    let src1 = InMemorySource::new(patch_with_one_knob("a", "1Hz", "2Hz"));
    let out1 = runtime.compile_only(&src1, &registry).expect("compile #1");
    let meta1 = out1.common.monitor.as_ref().expect("monitor meta");
    let old_hc_slot = host_control_slot(meta1).expect("synth in plan #1");

    // Compile #2: add a second knob — synth's `channels` axis goes 1 → 2,
    // which the planner treats as a shape change (drop+replace).
    let src2 = InMemorySource::new(patch_with_two_knobs());
    let out2 = runtime.compile_only(&src2, &registry).expect("compile #2");

    assert!(
        out2.common.plan.tombstones.contains(&old_hc_slot),
        "adding a host control must tombstone the old ~host_control slot \
         (expected {old_hc_slot} in {:?})",
        out2.common.plan.tombstones,
    );
    let meta2 = out2.common.monitor.as_ref().expect("monitor meta #2");
    let new_hc_slot = host_control_slot(meta2).expect("synth in plan #2");
    assert!(
        out2.common
            .plan
            .new_modules
            .iter()
            .any(|(i, _)| *i == new_hc_slot),
        "adding a host control must install a fresh ~host_control instance",
    );
}
