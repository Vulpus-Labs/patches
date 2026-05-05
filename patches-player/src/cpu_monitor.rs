//! CPU-monitor observer thread for `patches-player` (ADR 0065 / ticket 0780).
//!
//! Drains [`MonitorMessage`]s off the audio-thread SPSC, rebuilds slot →
//! (name, type) tables on `MonitorMeta`, and aggregates per-slot rolling
//! estimates of CPU cost as a fraction of block budget. The TUI reads a
//! [`CpuSnapshot`] under a short mutex hold per draw.

use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use patches_engine::{MonitorBlock, MonitorMessage};

/// Window length (in blocks) for the rolling average of per-slot estimates.
/// At 48 kHz / 128-sample blocks this is ≈1.7 s of history.
const WINDOW_BLOCKS: usize = 640;

/// One row in the CPU tab.
#[derive(Clone, Debug)]
pub struct CpuRow {
    pub name: String,
    pub type_name: String,
    pub pct: f32,
}

/// Snapshot consumed by the ratatui CPU tab. Cheap to clone (rows are
/// short) — taken under the observer's mutex once per draw.
#[derive(Clone, Default, Debug)]
pub struct CpuSnapshot {
    pub rows: Vec<CpuRow>,
    /// Periodic-phase aggregate, percent of block budget.
    pub periodic_pct: f32,
    /// Mean total measured block duration, percent of block budget. Lets
    /// the user spot unattributed time.
    pub block_pct: f32,
    /// Block size in samples (from the most recent block record).
    pub block_samples: u32,
    /// Window length used for rolling averages, in blocks.
    pub window_blocks: usize,
}

#[derive(Default)]
struct SlotAccum {
    name: String,
    type_name: String,
    /// Sum of recent per-block pct readings.
    sum_pct: f64,
    /// Count of recent readings (capped by `WINDOW_BLOCKS`).
    count: u32,
}

struct ObserverState {
    sample_rate: f32,
    slots: Vec<SlotAccum>,
    block_pct_sum: f64,
    block_pct_count: u32,
    periodic_pct_sum: f64,
    periodic_pct_count: u32,
    last_block_samples: u32,
    snapshot: Arc<Mutex<CpuSnapshot>>,
}

impl ObserverState {
    fn ingest_meta(&mut self, names: Vec<Option<std::sync::Arc<str>>>, types: Vec<&'static str>) {
        let n = names.len().max(types.len());
        let mut new_slots: Vec<SlotAccum> = (0..n).map(|_| SlotAccum::default()).collect();
        for (i, slot) in new_slots.iter_mut().enumerate().take(n) {
            slot.name = names.get(i).and_then(|o| o.as_ref()).map(|a| a.to_string()).unwrap_or_default();
            slot.type_name = types.get(i).copied().unwrap_or("").to_string();
            // Reset accumulators that don't survive a re-keying. v1 keeps
            // it simple: drop all per-slot history on plan swap.
        }
        self.slots = new_slots;
        self.block_pct_sum = 0.0;
        self.block_pct_count = 0;
        self.periodic_pct_sum = 0.0;
        self.periodic_pct_count = 0;
    }

    fn ingest_block(&mut self, b: MonitorBlock) {
        self.last_block_samples = b.block_samples;
        let block_budget_ns = b.block_samples as f64 * 1.0e9 / self.sample_rate as f64;
        if block_budget_ns <= 0.0 {
            return;
        }
        let block_pct = b.block_duration.as_nanos() as f64 / block_budget_ns * 100.0;
        self.block_pct_sum += block_pct;
        self.block_pct_count = (self.block_pct_count + 1).min(WINDOW_BLOCKS as u32);

        let periodic_pct = b.periodic_duration.as_nanos() as f64 / block_budget_ns * 100.0;
        self.periodic_pct_sum += periodic_pct;
        self.periodic_pct_count = (self.periodic_pct_count + 1).min(WINDOW_BLOCKS as u32);

        if b.module_slot != usize::MAX
            && b.module_samples_timed > 0
            && b.module_slot < self.slots.len()
        {
            let est_ns = b.module_accum.as_nanos() as f64 * b.block_samples as f64
                / b.module_samples_timed as f64;
            let pct = est_ns / block_budget_ns * 100.0;
            let slot = &mut self.slots[b.module_slot];
            slot.sum_pct += pct;
            slot.count = (slot.count + 1).min(WINDOW_BLOCKS as u32);
        }

        // Bound running sums by recomputing periodically so they don't drift
        // toward stale data. Cheap: exponential decay every WINDOW blocks.
        if self.block_pct_count >= WINDOW_BLOCKS as u32 {
            self.block_pct_sum *= 0.9;
            self.block_pct_count = (self.block_pct_count as f64 * 0.9) as u32;
        }
        if self.periodic_pct_count >= WINDOW_BLOCKS as u32 {
            self.periodic_pct_sum *= 0.9;
            self.periodic_pct_count = (self.periodic_pct_count as f64 * 0.9) as u32;
        }
        for slot in &mut self.slots {
            if slot.count >= WINDOW_BLOCKS as u32 {
                slot.sum_pct *= 0.9;
                slot.count = (slot.count as f64 * 0.9) as u32;
            }
        }
    }

    fn publish(&self) {
        let mut rows: Vec<CpuRow> = self
            .slots
            .iter()
            .filter(|s| s.count > 0 && !s.name.is_empty())
            .map(|s| CpuRow {
                name: s.name.clone(),
                type_name: s.type_name.clone(),
                pct: (s.sum_pct / s.count as f64) as f32,
            })
            .collect();
        rows.sort_by(|a, b| b.pct.partial_cmp(&a.pct).unwrap_or(std::cmp::Ordering::Equal));
        let block_pct = if self.block_pct_count > 0 {
            (self.block_pct_sum / self.block_pct_count as f64) as f32
        } else {
            0.0
        };
        let periodic_pct = if self.periodic_pct_count > 0 {
            (self.periodic_pct_sum / self.periodic_pct_count as f64) as f32
        } else {
            0.0
        };
        let snap = CpuSnapshot {
            rows,
            periodic_pct,
            block_pct,
            block_samples: self.last_block_samples,
            window_blocks: WINDOW_BLOCKS,
        };
        if let Ok(mut g) = self.snapshot.lock() {
            *g = snap;
        }
    }
}

/// Handle returned by [`spawn_cpu_monitor`].
pub struct CpuMonitor {
    pub snapshot: Arc<Mutex<CpuSnapshot>>,
    quit: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl CpuMonitor {
    pub fn stop(&mut self) {
        self.quit
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

impl Drop for CpuMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Spawn the observer thread. Returns immediately; the thread polls the SPSC
/// at a fixed cadence and republishes the snapshot.
pub fn spawn_cpu_monitor(
    mut rx: rtrb::Consumer<MonitorMessage>,
    sample_rate: f32,
) -> CpuMonitor {
    let snapshot = Arc::new(Mutex::new(CpuSnapshot::default()));
    let quit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let snapshot_clone = Arc::clone(&snapshot);
    let quit_clone = Arc::clone(&quit);
    let thread = thread::spawn(move || {
        let mut state = ObserverState {
            sample_rate,
            slots: Vec::new(),
            block_pct_sum: 0.0,
            block_pct_count: 0,
            periodic_pct_sum: 0.0,
            periodic_pct_count: 0,
            last_block_samples: 0,
            snapshot: snapshot_clone,
        };
        while !quit_clone.load(std::sync::atomic::Ordering::Acquire) {
            let mut drained = 0;
            while let Ok(msg) = rx.pop() {
                match msg {
                    MonitorMessage::MonitorMeta(meta) => {
                        let m = *meta;
                        state.ingest_meta(m.names, m.types);
                    }
                    MonitorMessage::Block(b) => state.ingest_block(b),
                }
                drained += 1;
                if drained > 1024 {
                    break;
                }
            }
            state.publish();
            thread::sleep(Duration::from_millis(50));
        }
    });
    CpuMonitor {
        snapshot,
        quit,
        thread: Some(thread),
    }
}
