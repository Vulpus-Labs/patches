use super::*;
use crate::cable_pool::CablePool;

fn mono_pool(value: f32) -> Vec<CableValue> {
    vec![CableValue::mono(value)]
}

fn poly_pool(channels: [f32; 16]) -> Vec<CableValue> {
    vec![CableValue::poly(channels)]
}

// MonoInput::read --------------------------------------------------------

#[test]
fn mono_input_read_scale_one() {
    let pool = mono_pool(2.5);
    let port = MonoInput::scalar(0, 1.0);
    assert_eq!(port.read(&pool), 2.5);
}

#[test]
fn mono_input_read_with_scale() {
    let pool = mono_pool(2.0);
    let port = MonoInput::scalar(0, 0.5);
    assert_eq!(port.read(&pool), 1.0);
}

// PolyInput::read --------------------------------------------------------

#[test]
fn poly_input_read_applies_scale_to_all_channels() {
    let channels: [f32; 16] = std::array::from_fn(|i| i as f32);
    let pool = poly_pool(channels);
    let port = PolyInput::scalar(0, 2.0);
    let result = port.read(&pool);
    for (i, &v) in result.iter().enumerate() {
        assert_eq!(v, i as f32 * 2.0, "channel {i} mismatch");
    }
}

// Kind-mismatch fallback (release builds only — debug_assert fires in debug) --

#[cfg(not(debug_assertions))]
#[test]
fn mono_input_kind_mismatch_returns_zero() {
    let pool = vec![CableValue::poly([1.0; 16])];
    let port = MonoInput::scalar(0, 1.0);
    assert_eq!(port.read(&pool), 0.0);
}

#[cfg(not(debug_assertions))]
#[test]
fn poly_input_kind_mismatch_returns_zero() {
    let pool = vec![CableValue::mono(1.0)];
    let port = PolyInput::scalar(0, 1.0);
    assert_eq!(port.read(&pool), [0.0; 16]);
}

// is_connected -----------------------------------------------------------

#[test]
fn is_connected_defaults_false_for_all_port_types() {
    assert!(!MonoInput::default().is_connected(), "MonoInput default should be disconnected");
    assert!(!PolyInput::default().is_connected(), "PolyInput default should be disconnected");
    assert!(!MonoOutput::default().is_connected(), "MonoOutput default should be disconnected");
    assert!(!PolyOutput::default().is_connected(), "PolyOutput default should be disconnected");

    // When explicitly connected, is_connected returns true.
    assert!(MonoInput::scalar(0, 1.0).is_connected(), "MonoInput connected");
    assert!(PolyInput::scalar(0, 1.0).is_connected(), "PolyInput connected");
    assert!(MonoOutput { cable_idx: 0, connected: true }.is_connected(), "MonoOutput connected");
    assert!(PolyOutput { cable_idx: 0, connected: true }.is_connected(), "PolyOutput connected");
}

// MonoOutput::write / PolyOutput::write round-trips ---------------------

#[test]
fn mono_output_write_round_trip() {
    let mut pool = vec![CableValue::mono(0.0)];
    let port = MonoOutput { cable_idx: 0, connected: true };
    port.write(&mut pool, 2.5);
    assert_eq!(pool[0].as_mono(), 2.5);
}

#[test]
fn poly_output_write_round_trip() {
    let mut pool = vec![CableValue::poly([0.0; 16])];
    let port = PolyOutput { cable_idx: 0, connected: true };
    let data: [f32; 16] = std::array::from_fn(|i| i as f32 * 0.1);
    port.write(&mut pool, data);
    assert_eq!(pool[0].as_poly(), data);
}

fn make_cable_pool(values: &[CableValue]) -> Vec<[CableValue; 2]> {
    values.iter().map(|&v| [v, v]).collect()
}

/// Absolute `cable_idx` for cycle logical slot `i` (ADR 0072 phase 5).
const fn cycle_idx(i: usize) -> usize {
    super::SCRATCH_CAPACITY + i
}

use crate::test_support::reserved_scratch;

// ── GateInput ────────────────────────────────────────────────────────

#[test]
fn gate_rising_and_falling_edges() {
    let mut pool = make_cable_pool(&[CableValue::mono(0.0)]);
    let mut g = GateInput {
        inner: MonoInput::scalar(cycle_idx(0), 1.0),
        ..Default::default()
    };

    // Low → no edges
    {
        let mut scratch = reserved_scratch();
        let cp = CablePool::new(&mut scratch, &mut pool, 0);
        let e = g.tick(&cp);
        assert!(!e.rose);
        assert!(!e.fell);
        assert!(!e.is_high);
    }

    // Go high → rising edge
    pool[0] = [CableValue::mono(1.0); 2];
    {
        let mut scratch = reserved_scratch();
        let cp = CablePool::new(&mut scratch, &mut pool, 0);
        let e = g.tick(&cp);
        assert!(e.rose);
        assert!(!e.fell);
        assert!(e.is_high);
    }

    // Stay high → no edges, still high
    {
        let mut scratch = reserved_scratch();
        let cp = CablePool::new(&mut scratch, &mut pool, 0);
        let e = g.tick(&cp);
        assert!(!e.rose);
        assert!(!e.fell);
        assert!(e.is_high);
    }

    // Go low → falling edge
    pool[0] = [CableValue::mono(0.0); 2];
    {
        let mut scratch = reserved_scratch();
        let cp = CablePool::new(&mut scratch, &mut pool, 0);
        let e = g.tick(&cp);
        assert!(!e.rose);
        assert!(e.fell);
        assert!(!e.is_high);
    }
}

// ── TriggerInput / PolyTriggerInput (ADR 0047) ─────────────────

#[test]
fn sub_trigger_zero_is_no_event() {
    let mut pool = make_cable_pool(&[CableValue::mono(0.0)]);
    let mut scratch = reserved_scratch();
    let cp = CablePool::new(&mut scratch, &mut pool, 0);
    let t = TriggerInput {
        inner: MonoInput::scalar(cycle_idx(0), 1.0),
    };
    assert_eq!(t.tick(&cp), None);
}

#[test]
fn sub_trigger_positive_is_event_with_frac() {
    let mut pool = make_cable_pool(&[CableValue::mono(0.37)]);
    let mut scratch = reserved_scratch();
    let cp = CablePool::new(&mut scratch, &mut pool, 0);
    let t = TriggerInput {
        inner: MonoInput::scalar(cycle_idx(0), 1.0),
    };
    assert_eq!(t.tick(&cp), Some(0.37));
}

#[test]
fn sub_trigger_one_is_boundary_event() {
    let mut pool = make_cable_pool(&[CableValue::mono(1.0)]);
    let mut scratch = reserved_scratch();
    let cp = CablePool::new(&mut scratch, &mut pool, 0);
    let t = TriggerInput {
        inner: MonoInput::scalar(cycle_idx(0), 1.0),
    };
    assert_eq!(t.tick(&cp), Some(1.0));
}

#[test]
fn poly_sub_trigger_per_voice() {
    let mut channels = [0.0f32; 16];
    channels[0] = 0.25;
    channels[5] = 0.9;
    let mut pool = make_cable_pool(&[CableValue::poly(channels)]);
    let mut scratch = reserved_scratch();
    let cp = CablePool::new(&mut scratch, &mut pool, 0);
    let t = PolyTriggerInput {
        inner: PolyInput::scalar(cycle_idx(0), 1.0),
    };
    let out = t.tick(&cp);
    assert_eq!(out[0], Some(0.25));
    assert_eq!(out[1], None);
    assert_eq!(out[5], Some(0.9));
}

// ── CableKind connection compatibility (ADR 0047) ────────────────────

#[test]
fn cable_kind_helpers() {
    assert!(!CableKind::Mono.is_poly());
    assert!(CableKind::Poly.is_poly());
    assert!(!CableKind::Stereo.is_poly());

    assert!(!CableKind::Mono.uses_poly_storage());
    assert!(CableKind::Poly.uses_poly_storage());
    assert!(CableKind::Stereo.uses_poly_storage());
}

// ── StereoInput / StereoOutput ───────────────────────────────────────

#[test]
fn stereo_output_writes_lr_to_poly_slot() {
    let mut pool = vec![CableValue::poly([0.0; 16])];
    let port = StereoOutput { cable_idx: 0, connected: true };
    port.write(&mut pool, 0.25, -0.5);
    let channels = pool[0].as_poly();
    assert_eq!(channels[0], 0.25);
    assert_eq!(channels[1], -0.5);
    assert_eq!(channels[2], 0.0);
}

#[test]
fn stereo_input_reads_lr_with_scale() {
    let mut frame = [0.0f32; 16];
    frame[0] = 1.0;
    frame[1] = 2.0;
    let pool = vec![CableValue::poly(frame)];
    let port = StereoInput::scalar(0, 0.5);
    assert_eq!(port.read(&pool), (0.5, 1.0));
}

#[test]
fn stereo_input_round_trip_through_output() {
    let mut pool = vec![CableValue::poly([0.0; 16])];
    let out = StereoOutput { cable_idx: 0, connected: true };
    out.write(&mut pool, 0.7, -0.3);
    let inp = StereoInput::scalar(0, 1.0);
    assert_eq!(inp.read(&pool), (0.7, -0.3));
}

// ── PolyGateInput ────────────────────────────────────────────────────

#[test]
fn poly_gate_per_voice_edges() {
    let mut pool = make_cable_pool(&[CableValue::poly([0.0; 16])]);
    let mut g = PolyGateInput {
        inner: PolyInput::scalar(cycle_idx(0), 1.0),
        ..Default::default()
    };

    // All low
    {
        let mut scratch = reserved_scratch();
        let cp = CablePool::new(&mut scratch, &mut pool, 0);
        let _ = g.tick(&cp);
    }

    // Voice 2 goes high
    let mut channels = [0.0f32; 16];
    channels[2] = 1.0;
    pool[0] = [CableValue::poly(channels); 2];
    {
        let mut scratch = reserved_scratch();
        let cp = CablePool::new(&mut scratch, &mut pool, 0);
        let result = g.tick(&cp);
        assert!(result[2].rose);
        assert!(result[2].is_high);
        assert!(!result[0].rose);
        assert!(!result[0].is_high);
    }
}
