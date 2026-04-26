//! Observer thread loop and replan transport (ADR 0056).
//!
//! Owns the consumer end of the tap ring plus a SPSC `Arc<Manifest>`
//! channel for replans. Each iteration: drain pending manifests
//! (rebuild slots), drain pending block frames (transpose to lane-major,
//! run processors, publish observations).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use patches_core::{TapBlockFrame, MAX_TAPS, TAP_BLOCK};
use patches_dsl::manifest::Manifest;

use crate::processor::{
    build_pipeline, Observation, Processor, ProcessorId, ProcessorIdentity,
};
use crate::subscribers::{Diagnostic, DiagnosticReader, Subscribers, SubscribersHandle};
use patches_io_ring::TapRingConsumer;

/// One manifest publication from the planner: the manifest itself plus
/// the tap publication rate (host rate × oversampling) the planner
/// computed when shipping the corresponding module graph. Per ADR 0054
/// §6, observer-side analyses (RMS windows, FFT bin frequencies, peak
/// decay) compute against this rate, not the host sample rate.
#[derive(Clone)]
pub struct ManifestPublication {
    pub manifest: Arc<Manifest>,
    pub sample_rate: f32,
}

/// Replan transport: control thread → observer thread.
pub struct ReplanProducer {
    tx: rtrb::Producer<ManifestPublication>,
}

impl ReplanProducer {
    /// Best-effort enqueue of a new publication. Returns `false` on full
    /// ring; callers may retry or drop. Replans don't have to be
    /// strictly ordered with audio — the latest one wins on the next
    /// observer iteration.
    pub fn submit(&mut self, pub_: ManifestPublication) -> bool {
        self.tx.push(pub_).is_ok()
    }
}

struct ReplanConsumer {
    rx: rtrb::Consumer<ManifestPublication>,
}

impl ReplanConsumer {
    /// Drain pending publications and return only the latest, if any.
    /// Older replans are superseded.
    fn drain_latest(&mut self) -> Option<ManifestPublication> {
        let mut latest: Option<ManifestPublication> = None;
        while let Ok(m) = self.rx.pop() {
            latest = Some(m);
        }
        latest
    }
}

fn replan_channel(capacity: usize) -> (ReplanProducer, ReplanConsumer) {
    let (tx, rx) = rtrb::RingBuffer::<ManifestPublication>::new(capacity);
    (ReplanProducer { tx }, ReplanConsumer { rx })
}

/// Per-slot processor list, keyed by lane index 0..MAX_TAPS.
type Slots = Vec<Vec<Box<dyn Processor>>>;

fn empty_slots() -> Slots {
    (0..MAX_TAPS).map(|_| Vec::new()).collect()
}

/// Run a manifest replan: rebuild slot processors, reusing any whose
/// identity matches a prior processor (state is preserved).
fn apply_manifest(
    old: &mut Slots,
    manifest: &Manifest,
    sample_rate: f32,
    diag_tx: &mut crate::subscribers::DiagnosticWriter,
    latest: &crate::subscribers::LatestValues,
) {
    let mut new_slots: Slots = empty_slots();

    // Index old processors by identity for reuse.
    let mut old_by_id: std::collections::HashMap<ProcessorIdentity, Box<dyn Processor>> =
        std::collections::HashMap::new();
    for slot in old.drain(..) {
        for p in slot {
            old_by_id.insert(p.identity().clone(), p);
        }
    }

    for desc in manifest.iter() {
        if desc.slot >= MAX_TAPS {
            continue;
        }
        let (fresh, unimplemented) = build_pipeline(desc, sample_rate);
        let mut reused: Vec<Box<dyn Processor>> = Vec::with_capacity(fresh.len());
        for p in fresh {
            let id = p.identity().clone();
            match old_by_id.remove(&id) {
                Some(prev) => reused.push(prev),
                None => reused.push(p),
            }
        }
        new_slots[desc.slot] = reused;
        for component in unimplemented {
            diag_tx.push(Diagnostic::NotYetImplemented {
                slot: desc.slot,
                tap_name: desc.name.clone(),
                component,
            });
        }
    }

    // Clear cells for slots that lost all processors so stale values
    // don't linger after a replan removes a tap.
    for (slot, processors) in new_slots.iter().enumerate() {
        if processors.is_empty() {
            for id in ProcessorId::ALL {
                latest.clear(slot, id);
            }
        }
    }

    *old = new_slots;
}

/// One pass over a single block frame: transpose to lane-major, run
/// each lane's processors, publish observations.
fn process_block(
    frame: &TapBlockFrame,
    slots: &mut Slots,
    latest: &crate::subscribers::LatestValues,
) {
    // Transpose sample-major → lane-major. Stack-allocated; this is
    // 32 × 64 × 4 B = 8 KiB which is fine off the audio thread.
    let mut lane: [f32; TAP_BLOCK] = [0.0; TAP_BLOCK];
    for (lane_idx, processors) in slots.iter_mut().enumerate() {
        if processors.is_empty() {
            continue;
        }
        for (s, lane_sample) in lane.iter_mut().enumerate() {
            *lane_sample = frame.samples[s][lane_idx];
        }
        for p in processors.iter_mut() {
            if let Some(Observation::Level(v)) = p.process(&lane) {
                latest.publish(lane_idx, p.id(), v);
            }
            // Spectrum/Scope variants are reserved; today's processors
            // emit only Level. Once future variants land, route them
            // here (separate latest-vector surface or distinct ring).
        }
    }
}

/// Owns the spawned observer thread; drop-or-`stop()` to shut down.
pub struct ObserverHandle {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub subscribers: SubscribersHandle,
    pub replans: Option<ReplanProducer>,
}

impl ObserverHandle {
    /// Move the replan producer out of the handle, e.g. to hand it to a
    /// host runtime. After this, [`Self::replans`] is `None`.
    pub fn take_replans(&mut self) -> Option<ReplanProducer> {
        self.replans.take()
    }

    /// Signal shutdown and join the observer thread.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ObserverHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Spawn the observer thread. Takes the consumer end of the tap ring
/// (built via [`patches_io_ring::tap_ring`]). The sample rate used by
/// per-slot pipelines arrives with each [`ManifestPublication`]
/// (planner-injected); the observer holds no slot state until the
/// first publication.
/// Returns the join handle, the public subscriber handle, the
/// diagnostics reader, and the replan producer for the control thread.
pub fn spawn_observer(
    mut rx: TapRingConsumer,
    poll_interval: Duration,
) -> (ObserverHandle, DiagnosticReader) {
    let drops_shared = rx.shared();
    let (mut subs, diag_reader) = Subscribers::new(drops_shared, 64);
    let handle = subs.handle();
    let (replan_tx, mut replan_rx) = replan_channel(4);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    let thread = thread::Builder::new()
        .name("patches-observer".into())
        .spawn(move || {
            let mut slots: Slots = empty_slots();
            while !stop_thread.load(Ordering::Relaxed) {
                if let Some(p) = replan_rx.drain_latest() {
                    apply_manifest(
                        &mut slots,
                        &p.manifest,
                        p.sample_rate,
                        &mut subs.diag_tx,
                        &subs.latest,
                    );
                }
                let mut got_any = false;
                rx.drain(|frame| {
                    got_any = true;
                    process_block(frame, &mut slots, &subs.latest);
                });
                if !got_any {
                    thread::sleep(poll_interval);
                }
            }
        })
        .expect("spawn observer thread");

    (
        ObserverHandle {
            stop,
            thread: Some(thread),
            subscribers: handle,
            replans: Some(replan_tx),
        },
        diag_reader,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{provenance::Provenance, Span};
    use patches_dsl::ast::{Scalar, Value};
    use patches_dsl::manifest::{TapDescriptor, TapType};
    use std::time::{Duration, Instant};

    use patches_io_ring::tap_ring;

    fn meter_manifest(name: &str, slot: usize, params: Vec<((String, String), Value)>) -> Manifest {
        vec![TapDescriptor {
            slot,
            name: name.to_string(),
            components: vec![TapType::Meter],
            params,
            source: Provenance::root(Span::synthetic()),
        }]
    }

    fn pubof(manifest: Manifest, sample_rate: f32) -> ManifestPublication {
        ManifestPublication {
            manifest: Arc::new(manifest),
            sample_rate,
        }
    }

    fn dc_block_frame(value: f32, lane: usize, sample_time: u64) -> TapBlockFrame {
        let mut f = TapBlockFrame::zeroed();
        f.sample_time = sample_time;
        for s in 0..TAP_BLOCK {
            f.samples[s][lane] = value;
        }
        f
    }

    fn wait_for<F: FnMut() -> bool>(timeout: Duration, mut f: F) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if f() {
                return true;
            }
            thread::sleep(Duration::from_millis(2));
        }
        false
    }

    #[test]
    fn end_to_end_meter_pipeline_publishes_peak_and_rms() {
        let (mut tx, rx) = tap_ring(8);
        let (mut handle, _diag) = spawn_observer(rx, Duration::from_millis(1));
        assert!(handle.replans.as_mut().unwrap().submit(pubof(meter_manifest("t", 0, vec![]), 48_000.0)));
        for i in 0..128u64 {
            while !tx.try_push_frame(&dc_block_frame(1.0, 0, i * TAP_BLOCK as u64)) {
                thread::yield_now();
            }
        }
        let ok = wait_for(Duration::from_secs(2), || {
            let peak = handle.subscribers.read(0, ProcessorId::MeterPeak);
            let rms = handle.subscribers.read(0, ProcessorId::MeterRms);
            (peak - 1.0).abs() < 1e-3 && (rms - 1.0).abs() < 1e-2
        });
        assert!(ok, "peak/rms never settled to ~1.0");
    }

    #[test]
    fn unbound_lane_reads_zero() {
        let (_tx, rx) = tap_ring(4);
        let (handle, _diag) = spawn_observer(rx, Duration::from_millis(1));
        // No manifest, no frames — every cell is zero.
        for slot in 0..MAX_TAPS {
            for id in ProcessorId::ALL {
                assert_eq!(handle.subscribers.read(slot, id), 0.0);
            }
        }
    }

    #[test]
    fn unimplemented_components_emit_diagnostic() {
        let (_tx, rx) = tap_ring(4);
        let (mut handle, mut diag) = spawn_observer(rx, Duration::from_millis(1));
        let m = vec![TapDescriptor {
            slot: 0,
            name: "x".into(),
            components: vec![TapType::Osc],
            params: vec![],
            source: Provenance::root(Span::synthetic()),
        }];
        assert!(handle.replans.as_mut().unwrap().submit(ManifestPublication {
            manifest: Arc::new(m),
            sample_rate: 48_000.0,
        }));
        let mut events: Vec<Diagnostic> = Vec::new();
        wait_for(Duration::from_secs(1), || {
            events.extend(diag.drain());
            !events.is_empty()
        });
        assert!(matches!(
            events.first(),
            Some(Diagnostic::NotYetImplemented { component: TapType::Osc, .. })
        ));
    }

    #[test]
    fn rms_is_sample_rate_aware_in_time_domain() {
        // Same window_ms at 44.1k vs 96k: response time should be
        // roughly the same in samples-per-second terms. We just sanity
        // check that both observers eventually settle to ~1.0 with DC.
        for sr in [44_100.0_f32, 96_000.0] {
            let (mut tx, rx) = tap_ring(16);
            let (mut handle, _diag) = spawn_observer(rx, Duration::from_millis(1));
            let m = meter_manifest("t", 0, vec![
                (("meter".into(), "window".into()), Value::Scalar(Scalar::Int(20))),
            ]);
            assert!(handle.replans.as_mut().unwrap().submit(pubof(m, sr)));
            // 20 ms window at 96k = 1920 samples = 30 blocks. Push 64 blocks.
            for i in 0..64u64 {
                while !tx.try_push_frame(&dc_block_frame(1.0, 0, i * TAP_BLOCK as u64)) {
                    thread::yield_now();
                }
            }
            let ok = wait_for(Duration::from_secs(2), || {
                (handle.subscribers.read(0, ProcessorId::MeterRms) - 1.0).abs() < 1e-2
            });
            assert!(ok, "RMS never settled at sr={sr}");
        }
    }

    #[test]
    fn replan_clears_dropped_slots() {
        let (mut tx, rx) = tap_ring(8);
        let (mut handle, _diag) = spawn_observer(rx, Duration::from_millis(1));
        assert!(handle.replans.as_mut().unwrap().submit(pubof(meter_manifest("t", 0, vec![]), 48_000.0)));
        for i in 0..16u64 {
            while !tx.try_push_frame(&dc_block_frame(1.0, 0, i * TAP_BLOCK as u64)) {
                thread::yield_now();
            }
        }
        wait_for(Duration::from_secs(1), || {
            handle.subscribers.read(0, ProcessorId::MeterPeak) > 0.5
        });
        // Replan with empty manifest — slot 0 should clear.
        assert!(handle.replans.as_mut().unwrap().submit(pubof(Vec::new(), 48_000.0)));
        let ok = wait_for(Duration::from_secs(1), || {
            handle.subscribers.read(0, ProcessorId::MeterPeak) == 0.0
                && handle.subscribers.read(0, ProcessorId::MeterRms) == 0.0
        });
        assert!(ok, "values never cleared after replan");
    }
}
