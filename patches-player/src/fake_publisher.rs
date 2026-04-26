//! Deterministic fake publisher for ticket 0704 — drives the TUI's meter
//! pane from a sine-walk per declared tap until ticket 0705 wires the
//! real engine→observer pipeline. Replace this module's call site with
//! the live `Subscribers` instance in 0705.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use patches_observation::processor::ProcessorId;
use patches_observation::subscribers::Subscribers;

/// Spawn a thread that walks a deterministic sine of period ~6 s through
/// each declared slot, publishing peak + RMS as linear amplitudes. The
/// thread exits when `stop` is set.
pub fn spawn(
    subs: Subscribers,
    slots: Vec<usize>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let start = Instant::now();
        while !stop.load(Ordering::Acquire) {
            let t = start.elapsed().as_secs_f32();
            for (i, &slot) in slots.iter().enumerate() {
                let phase = t * 0.6 + i as f32 * 0.7;
                let env = 0.5 + 0.5 * (phase * std::f32::consts::TAU * 0.15).sin();
                let peak = env.powf(0.7).clamp(0.0, 1.0);
                let rms = peak * 0.6;
                subs.publish_latest(slot, ProcessorId::MeterPeak, peak);
                subs.publish_latest(slot, ProcessorId::MeterRms, rms);
            }
            thread::sleep(Duration::from_millis(33));
        }
    })
}
