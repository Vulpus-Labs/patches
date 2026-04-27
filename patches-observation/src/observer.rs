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
    build_pipeline, Processor, ProcessorId, ProcessorIdentity,
};
use crate::subscribers::{
    Diagnostic, DiagnosticReader, SlotMutexes, Subscribers, SubscribersHandle,
};
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
    /// Generation number paired with this manifest (ticket 0707). The
    /// host runtime increments on every plan push; the same value is
    /// stamped on the corresponding `ExecutionPlan` and on every
    /// `TapBlockFrame` emitted while that plan is in force. The
    /// observer drops frames whose generation does not match the
    /// current value, preventing stale-slot misinterpretation across
    /// replans. `0` means "no manifest yet".
    pub generation: u32,
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

/// Run a manifest replan: rebuild slot processors, reusing any whose
/// identity matches a prior processor (state is preserved). Operates
/// directly on the shared per-slot mutex array.
fn apply_manifest(
    slots: &SlotMutexes,
    manifest: &Manifest,
    sample_rate: f32,
    diag_tx: &mut crate::subscribers::DiagnosticWriter,
    latest: &crate::subscribers::LatestValues,
) {
    // Drain old processors out of every slot so we can rebuild fresh.
    let mut old_by_id: std::collections::HashMap<ProcessorIdentity, Box<dyn Processor>> =
        std::collections::HashMap::new();
    for slot_mutex in slots.iter() {
        let mut g = match slot_mutex.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        for p in g.drain(..) {
            old_by_id.insert(p.identity().clone(), p);
        }
    }

    let mut new_slots: Vec<Vec<Box<dyn Processor>>> =
        (0..MAX_TAPS).map(|_| Vec::new()).collect();

    for desc in manifest.iter() {
        if desc.slot >= MAX_TAPS {
            diag_tx.push(Diagnostic::InvalidSlot {
                slot: desc.slot,
                tap_name: desc.name.clone(),
            });
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

    for (slot, processors) in new_slots.into_iter().enumerate() {
        if processors.is_empty() {
            for id in ProcessorId::ALL {
                latest.clear(slot, id);
            }
        }
        let mut g = match slots[slot].lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        *g = processors;
    }
}

/// One pass over a single block frame: transpose to lane-major, run
/// each lane's processors, publish observations.
///
/// Frames whose `manifest_generation` does not match the observer's
/// `current_generation` are silently dropped (ticket 0707). Generation
/// mismatch is distinct from ring-overflow drops; the per-slot drop
/// counters are *not* bumped here. Generation `0` matches anything —
/// it represents the "no manifest yet" pre-publication regime, where
/// frames are zero-init and the observer has no slot wiring anyway.
fn process_block(
    frame: &TapBlockFrame,
    slots: &SlotMutexes,
    latest: &crate::subscribers::LatestValues,
    current_generation: u32,
) {
    if current_generation != 0
        && frame.manifest_generation != 0
        && frame.manifest_generation != current_generation
    {
        return;
    }
    let block_sample_time = frame.sample_time;
    let mut lane: [f32; TAP_BLOCK] = [0.0; TAP_BLOCK];
    for (lane_idx, slot_mutex) in slots.iter().enumerate() {
        let mut g = match slot_mutex.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if g.is_empty() {
            continue;
        }
        for (s, lane_sample) in lane.iter_mut().enumerate() {
            *lane_sample = frame.samples[s][lane_idx];
        }
        for p in g.iter_mut() {
            p.write_block(&lane, block_sample_time);
            // Publish scalar streams to the atomic surface so readers
            // can fetch lock-free; vector streams stay buffered for
            // lazy reader-side analysis.
            if let Some(v) = p.scalar() {
                latest.publish(lane_idx, p.id(), v);
            }
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
            let slots = Arc::clone(&subs.slots);
            let mut current_generation: u32 = 0;
            while !stop_thread.load(Ordering::Relaxed) {
                if let Some(p) = replan_rx.drain_latest() {
                    current_generation = p.generation;
                    apply_manifest(
                        &slots,
                        &p.manifest,
                        p.sample_rate,
                        &mut subs.diag_tx,
                        &subs.latest,
                    );
                }
                let mut got_any = false;
                rx.drain(|frame| {
                    got_any = true;
                    process_block(frame, &slots, &subs.latest, current_generation);
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
    use patches_dsl::manifest::{TapDescriptor, TapType};
    use std::time::{Duration, Instant};

    use patches_io_ring::tap_ring;

    fn meter_manifest(name: &str, slot: usize) -> Manifest {
        vec![TapDescriptor {
            slot,
            name: name.to_string(),
            components: vec![TapType::Meter],
            source: Provenance::root(Span::synthetic()),
        }]
    }

    fn pubof(manifest: Manifest, sample_rate: f32) -> ManifestPublication {
        ManifestPublication {
            manifest: Arc::new(manifest),
            sample_rate,
            generation: 1,
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
        assert!(handle.replans.as_mut().unwrap().submit(pubof(meter_manifest("t", 0), 48_000.0)));
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
    fn gate_led_pipeline_publishes_scalar() {
        let (mut tx, rx) = tap_ring(8);
        let (mut handle, _diag) = spawn_observer(rx, Duration::from_millis(1));
        let m = vec![TapDescriptor {
            slot: 0,
            name: "x".into(),
            components: vec![TapType::GateLed],
            source: Provenance::root(Span::synthetic()),
        }];
        assert!(handle.replans.as_mut().unwrap().submit(ManifestPublication {
            manifest: Arc::new(m),
            sample_rate: 48_000.0,
            generation: 1,
        }));
        for i in 0..32u64 {
            while !tx.try_push_frame(&dc_block_frame(1.0, 0, i * TAP_BLOCK as u64)) {
                thread::yield_now();
            }
        }
        let ok = wait_for(Duration::from_secs(2), || {
            (handle.subscribers.read(0, ProcessorId::GateLed) - 1.0).abs() < 1e-3
        });
        assert!(ok, "gate LED never reached 1.0");
    }

    #[test]
    fn rms_is_sample_rate_aware_in_time_domain() {
        // Default 50 ms RMS window scales with sample rate: 50 ms at
        // 44.1k = 2205 samples (~35 blocks) vs 50 ms at 96k = 4800
        // samples (~75 blocks). Push enough to fill the larger one.
        for sr in [44_100.0_f32, 96_000.0] {
            let (mut tx, rx) = tap_ring(128);
            let (mut handle, _diag) = spawn_observer(rx, Duration::from_millis(1));
            let m = meter_manifest("t", 0);
            assert!(handle.replans.as_mut().unwrap().submit(pubof(m, sr)));
            for i in 0..128u64 {
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
    fn invalid_slot_emits_diagnostic() {
        let (_tx, rx) = tap_ring(4);
        let (mut handle, mut diag) = spawn_observer(rx, Duration::from_millis(1));
        let m = vec![TapDescriptor {
            slot: MAX_TAPS, // out of range
            name: "rogue".into(),
            components: vec![TapType::Meter],
            source: Provenance::root(Span::synthetic()),
        }];
        assert!(handle.replans.as_mut().unwrap().submit(pubof(m, 48_000.0)));
        let mut events: Vec<Diagnostic> = Vec::new();
        wait_for(Duration::from_secs(1), || {
            events.extend(diag.drain());
            !events.is_empty()
        });
        assert!(matches!(
            events.first(),
            Some(Diagnostic::InvalidSlot { slot, tap_name }) if *slot == MAX_TAPS && tap_name == "rogue"
        ));
    }

    #[test]
    fn stale_generation_frames_are_dropped() {
        // Observer at gen=2; frame stamped gen=1 must be ignored. We
        // verify by sending a stale-gen frame that *would* drive the
        // peak to 1.0 if accepted; the peak should remain 0.0.
        let (mut tx, rx) = tap_ring(64);
        let (mut handle, _diag) = spawn_observer(rx, Duration::from_millis(1));
        let m = meter_manifest("t", 0);
        assert!(handle.replans.as_mut().unwrap().submit(ManifestPublication {
            manifest: Arc::new(m),
            sample_rate: 48_000.0,
            generation: 2,
        }));
        // Stamp frame with generation 1 (stale). Use a small fixed
        // count and a generously-sized ring so producer-side overflow
        // drops cannot contaminate the assertion.
        let mut stale = dc_block_frame(1.0, 0, 0);
        stale.manifest_generation = 1;
        for _ in 0..8 {
            while !tx.try_push_frame(&stale) {
                thread::yield_now();
            }
        }
        // Give the observer time to drain.
        thread::sleep(Duration::from_millis(100));
        assert_eq!(handle.subscribers.read(0, ProcessorId::MeterPeak), 0.0);
        // Drops counter should NOT have advanced for stale-gen drops.
        assert_eq!(handle.subscribers.dropped(0), 0);
    }

    #[test]
    fn matching_generation_frames_are_processed() {
        let (mut tx, rx) = tap_ring(8);
        let (mut handle, _diag) = spawn_observer(rx, Duration::from_millis(1));
        assert!(handle.replans.as_mut().unwrap().submit(ManifestPublication {
            manifest: Arc::new(meter_manifest("t", 0)),
            sample_rate: 48_000.0,
            generation: 7,
        }));
        for i in 0..128u64 {
            let mut f = dc_block_frame(1.0, 0, i * TAP_BLOCK as u64);
            f.manifest_generation = 7;
            while !tx.try_push_frame(&f) {
                thread::yield_now();
            }
        }
        let ok = wait_for(Duration::from_secs(2), || {
            (handle.subscribers.read(0, ProcessorId::MeterPeak) - 1.0).abs() < 1e-3
        });
        assert!(ok, "matching-gen frames never settled to 1.0");
    }

    #[test]
    fn replan_clears_dropped_slots() {
        let (mut tx, rx) = tap_ring(8);
        let (mut handle, _diag) = spawn_observer(rx, Duration::from_millis(1));
        assert!(handle.replans.as_mut().unwrap().submit(pubof(meter_manifest("t", 0), 48_000.0)));
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
