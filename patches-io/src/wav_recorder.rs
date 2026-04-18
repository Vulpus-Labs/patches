//! Engine-level WAV recorder.
//!
//! Records the same stereo signal written to the hardware output buffer —
//! capturing audio *after* any future oversampling/decimation rather than
//! tapping the backplane slots that upstream modules write to.
//!
//! [`open`] spawns a background writer thread and returns a [`WavRecorder`]
//! handle (kept by [`SoundEngine`]) and an `rtrb` producer (given to the
//! audio callback).  The audio callback pushes one `[f32; 2]` frame per
//! output sample; the background thread drains the ring buffer and writes
//! 16-bit stereo PCM WAV.
//!
//! # Shutdown sequence
//!
//! `SoundEngine::stop()` drops the CPAL stream first (so no more frames are
//! pushed to the ring buffer) and then drops the `WavRecorder` (whose `Drop`
//! impl sets the stop flag and joins the writer thread).  The writer drains
//! any frames that arrived before the stop signal, then finalises and flushes
//! the file.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

/// Ring-buffer capacity in stereo frames (~1.5 s at 44.1 kHz).
const RING_CAPACITY: usize = 65_536;

/// WAV writer thread handle.
///
/// Dropping this signals the writer thread to flush all remaining buffered
/// samples, finalise the WAV file, and exit.
pub struct WavRecorder {
    stop_flag: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl WavRecorder {
    fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

impl Drop for WavRecorder {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Spawn a WAV writer thread targeting `path` at `sample_rate` Hz.
///
/// Returns the recorder handle (to be held by [`SoundEngine`] until stop)
/// and the ring-buffer producer (to be placed in [`AudioCallback`]).
pub fn open(
    path: &str,
    sample_rate: u32,
) -> std::io::Result<(WavRecorder, rtrb::Producer<[f32; 2]>)> {
    let (tx, rx) = rtrb::RingBuffer::<[f32; 2]>::new(RING_CAPACITY);
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag2 = Arc::clone(&stop_flag);
    let path = path.to_owned();

    let thread = thread::Builder::new()
        .name("patches-wav-writer".to_owned())
        .spawn(move || run_writer(path, sample_rate, rx, &stop_flag2))?;

    Ok((WavRecorder { stop_flag, thread: Some(thread) }, tx))
}

// ---------------------------------------------------------------------------
// Background writer thread
// ---------------------------------------------------------------------------

fn run_writer(
    path: String,
    sample_rate: u32,
    mut rx: rtrb::Consumer<[f32; 2]>,
    stop: &AtomicBool,
) {
    let file = match File::create(&path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[patches-wav-writer] could not create {path:?}: {e}");
            return;
        }
    };
    let mut writer = BufWriter::new(file);

    if let Err(e) = crate::write_wav_header(&mut writer, 2, 16, sample_rate) {
        eprintln!("[patches-wav-writer] could not write header to {path:?}: {e}");
        return;
    }

    let mut frames_written: u32 = 0;
    let mut frames_at_last_commit: u32 = 0;

    loop {
        // Drain all available frames before checking the stop flag so we
        // never discard samples that were pushed just before shutdown.
        while let Ok([left, right]) = rx.pop() {
            let l = (left.clamp(-1.0, 1.0) * 32_767.0) as i16;
            let r = (right.clamp(-1.0, 1.0) * 32_767.0) as i16;
            let ok = writer.write_all(&l.to_le_bytes()).is_ok()
                && writer.write_all(&r.to_le_bytes()).is_ok();
            if !ok {
                eprintln!("[patches-wav-writer] write error — aborting for {path:?}");
                return;
            }
            frames_written = frames_written.saturating_add(1);
        }

        // Periodically update the WAV size fields so that an ungraceful
        // process exit leaves a playable (truncated) file.
        if frames_written.saturating_sub(frames_at_last_commit) >= sample_rate
            && crate::commit_wav_sizes(&mut writer, 2, 16, frames_written).is_ok()
        {
            frames_at_last_commit = frames_written;
        }

        if stop.load(Ordering::Acquire) {
            break;
        }

        thread::sleep(Duration::from_millis(5));
    }

    // Drain any frames that arrived between the last drain and the stop signal.
    while let Ok([left, right]) = rx.pop() {
        let l = (left.clamp(-1.0, 1.0) * 32_767.0) as i16;
        let r = (right.clamp(-1.0, 1.0) * 32_767.0) as i16;
        let _ = writer.write_all(&l.to_le_bytes());
        let _ = writer.write_all(&r.to_le_bytes());
        frames_written = frames_written.saturating_add(1);
    }

    if let Err(e) = crate::finalize_wav(&mut writer, 2, 16, frames_written) {
        eprintln!("[patches-wav-writer] could not finalise {path:?}: {e}");
    }
}

