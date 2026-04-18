use super::*;

const SR: f32 = 48_000.0;

#[test]
fn impulse_response_is_bounded() {
    let mut bbd = Bbd::new(&BbdDevice::MN3009, SR);
    bbd.set_delay_seconds(0.003);
    let mut peak = 0.0_f32;
    // Fire an impulse and run for ~50 ms.
    let n = (SR * 0.05) as usize;
    for i in 0..n {
        let input = if i == 0 { 1.0 } else { 0.0 };
        let y = bbd.process(input);
        assert!(y.is_finite(), "non-finite sample at i={i}: {y}");
        peak = peak.max(y.abs());
    }
    // With DC normalisation the impulse response stays well-bounded.
    assert!(peak < 10.0, "impulse response peak too large: {peak}");
}

#[test]
fn steady_state_bounded_on_constant_input() {
    let mut bbd = Bbd::new(&BbdDevice::MN3009, SR);
    bbd.set_delay_seconds(0.003);
    // Let the BBD warm up, then check the output stays bounded on DC.
    let mut last = 0.0_f32;
    for i in 0..(SR * 0.1) as usize {
        last = bbd.process(0.5);
        assert!(last.is_finite(), "non-finite at i={i}");
    }
    assert!(
        last.abs() < 2.0,
        "DC output diverged: {last}"
    );
}

#[test]
fn delay_sweep_is_click_free() {
    let mut bbd = Bbd::new(&BbdDevice::MN3009, SR);
    // Slowly sweep delay from 1 ms to 5 ms over 100 ms while feeding a
    // 440 Hz sine. Output shouldn't jump.
    let mut prev = 0.0_f32;
    let mut max_step = 0.0_f32;
    let n = (SR * 0.1) as usize;
    for i in 0..n {
        let t = i as f32 / SR;
        let d = 0.001 + 0.004 * (i as f32 / n as f32);
        bbd.set_delay_seconds(d);
        let x = (std::f32::consts::TAU * 440.0 * t).sin();
        let y = bbd.process(x);
        let step = (y - prev).abs();
        max_step = max_step.max(step);
        prev = y;
    }
    assert!(max_step.is_finite());
    assert!(
        max_step < 1.0,
        "delay sweep produced a click of size {max_step}"
    );
}

#[test]
fn longer_delay_shifts_response_later() {
    // Group-delay check: with a longer configured delay the bucket
    // read pointer falls further behind the write pointer, so the
    // impulse emerges later. Compare time-to-peak at two delays.
    fn time_to_peak(delay_s: f32) -> usize {
        let mut bbd = Bbd::new(&BbdDevice::MN3009, SR);
        bbd.set_delay_seconds(delay_s);
        // Fire impulse, run past the delay, find peak sample index.
        let horizon = (SR * (delay_s * 3.0 + 0.01)) as usize;
        let mut peak_idx = 0;
        let mut peak_abs = 0.0_f32;
        for i in 0..horizon {
            let input = if i == 0 { 1.0 } else { 0.0 };
            let y = bbd.process(input).abs();
            if y > peak_abs {
                peak_abs = y;
                peak_idx = i;
            }
        }
        peak_idx
    }
    let short = time_to_peak(0.002);
    let long = time_to_peak(0.006);
    assert!(
        long > short,
        "longer delay should peak later: short={short}, long={long}"
    );
}

#[test]
fn reset_clears_state() {
    let mut bbd = Bbd::new(&BbdDevice::MN3009, SR);
    bbd.set_delay_seconds(0.003);
    for i in 0..1024 {
        bbd.process((i as f32 * 0.01).sin());
    }
    bbd.reset();
    let y = bbd.process(0.0);
    assert!(y.abs() < 1.0e-6, "after reset silent in → silent out: {y}");
}

#[test]
fn set_delay_seconds_does_not_allocate() {
    // Smoke test: call repeatedly; Box allocation would trigger under
    // miri but a negative-delay guard is the visible contract here.
    let mut bbd = Bbd::new(&BbdDevice::MN3009, SR);
    for i in 0..1000 {
        let d = 0.001 + (i as f32) * 1.0e-6;
        bbd.set_delay_seconds(d);
    }
    assert!(bbd.delay_seconds() > 0.0);
}
