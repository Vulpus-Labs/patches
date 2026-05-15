//! WAV writer + background recorder for Patches.
//!
//! Provides:
//! - [`write_wav`] / [`write_wav_header`] / [`commit_wav_sizes`] /
//!   [`finalize_wav`]: PCM WAV writer (complete file or streaming).
//! - [`wav_recorder`]: background-thread recorder fed by an `rtrb` ring
//!   buffer; used by `patches-player` to capture engine output.
//!
//! Audio-file *reading* (AIFF/WAV decode, resampling) used to live here
//! but moved to the bundle repos once the in-tree consumer set
//! collapsed to writer-only.

pub mod wav_recorder;
pub mod wav_write;

pub use wav_write::{commit_wav_sizes, finalize_wav, write_wav, write_wav_header};
