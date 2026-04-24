//! Master-output meter tap — lock-free bridge from audio thread to GUI.
//!
//! Ticket 0673: prototype meter for the webview spike. Not the general
//! graph tap API (that is tracked in the observation UI plan). This is
//! a hardcoded single tap on plugin output — enough to exercise the
//! canvas rendering path with real signal.

use std::sync::atomic::{AtomicU32, Ordering};

/// Shared meter state. Audio thread writes, GUI thread reads.
///
/// Peak uses per-block max-abs with exponential decay between blocks
/// (ballistics applied audio-side so the GUI never sees raw peaks).
/// RMS is the block's root-mean-square.
#[derive(Debug, Default)]
pub struct MeterTap {
    peak_l: AtomicU32,
    peak_r: AtomicU32,
    rms_l: AtomicU32,
    rms_r: AtomicU32,
}

/// Per-block peak decay coefficient. Applied once per process block;
/// at 48 kHz / 64-frame blocks that is ~750 decays/sec, giving a peak
/// fall time of roughly 20 dB/sec — standard meter ballistics.
const PEAK_DECAY: f32 = 0.9985;

impl MeterTap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the previous block's peaks, decayed once. Audio thread uses
    /// these as the starting point for the new block's running max.
    pub fn decayed_peaks(&self) -> (f32, f32) {
        let pl = f32::from_bits(self.peak_l.load(Ordering::Relaxed)) * PEAK_DECAY;
        let pr = f32::from_bits(self.peak_r.load(Ordering::Relaxed)) * PEAK_DECAY;
        (pl, pr)
    }

    /// Audio-thread publish. Call once per process block with the
    /// running max-abs and sum-of-squares accumulated over the block.
    pub fn publish(&self, peak_l: f32, peak_r: f32, sq_l: f32, sq_r: f32, frames: usize) {
        if frames == 0 {
            return;
        }
        let rl = (sq_l / frames as f32).sqrt();
        let rr = (sq_r / frames as f32).sqrt();
        self.peak_l.store(peak_l.to_bits(), Ordering::Relaxed);
        self.peak_r.store(peak_r.to_bits(), Ordering::Relaxed);
        self.rms_l.store(rl.to_bits(), Ordering::Relaxed);
        self.rms_r.store(rr.to_bits(), Ordering::Relaxed);
    }

    /// GUI-thread read. Returns `(peak_l, peak_r, rms_l, rms_r)`.
    pub fn read(&self) -> (f32, f32, f32, f32) {
        (
            f32::from_bits(self.peak_l.load(Ordering::Relaxed)),
            f32::from_bits(self.peak_r.load(Ordering::Relaxed)),
            f32::from_bits(self.rms_l.load(Ordering::Relaxed)),
            f32::from_bits(self.rms_r.load(Ordering::Relaxed)),
        )
    }
}
