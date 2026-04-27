//! Subscriber surface (ADR 0056 §7).
//!
//! Two paths reach the UI:
//! - **Scalar streams** (meter peak, RMS, gate/trigger LEDs): published
//!   via per-`(slot, ProcessorId)` `AtomicU32`s — readers fetch
//!   without locking.
//! - **Vector streams** (spectrum, scope): the observer holds the
//!   processors behind a per-slot `Mutex`; readers lock the slot,
//!   call [`Processor::read_into`] (which runs the lazy analysis on
//!   the buffered input), and unlock. Heavy work (FFT, scope
//!   linearisation) thus runs at reader cadence, not block cadence.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};

use patches_core::MAX_TAPS;
use patches_dsl::manifest::TapType;

use crate::processor::{
    Oscilloscope, Processor, ProcessorId, ScopeReadOpts, Spectrum, SpectrumReadOpts,
    SPECTRUM_BIN_COUNT, SPECTRUM_FFT_SIZE_DEFAULT, spectrum_bin_count,
};
use patches_io_ring::{tap_ring, TapRingShared};

/// Latest scalar value per `(slot, ProcessorId)`. Stored as `f32::to_bits`
/// in `AtomicU32` so reads/writes are lock-free and wait-free.
pub struct LatestValues {
    cells: [[AtomicU32; ProcessorId::COUNT]; MAX_TAPS],
}

impl LatestValues {
    fn new() -> Self {
        Self {
            cells: std::array::from_fn(|_| {
                std::array::from_fn(|_| AtomicU32::new(0))
            }),
        }
    }

    /// Publish a new scalar for `(slot, id)`. Lock-free.
    pub fn publish(&self, slot: usize, id: ProcessorId, value: f32) {
        self.cells[slot][id.index()].store(value.to_bits(), Ordering::Relaxed);
    }

    /// Read the latest scalar for `(slot, id)`. Lock-free.
    pub fn read(&self, slot: usize, id: ProcessorId) -> f32 {
        f32::from_bits(self.cells[slot][id.index()].load(Ordering::Relaxed))
    }

    /// Clear the cell for `(slot, id)` back to zero. Used when the
    /// observer drops a slot on replan so stale values don't linger.
    pub fn clear(&self, slot: usize, id: ProcessorId) {
        self.cells[slot][id.index()].store(0, Ordering::Relaxed);
    }

    /// Atomic swap-to-zero. Returns the previous bits as `f32`. Used by
    /// latching one-shot streams (e.g. trigger LEDs) where the audio
    /// side stores 1.0 on fire and the consumer claims-and-clears in a
    /// single step so events are never lost between polls.
    pub fn take(&self, slot: usize, id: ProcessorId) -> f32 {
        let prev = self.cells[slot][id.index()].swap(0, Ordering::Relaxed);
        f32::from_bits(prev)
    }
}

/// Per-slot processor list, shared between the observer thread (which
/// calls [`Processor::write_block`] per audio block) and any reader
/// (which calls [`Processor::read_into`] for vector streams or reads
/// the scalar atomic surface). Per-slot granularity keeps readers from
/// contending against unrelated slots' writers.
pub type SlotProcessors = Vec<Box<dyn Processor>>;
pub type SlotMutexes = [Mutex<SlotProcessors>; MAX_TAPS];

fn empty_slot_mutexes() -> SlotMutexes {
    std::array::from_fn(|_| Mutex::new(Vec::new()))
}

/// One observer-side diagnostic event surfaced through the same channel
/// as drop counters (ticket 0701 §Notes).
#[derive(Debug, Clone, PartialEq)]
pub enum Diagnostic {
    /// Observer received a manifest entry whose component type has no
    /// real pipeline yet. One-shot per `(slot, component)` per
    /// manifest. Subscribers should surface as a UI warning, not a
    /// per-block stream.
    NotYetImplemented {
        slot: usize,
        tap_name: String,
        component: TapType,
    },
    /// Manifest descriptor referenced a slot >= MAX_TAPS. The observer
    /// drops the descriptor; the tap is unobservable until corrected.
    /// Surfaces planner bugs that would otherwise be invisible (ticket
    /// 0707).
    InvalidSlot {
        slot: usize,
        tap_name: String,
    },
}

impl Diagnostic {
    /// Render to a single human-readable line for the status log.
    pub fn render(&self) -> String {
        match self {
            Diagnostic::NotYetImplemented { tap_name, component, .. } => {
                let comp = match component {
                    TapType::Meter => "meter",
                    TapType::Osc => "osc",
                    TapType::Spectrum => "spectrum",
                    TapType::GateLed => "gate_led",
                    TapType::TriggerLed => "trigger_led",
                };
                format!("tap `{tap_name}` (`{comp}`): not yet implemented")
            }
            Diagnostic::InvalidSlot { slot, tap_name } => {
                format!("tap `{tap_name}`: invalid slot {slot}")
            }
        }
    }
}

/// UI-side reader for diagnostic events. Single consumer.
pub struct DiagnosticReader {
    pub(crate) rx: rtrb::Consumer<Diagnostic>,
}

impl DiagnosticReader {
    /// Drain all pending diagnostics. Empty list when the queue is empty.
    pub fn drain(&mut self) -> Vec<Diagnostic> {
        let mut out = Vec::new();
        while let Ok(d) = self.rx.pop() {
            out.push(d);
        }
        out
    }
}

/// Observer-side diagnostic writer.
pub struct DiagnosticWriter {
    pub(crate) tx: rtrb::Producer<Diagnostic>,
}

impl DiagnosticWriter {
    /// Best-effort push; drops on full ring (ticket 0701 keeps the
    /// surface narrow — diagnostics are advisory and a backed-up UI
    /// shouldn't stall the observer).
    pub fn push(&mut self, d: Diagnostic) {
        let _ = self.tx.push(d);
    }
}

/// Public, clonable handle for UI subscribers. Holds `Arc`s into the
/// observer's shared atomic surface, the per-slot processor mutexes,
/// and the drop-counter handle.
#[derive(Clone)]
pub struct SubscribersHandle {
    latest: Arc<LatestValues>,
    slots: Arc<SlotMutexes>,
    drops: Arc<TapRingShared>,
}

impl SubscribersHandle {
    /// Latest peak / RMS scalar for `(slot, id)`. Returns 0.0 for
    /// unbound slots or unsupported component pipelines (zero-init).
    pub fn read(&self, slot: usize, id: ProcessorId) -> f32 {
        self.latest.read(slot, id)
    }

    /// Atomic claim-and-clear of a latching scalar (e.g. trigger LED).
    /// Returns `true` if the cell held a non-zero value, simultaneously
    /// resetting it to zero. Subsequent calls return `false` until the
    /// audio side latches another fire.
    pub fn take_trigger(&self, slot: usize) -> bool {
        if slot >= MAX_TAPS {
            return false;
        }
        self.latest.take(slot, ProcessorId::TriggerLed) > 0.0
    }

    /// Per-slot block-frame drop count forwarded from the tap ring.
    pub fn dropped(&self, slot: usize) -> u64 {
        self.drops.dropped(slot)
    }

    /// Run the spectrum analysis on the latest buffered input for
    /// `slot` and copy magnitudes into `dst`. Returns `true` if a
    /// result is available; `false` if no spectrum processor is wired
    /// at this slot or it has not yet seen enough input for the
    /// requested FFT size. On `false`, `dst` is resized to the bin
    /// count for the requested (resolved) FFT size, zero-filled.
    pub fn read_spectrum_into_with(
        &self,
        slot: usize,
        opts: SpectrumReadOpts,
        dst: &mut Vec<f32>,
    ) -> bool {
        let bins = spectrum_bin_count(opts.resolve_fft_size());
        let fail = |dst: &mut Vec<f32>| {
            dst.clear();
            dst.resize(bins, 0.0);
        };
        if slot >= MAX_TAPS {
            fail(dst);
            return false;
        }
        let mut g = match self.slots[slot].lock() {
            Ok(g) => g,
            Err(_) => {
                fail(dst);
                return false;
            }
        };
        for p in g.iter_mut() {
            if p.id() == ProcessorId::Spectrum {
                if let Some(s) = p.as_any_mut().downcast_mut::<Spectrum>() {
                    if s.read_with(opts, dst) {
                        return true;
                    }
                }
                fail(dst);
                return false;
            }
        }
        fail(dst);
        false
    }

    /// Default-options spectrum read. Returns
    /// [`SPECTRUM_FFT_SIZE_DEFAULT`]/2+1 bins.
    pub fn read_spectrum_into(&self, slot: usize, dst: &mut Vec<f32>) -> bool {
        self.read_spectrum_into_with(
            slot,
            SpectrumReadOpts { fft_size: SPECTRUM_FFT_SIZE_DEFAULT },
            dst,
        )
    }

    /// Pull the latest decimated oscilloscope window for `slot` into
    /// `dst`. Returns `true` if enough raw samples have been buffered
    /// for the requested window; `false` otherwise. On `false`, `dst`
    /// is cleared.
    pub fn read_scope_into_with(
        &self,
        slot: usize,
        opts: ScopeReadOpts,
        dst: &mut Vec<f32>,
    ) -> bool {
        if slot >= MAX_TAPS {
            dst.clear();
            return false;
        }
        let mut g = match self.slots[slot].lock() {
            Ok(g) => g,
            Err(_) => {
                dst.clear();
                return false;
            }
        };
        for p in g.iter_mut() {
            if p.id() == ProcessorId::Scope {
                if let Some(o) = p.as_any_mut().downcast_mut::<Oscilloscope>() {
                    if o.read_with(opts, dst) {
                        return true;
                    }
                }
                dst.clear();
                return false;
            }
        }
        dst.clear();
        false
    }

    /// Default-options scope read.
    pub fn read_scope_into(&self, slot: usize, dst: &mut Vec<f32>) -> bool {
        self.read_scope_into_with(slot, ScopeReadOpts::default(), dst)
    }
}

// Silence unused-import warnings in the absence of typed default bin
// counts elsewhere; bins are computed from opts.
#[allow(dead_code)]
fn _ensure_bin_count_used() -> usize {
    SPECTRUM_BIN_COUNT
}

/// Owned bundle of subscriber state held by the observer thread.
/// `handle()` returns the public reader; the observer module mutates
/// `slots` directly under each per-slot mutex.
pub struct Subscribers {
    pub(crate) latest: Arc<LatestValues>,
    pub(crate) slots: Arc<SlotMutexes>,
    pub(crate) drops: Arc<TapRingShared>,
    pub(crate) diag_tx: DiagnosticWriter,
}

impl Subscribers {
    /// Build a subscriber bundle. `diag_capacity` sizes the diagnostics
    /// ring; small (16) is fine — diagnostics are one-shot per replan.
    pub fn new(
        drops: Arc<TapRingShared>,
        diag_capacity: usize,
    ) -> (Self, DiagnosticReader) {
        let (tx, rx) = rtrb::RingBuffer::<Diagnostic>::new(diag_capacity);
        (
            Self {
                latest: Arc::new(LatestValues::new()),
                slots: Arc::new(empty_slot_mutexes()),
                drops,
                diag_tx: DiagnosticWriter { tx },
            },
            DiagnosticReader { rx },
        )
    }

    /// Public clonable reader handle for UI subscribers.
    pub fn handle(&self) -> SubscribersHandle {
        SubscribersHandle {
            latest: Arc::clone(&self.latest),
            slots: Arc::clone(&self.slots),
            drops: Arc::clone(&self.drops),
        }
    }

    /// Writer-side publish for bringup harnesses (e.g. patches-player TUI
    /// fake-publisher in ticket 0704). The real observer pipeline writes
    /// the same surface internally; this method is the one external way in.
    pub fn publish_latest(&self, slot: usize, id: ProcessorId, value: f32) {
        self.latest.publish(slot, id, value);
    }

    /// Writer-side diagnostic push for bringup harnesses.
    pub fn push_diagnostic(&mut self, d: Diagnostic) {
        self.diag_tx.push(d);
    }

    /// Bringup helper: build a self-contained `Subscribers` + reader handle
    /// + diagnostic reader without a real audio-side ring.
    pub fn for_bringup(diag_capacity: usize) -> (Self, SubscribersHandle, DiagnosticReader) {
        let (producer, _consumer) = tap_ring(16);
        let drops = producer.shared();
        let (subs, diag_rx) = Self::new(drops, diag_capacity);
        let handle = subs.handle();
        (subs, handle, diag_rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::{
        Oscilloscope, ProcessorIdentity, Spectrum, SCOPE_DECIMATION_DEFAULT, SCOPE_WINDOW_DEFAULT,
        SPECTRUM_FFT_SIZE_DEFAULT,
    };
    use patches_core::TAP_BLOCK as CORE_TAP_BLOCK;

    fn fresh() -> Subscribers {
        let (producer, _consumer) = tap_ring(4);
        let (subs, _diag) = Subscribers::new(producer.shared(), 4);
        subs
    }

    fn install_spectrum(subs: &Subscribers, slot: usize) {
        let id = ProcessorIdentity::new("t", ProcessorId::Spectrum);
        let p: Box<dyn Processor> = Box::new(Spectrum::new(id));
        subs.slots[slot].lock().unwrap().push(p);
    }

    fn install_scope(subs: &Subscribers, slot: usize) {
        let id = ProcessorIdentity::new("t", ProcessorId::Scope);
        let p: Box<dyn Processor> = Box::new(Oscilloscope::new(id));
        subs.slots[slot].lock().unwrap().push(p);
    }

    fn drive_block(subs: &Subscribers, slot: usize, value: f32, t: u64) {
        let block = [value; CORE_TAP_BLOCK];
        for p in subs.slots[slot].lock().unwrap().iter_mut() {
            p.write_block(&block, t);
        }
    }

    fn default_spectrum_bins() -> usize {
        spectrum_bin_count(SPECTRUM_FFT_SIZE_DEFAULT)
    }

    #[test]
    fn spectrum_read_after_full_window() {
        let subs = fresh();
        install_spectrum(&subs, 7);
        let blocks = SPECTRUM_FFT_SIZE_DEFAULT.div_ceil(CORE_TAP_BLOCK);
        for b in 0..blocks {
            drive_block(&subs, 7, 1.0, (b * CORE_TAP_BLOCK) as u64);
        }
        let mut out = Vec::new();
        let ok = subs.handle().read_spectrum_into(7, &mut out);
        assert!(ok);
        assert_eq!(out.len(), default_spectrum_bins());
    }

    #[test]
    fn spectrum_read_before_publish_returns_zeros_and_false() {
        let subs = fresh();
        install_spectrum(&subs, 3);
        let mut out = Vec::new();
        let ok = subs.handle().read_spectrum_into(3, &mut out);
        assert!(!ok);
        assert_eq!(out.len(), default_spectrum_bins());
        assert!(out.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn spectrum_read_with_no_processor_is_false() {
        let subs = fresh();
        let mut out = Vec::new();
        let ok = subs.handle().read_spectrum_into(0, &mut out);
        assert!(!ok);
        assert_eq!(out.len(), default_spectrum_bins());
    }

    #[test]
    fn spectrum_read_with_4096_size() {
        let subs = fresh();
        install_spectrum(&subs, 1);
        let blocks = 4096usize.div_ceil(CORE_TAP_BLOCK);
        for b in 0..blocks {
            drive_block(&subs, 1, 1.0, (b * CORE_TAP_BLOCK) as u64);
        }
        let mut out = Vec::new();
        let ok = subs
            .handle()
            .read_spectrum_into_with(1, SpectrumReadOpts { fft_size: 4096 }, &mut out);
        assert!(ok);
        assert_eq!(out.len(), spectrum_bin_count(4096));
    }

    #[test]
    fn scope_read_after_writes() {
        let subs = fresh();
        install_scope(&subs, 2);
        let span = SCOPE_DECIMATION_DEFAULT * SCOPE_WINDOW_DEFAULT;
        for b in 0..span.div_ceil(CORE_TAP_BLOCK) {
            drive_block(&subs, 2, 0.25, (b * CORE_TAP_BLOCK) as u64);
        }
        let mut out = Vec::new();
        let ok = subs.handle().read_scope_into(2, &mut out);
        assert!(ok);
        assert_eq!(out.len(), SCOPE_WINDOW_DEFAULT);
    }

    #[test]
    fn scope_read_before_writes_returns_empty_and_false() {
        let subs = fresh();
        install_scope(&subs, 0);
        let mut out = vec![1.0; 16];
        assert!(!subs.handle().read_scope_into(0, &mut out));
        assert!(out.is_empty());
    }

    #[test]
    fn scope_read_with_custom_decimation_and_window() {
        let subs = fresh();
        install_scope(&subs, 4);
        let span: usize = 4 * 1024;
        for b in 0..span.div_ceil(CORE_TAP_BLOCK) {
            drive_block(&subs, 4, 0.5, (b * CORE_TAP_BLOCK) as u64);
        }
        let mut out = Vec::new();
        let opts = ScopeReadOpts { decimation: 4, window_samples: 1024 };
        let ok = subs.handle().read_scope_into_with(4, opts, &mut out);
        assert!(ok);
        assert_eq!(out.len(), 1024);
    }
}
