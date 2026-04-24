//! Module-panic halt state (ADR 0051).
//!
//! A single [`HaltState`] is shared between the audio thread (which writes
//! the breadcrumb slot index and, on panic, the halt details) and any
//! control-thread holder of a [`HaltHandle`] (which polls [`snapshot`] for
//! UI / diagnostic output).
//!
//! The breadcrumb (`current_module_slot`) is updated with `Relaxed` ordering
//! before and after every `Module::process` / `PeriodicUpdate::periodic_update`
//! call. On a caught unwind the audio thread reads it back to identify the
//! offending module, writes a [`HaltInfoSnapshot`] into the mutex, and sets
//! `halted` with `Release`. Readers check `halted` with `Acquire` and then
//! clone the mutex payload.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Sentinel meaning "no module currently ticking".
pub const NO_SLOT: usize = usize::MAX;

/// Owned snapshot of the halt state, safe to clone and pass to UI threads.
#[derive(Debug, Clone)]
pub struct HaltInfoSnapshot {
    pub slot: usize,
    pub module_name: String,
    pub payload: String,
}

/// Shared halt state. Cheap `Arc`-clonable handle for control-thread polls.
pub struct HaltState {
    pub(crate) halted: AtomicBool,
    pub(crate) current_module_slot: AtomicUsize,
    pub(crate) info: Mutex<Option<HaltInfoSnapshot>>,
}

impl HaltState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            halted: AtomicBool::new(false),
            current_module_slot: AtomicUsize::new(NO_SLOT),
            info: Mutex::new(None),
        })
    }

    #[inline]
    pub(crate) fn mark_slot(&self, slot: usize) {
        self.current_module_slot.store(slot, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) fn clear_slot(&self) {
        self.current_module_slot.store(NO_SLOT, Ordering::Relaxed);
    }

    #[inline]
    pub fn is_halted(&self) -> bool {
        self.halted.load(Ordering::Acquire)
    }

    /// Reset halt state. Called on plan adoption (rebuild).
    pub fn clear(&self) {
        self.halted.store(false, Ordering::Release);
        self.current_module_slot.store(NO_SLOT, Ordering::Relaxed);
        // Poisoning is tolerable here — if a previous reader panicked while
        // holding the lock, the halt info is stale regardless.
        match self.info.lock() {
            Ok(mut g) => *g = None,
            Err(poison) => *poison.into_inner() = None,
        }
    }

    /// Record a halt. Called from the audio thread inside a `catch_unwind`
    /// Err branch. May allocate (already off the happy-path RT hot loop).
    pub(crate) fn record(&self, slot: usize, module_name: &'static str, payload: String) {
        let snapshot = HaltInfoSnapshot {
            slot,
            module_name: module_name.to_string(),
            payload,
        };
        match self.info.lock() {
            Ok(mut g) => *g = Some(snapshot),
            Err(poison) => *poison.into_inner() = Some(snapshot),
        }
        self.halted.store(true, Ordering::Release);
    }

    /// Non-blocking control-thread read. Returns `None` when not halted.
    pub fn snapshot(&self) -> Option<HaltInfoSnapshot> {
        if !self.is_halted() {
            return None;
        }
        match self.info.lock() {
            Ok(g) => g.clone(),
            Err(poison) => poison.into_inner().clone(),
        }
    }
}

/// Cheap clonable handle exposed to control threads.
#[derive(Clone)]
pub struct HaltHandle(Arc<HaltState>);

impl HaltHandle {
    pub(crate) fn from_arc(state: Arc<HaltState>) -> Self {
        Self(state)
    }

    /// Return the current halt info, or `None` if the engine is not halted.
    pub fn halt_info(&self) -> Option<HaltInfoSnapshot> {
        self.0.snapshot()
    }

    pub fn is_halted(&self) -> bool {
        self.0.is_halted()
    }
}

/// Truncate a panic payload to `max` bytes (char-boundary safe).
pub(crate) fn truncate_payload(s: String, max: usize) -> String {
    if s.len() <= max {
        return s;
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = s;
    out.truncate(cut);
    out.push('…');
    out
}

/// Extract a short string summary from a `catch_unwind` payload.
pub(crate) fn payload_summary(payload: Box<dyn std::any::Any + Send>) -> String {
    let raw = if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    };
    truncate_payload(raw, 256)
}
