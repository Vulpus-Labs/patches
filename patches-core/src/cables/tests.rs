use super::*;
use crate::cable_pool::CablePool;

fn mono_pool(value: f32) -> Vec<CableValue> {
    vec![CableValue::Mono(value)]
}

fn poly_pool(channels: [f32; 16]) -> Vec<CableValue> {
    vec![CableValue::Poly(channels)]
}

// MonoInput::read --------------------------------------------------------

#[test]
fn mono_input_read_scale_one() {
    let pool = mono_pool(2.5);
    let port = MonoInput { cable_idx: 0, scale: 1.0, connected: true };
    assert_eq!(port.read(&pool), 2.5);
}

#[test]
fn mono_input_read_with_scale() {
    let pool = mono_pool(2.0);
    let port = MonoInput { cable_idx: 0, scale: 0.5, connected: true };
    assert_eq!(port.read(&pool), 1.0);
}

// PolyInput::read --------------------------------------------------------

#[test]
fn poly_input_read_applies_scale_to_all_channels() {
    let channels: [f32; 16] = std::array::from_fn(|i| i as f32);
    let pool = poly_pool(channels);
    let port = PolyInput { cable_idx: 0, scale: 2.0, connected: true };
    let result = port.read(&pool);
    for (i, &v) in result.iter().enumerate() {
        assert_eq!(v, i as f32 * 2.0, "channel {i} mismatch");
    }
}

// Kind-mismatch fallback (release builds only — debug_assert fires in debug) --

#[cfg(not(debug_assertions))]
#[test]
fn mono_input_kind_mismatch_returns_zero() {
    let pool = vec![CableValue::Poly([1.0; 16])];
    let port = MonoInput { cable_idx: 0, scale: 1.0, connected: true };
    assert_eq!(port.read(&pool), 0.0);
}

#[cfg(not(debug_assertions))]
#[test]
fn poly_input_kind_mismatch_returns_zero() {
    let pool = vec![CableValue::Mono(1.0)];
    let port = PolyInput { cable_idx: 0, scale: 1.0, connected: true };
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
    assert!(MonoInput { cable_idx: 0, scale: 1.0, connected: true }.is_connected(), "MonoInput connected");
    assert!(PolyInput { cable_idx: 0, scale: 1.0, connected: true }.is_connected(), "PolyInput connected");
    assert!(MonoOutput { cable_idx: 0, connected: true }.is_connected(), "MonoOutput connected");
    assert!(PolyOutput { cable_idx: 0, connected: true }.is_connected(), "PolyOutput connected");
}

// MonoOutput::write / PolyOutput::write round-trips ---------------------

#[test]
fn mono_output_write_round_trip() {
    let mut pool = vec![CableValue::Mono(0.0)];
    let port = MonoOutput { cable_idx: 0, connected: true };
    port.write(&mut pool, 2.5);
    match pool[0] {
        CableValue::Mono(v) => assert_eq!(v, 2.5),
        _ => panic!("expected CableValue::Mono"),
    }
}

#[test]
fn poly_output_write_round_trip() {
    let mut pool = vec![CableValue::Poly([0.0; 16])];
    let port = PolyOutput { cable_idx: 0, connected: true };
    let data: [f32; 16] = std::array::from_fn(|i| i as f32 * 0.1);
    port.write(&mut pool, data);
    match pool[0] {
        CableValue::Poly(channels) => assert_eq!(channels, data),
        _ => panic!("expected CableValue::Poly"),
    }
}

// ── TriggerInput ─────────────────────────────────────────────────────

fn make_cable_pool(values: &[CableValue]) -> Vec<[CableValue; 2]> {
    values.iter().map(|&v| [v, v]).collect()
}

#[test]
fn trigger_no_edge_on_first_call_below_threshold() {
    let mut pool = make_cable_pool(&[CableValue::Mono(0.0)]);
    let cp = CablePool::new(&mut pool, 0);
    let mut t = TriggerInput {
        inner: MonoInput { cable_idx: 0, scale: 1.0, connected: true },
        ..Default::default()
    };
    assert!(!t.tick(&cp));
}

#[test]
fn trigger_rising_edge_on_0_to_1() {
    let mut pool = make_cable_pool(&[CableValue::Mono(0.0)]);
    let mut t = TriggerInput {
        inner: MonoInput { cable_idx: 0, scale: 1.0, connected: true },
        ..Default::default()
    };

    // First tick: low
    {
        let cp = CablePool::new(&mut pool, 0);
        assert!(!t.tick(&cp));
    }

    // Second tick: high — rising edge
    pool[0] = [CableValue::Mono(1.0); 2];
    {
        let cp = CablePool::new(&mut pool, 0);
        assert!(t.tick(&cp));
    }
}

#[test]
fn trigger_no_retrigger_when_held_high() {
    let mut pool = make_cable_pool(&[CableValue::Mono(1.0)]);
    let mut t = TriggerInput {
        inner: MonoInput { cable_idx: 0, scale: 1.0, connected: true },
        ..Default::default()
    };

    // First tick: rising from 0 → 1
    {
        let cp = CablePool::new(&mut pool, 0);
        assert!(t.tick(&cp));
    }
    // Second tick: held high — no re-trigger
    {
        let cp = CablePool::new(&mut pool, 0);
        assert!(!t.tick(&cp));
    }
}

#[test]
fn trigger_value_returns_last_read() {
    let mut pool = make_cable_pool(&[CableValue::Mono(0.75)]);
    let mut t = TriggerInput {
        inner: MonoInput { cable_idx: 0, scale: 1.0, connected: true },
        ..Default::default()
    };
    let cp = CablePool::new(&mut pool, 0);
    t.tick(&cp);
    assert_eq!(t.value(), 0.75);
}

// ── PolyTriggerInput ─────────────────────────────────────────────────

#[test]
fn poly_trigger_per_voice_edges() {
    let mut channels = [0.0f32; 16];
    channels[0] = 1.0; // voice 0 high
    channels[3] = 1.0; // voice 3 high
    let mut pool = make_cable_pool(&[CableValue::Poly(channels)]);
    let mut t = PolyTriggerInput {
        inner: PolyInput { cable_idx: 0, scale: 1.0, connected: true },
        ..Default::default()
    };

    let cp = CablePool::new(&mut pool, 0);
    let result = t.tick(&cp);
    assert!(result[0], "voice 0 should have rising edge");
    assert!(!result[1], "voice 1 should not have rising edge");
    assert!(result[3], "voice 3 should have rising edge");
}

// ── GateInput ────────────────────────────────────────────────────────

#[test]
fn gate_rising_and_falling_edges() {
    let mut pool = make_cable_pool(&[CableValue::Mono(0.0)]);
    let mut g = GateInput {
        inner: MonoInput { cable_idx: 0, scale: 1.0, connected: true },
        ..Default::default()
    };

    // Low → no edges
    {
        let cp = CablePool::new(&mut pool, 0);
        let e = g.tick(&cp);
        assert!(!e.rose);
        assert!(!e.fell);
        assert!(!e.is_high);
    }

    // Go high → rising edge
    pool[0] = [CableValue::Mono(1.0); 2];
    {
        let cp = CablePool::new(&mut pool, 0);
        let e = g.tick(&cp);
        assert!(e.rose);
        assert!(!e.fell);
        assert!(e.is_high);
    }

    // Stay high → no edges, still high
    {
        let cp = CablePool::new(&mut pool, 0);
        let e = g.tick(&cp);
        assert!(!e.rose);
        assert!(!e.fell);
        assert!(e.is_high);
    }

    // Go low → falling edge
    pool[0] = [CableValue::Mono(0.0); 2];
    {
        let cp = CablePool::new(&mut pool, 0);
        let e = g.tick(&cp);
        assert!(!e.rose);
        assert!(e.fell);
        assert!(!e.is_high);
    }
}

// ── PolyGateInput ────────────────────────────────────────────────────

#[test]
fn poly_gate_per_voice_edges() {
    let mut pool = make_cable_pool(&[CableValue::Poly([0.0; 16])]);
    let mut g = PolyGateInput {
        inner: PolyInput { cable_idx: 0, scale: 1.0, connected: true },
        ..Default::default()
    };

    // All low
    {
        let cp = CablePool::new(&mut pool, 0);
        let _ = g.tick(&cp);
    }

    // Voice 2 goes high
    let mut channels = [0.0f32; 16];
    channels[2] = 1.0;
    pool[0] = [CableValue::Poly(channels); 2];
    {
        let cp = CablePool::new(&mut pool, 0);
        let result = g.tick(&cp);
        assert!(result[2].rose);
        assert!(result[2].is_high);
        assert!(!result[0].rose);
        assert!(!result[0].is_high);
    }
}
