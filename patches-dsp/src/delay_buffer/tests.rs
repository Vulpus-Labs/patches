use super::*;
use crate::test_support::assert_within;

// ── DelayBuffer ──────────────────────────────────────────────────────────

#[test]
fn push_and_read_nearest() {
    let mut buf = DelayBuffer::new(4);
    buf.push(1.0);
    buf.push(2.0);
    buf.push(3.0);
    assert_eq!(buf.read_nearest(0), 3.0);
    assert_eq!(buf.read_nearest(1), 2.0);
    assert_eq!(buf.read_nearest(2), 1.0);
}

#[test]
fn capacity_rounded_up() {
    assert_eq!(DelayBuffer::new(1).capacity(), 1);
    assert_eq!(DelayBuffer::new(3).capacity(), 4);
    assert_eq!(DelayBuffer::new(4).capacity(), 4);
    assert_eq!(DelayBuffer::new(5).capacity(), 8);
}

#[test]
fn for_duration_at_48k() {
    // 10 ms at 48 kHz = 480 samples → rounds up to 512.
    let buf = DelayBuffer::for_duration(0.01, 48_000.0);
    assert_eq!(buf.capacity(), 512);
}

#[test]
fn wrap_around() {
    let mut buf = DelayBuffer::new(4);
    // Push 6 samples into a 4-slot buffer; oldest two are overwritten.
    for i in 0..6u32 {
        buf.push(i as f32);
    }
    assert_eq!(buf.read_nearest(0), 5.0);
    assert_eq!(buf.read_nearest(1), 4.0);
    assert_eq!(buf.read_nearest(2), 3.0);
    assert_eq!(buf.read_nearest(3), 2.0);
}

#[test]
fn linear_at_integer_offsets() {
    let mut buf = DelayBuffer::new(8);
    buf.push(1.0);
    buf.push(2.0);
    buf.push(4.0);
    assert_eq!(buf.read_linear(0.0), 4.0);
    assert_eq!(buf.read_linear(1.0), 2.0);
    assert_eq!(buf.read_linear(2.0), 1.0);
}

#[test]
fn linear_midpoint() {
    let mut buf = DelayBuffer::new(8);
    buf.push(0.0);
    buf.push(2.0);
    let v = buf.read_linear(0.5);
    assert_within!(1.0, v, 1e-6);
}

#[test]
fn cubic_at_integer_offsets() {
    let mut buf = DelayBuffer::new(8);
    for v in [1.0_f32, 2.0, 3.0, 4.0, 5.0] {
        buf.push(v);
    }
    // Catmull-Rom must reproduce exact values at integer offsets.
    assert_eq!(buf.read_cubic(0.0), 5.0);
    assert_eq!(buf.read_cubic(1.0), 4.0);
    assert_eq!(buf.read_cubic(2.0), 3.0);
}

#[test]
fn cubic_partition_of_unity() {
    // For a constant signal the cubic interpolant must equal that constant.
    let mut buf = DelayBuffer::new(16);
    for _ in 0..16 {
        buf.push(3.0);
    }
    for tenth in 0..=10 {
        let f = tenth as f32 * 0.1;
        let v = buf.read_cubic(1.0 + f);
        assert_within!(3.0, v, 1e-5, "f={f}: expected 3.0, got {v}");
    }
}

#[test]
fn thiran_passes_dc() {
    // A constant input must pass through the all-pass at unity gain once settled.
    let mut buf = DelayBuffer::new(64);
    let mut interp = ThiranInterp::new();
    for _ in 0..64 {
        buf.push(1.0);
    }
    let mut out = 0.0_f32;
    for _ in 0..200 {
        buf.push(1.0);
        out = interp.read(&buf, 5.3);
    }
    assert_within!(1.0, out, 1e-5);
}

// ── PolyDelayBuffer ──────────────────────────────────────────────────────

#[test]
fn poly_push_and_read_nearest() {
    let mut buf = PolyDelayBuffer::new(8);
    let a: [f32; 16] = std::array::from_fn(|i| i as f32);
    let b: [f32; 16] = std::array::from_fn(|i| (i + 16) as f32);
    buf.push(a);
    buf.push(b);
    assert_eq!(buf.read_nearest(0), b);
    assert_eq!(buf.read_nearest(1), a);
}

#[test]
fn poly_linear_midpoint() {
    let mut buf = PolyDelayBuffer::new(8);
    buf.push([0.0; 16]);
    buf.push([2.0; 16]);
    let v = buf.read_linear(0.5);
    for (i, s) in v.iter().enumerate() {
        assert_within!(1.0, *s, 1e-6, "voice {i}: expected 1.0, got {s}");
    }
}

#[test]
fn poly_cubic_partition_of_unity() {
    let mut buf = PolyDelayBuffer::new(16);
    for _ in 0..16 {
        buf.push([7.0; 16]);
    }
    for tenth in 0..=10 {
        let f = tenth as f32 * 0.1;
        let v = buf.read_cubic(1.0 + f);
        for (i, s) in v.iter().enumerate() {
            assert_within!(7.0, *s, 1e-4, "voice {i} f={f}: expected 7.0, got {s}");
        }
    }
}

#[test]
fn poly_thiran_passes_dc() {
    let mut buf = PolyDelayBuffer::new(64);
    let mut interp = PolyThiranInterp::new();
    for _ in 0..64 {
        buf.push([1.0; 16]);
    }
    let mut out = [0.0_f32; 16];
    for _ in 0..200 {
        buf.push([1.0; 16]);
        out = interp.read(&buf, 3.7);
    }
    for (i, s) in out.iter().enumerate() {
        assert_within!(1.0, *s, 1e-5, "voice {i}: Thiran DC output {s}");
    }
}

// ── T7 — determinism and state reset ─────────────────────────────────────

/// T7 — determinism and state reset
/// Two fresh DelayBuffer instances fed the same push sequence must produce
/// bit-identical read outputs.
#[test]
fn delay_buffer_determinism() {
    let mut buf_a = DelayBuffer::new(64);
    let mut buf_b = DelayBuffer::new(64);
    let sequence: Vec<f32> = (0..50).map(|i| (i as f32 * 0.1).sin()).collect();
    for &s in &sequence {
        buf_a.push(s);
        buf_b.push(s);
    }
    for offset in [0_usize, 1, 5, 10, 20, 49] {
        assert_eq!(
            buf_a.read_nearest(offset),
            buf_b.read_nearest(offset),
            "read_nearest({offset}) differs"
        );
        assert_eq!(
            buf_a.read_linear(offset as f32 + 0.3),
            buf_b.read_linear(offset as f32 + 0.3),
            "read_linear({offset}+0.3) differs"
        );
    }
}

/// T7 — determinism and state reset
/// Two fresh PolyDelayBuffer instances fed the same push sequence must
/// produce bit-identical read outputs.
#[test]
fn poly_delay_buffer_determinism() {
    let mut buf_a = PolyDelayBuffer::new(64);
    let mut buf_b = PolyDelayBuffer::new(64);
    let sequence: Vec<[f32; 16]> = (0..50)
        .map(|i| std::array::from_fn(|v| ((i * 16 + v) as f32 * 0.05).sin()))
        .collect();
    for &frame in &sequence {
        buf_a.push(frame);
        buf_b.push(frame);
    }
    for offset in [0_usize, 1, 5, 10, 20, 49] {
        assert_eq!(
            buf_a.read_nearest(offset),
            buf_b.read_nearest(offset),
            "read_nearest({offset}) differs"
        );
        assert_eq!(
            buf_a.read_linear(offset as f32 + 0.3),
            buf_b.read_linear(offset as f32 + 0.3),
            "read_linear({offset}+0.3) differs"
        );
    }
}

/// T7 — determinism and state reset
/// Run ThiranInterp + DelayBuffer with a sequence, then reset ThiranInterp
/// and re-init a fresh DelayBuffer, run the same sequence again, and verify
/// bit-identical output.
#[test]
fn thiran_reset_produces_identical_output() {
    let sequence: Vec<f32> = (0..80).map(|i| (i as f32 * 0.15).sin()).collect();
    let offset = 3.7_f32;

    // First run
    let mut buf = DelayBuffer::new(64);
    let mut interp = ThiranInterp::new();
    let mut outputs_first: Vec<f32> = Vec::with_capacity(sequence.len());
    for &s in &sequence {
        buf.push(s);
        outputs_first.push(interp.read(&buf, offset));
    }

    // Reset and fresh buffer
    interp.reset();
    let mut buf2 = DelayBuffer::new(64);
    let mut outputs_second: Vec<f32> = Vec::with_capacity(sequence.len());
    for &s in &sequence {
        buf2.push(s);
        outputs_second.push(interp.read(&buf2, offset));
    }

    for (i, (a, b)) in outputs_first.iter().zip(outputs_second.iter()).enumerate() {
        assert_eq!(a.to_bits(), b.to_bits(), "ThiranInterp output differs at sample {i}");
    }
}

/// T7 — determinism and state reset
/// Run PolyThiranInterp + PolyDelayBuffer with a sequence, then reset
/// PolyThiranInterp and re-init a fresh PolyDelayBuffer, run the same
/// sequence again, and verify bit-identical output.
#[test]
fn poly_thiran_reset_produces_identical_output() {
    let sequence: Vec<[f32; 16]> = (0..80)
        .map(|i| std::array::from_fn(|v| ((i * 16 + v) as f32 * 0.05).sin()))
        .collect();
    let offset = 5.2_f32;

    // First run
    let mut buf = PolyDelayBuffer::new(64);
    let mut interp = PolyThiranInterp::new();
    let mut outputs_first: Vec<[f32; 16]> = Vec::with_capacity(sequence.len());
    for &frame in &sequence {
        buf.push(frame);
        outputs_first.push(interp.read(&buf, offset));
    }

    // Reset and fresh buffer
    interp.reset();
    let mut buf2 = PolyDelayBuffer::new(64);
    let mut outputs_second: Vec<[f32; 16]> = Vec::with_capacity(sequence.len());
    for &frame in &sequence {
        buf2.push(frame);
        outputs_second.push(interp.read(&buf2, offset));
    }

    for (i, (a, b)) in outputs_first.iter().zip(outputs_second.iter()).enumerate() {
        for v in 0..16 {
            assert_eq!(
                a[v].to_bits(),
                b[v].to_bits(),
                "PolyThiranInterp voice {v} output differs at sample {i}"
            );
        }
    }
}

// ── T4 — stability and convergence ───────────────────────────────────────

/// T4 — stability and convergence
/// Feed 10,000 sine wave samples with read offset modulated from 1.5 to
/// 500.0 using a sine-shaped pattern. Every output must be finite and
/// within [-2.0, 2.0].
#[test]
fn thiran_delay_stability_under_modulation() {
    let mut buf = DelayBuffer::new(1024);
    let mut interp = ThiranInterp::new();
    let n = 10_000_usize;
    let two_pi = std::f32::consts::TAU;

    for i in 0..n {
        let t = i as f32 / n as f32;
        // Input: unit-amplitude sine at ~110 Hz equivalent
        let input = (two_pi * 110.0 * t).sin();
        buf.push(input);

        // Offset modulated from 1.5 to 500.0 via a sine-shaped pattern
        let mod_phase = (two_pi * t).sin(); // in [-1, 1]
        let offset = 1.5 + (mod_phase + 1.0) * 0.5 * (500.0 - 1.5);

        let out = interp.read(&buf, offset);
        assert!(out.is_finite(), "output not finite at sample {i}: {out}");
        assert!(
            (-2.0..=2.0).contains(&out),
            "output out of bounds at sample {i}: {out}"
        );
    }
}
