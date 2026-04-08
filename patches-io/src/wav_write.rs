//! WAV file writer.
//!
//! Provides both a complete-file convenience function ([`write_wav`]) and
//! low-level streaming helpers ([`write_wav_header`], [`commit_wav_sizes`],
//! [`finalize_wav`]) for use by the engine's ring-buffer recorder.

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use crate::audio_data::AudioIoError;

// ---------------------------------------------------------------------------
// Complete-file writer
// ---------------------------------------------------------------------------

/// Write a complete 16-bit PCM WAV file from per-channel sample data.
///
/// `channels` is a slice of channel buffers (all must be the same length).
/// Samples are clamped to [-1, 1] before quantisation.
pub fn write_wav(path: &Path, channels: &[&[f32]], sample_rate: u32) -> Result<(), AudioIoError> {
    if channels.is_empty() {
        return Err(AudioIoError::NoSamples);
    }
    let num_frames = channels[0].len();
    let n_ch = channels.len() as u16;

    let file = std::fs::File::create(path)?;
    let mut w = std::io::BufWriter::new(file);

    write_wav_header(&mut w, n_ch, 16, sample_rate)?;

    for frame in 0..num_frames {
        for ch in channels {
            let s = (ch[frame].clamp(-1.0, 1.0) * 32_767.0) as i16;
            w.write_all(&s.to_le_bytes())?;
        }
    }

    finalize_wav(&mut w, n_ch, 16, num_frames as u32)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Streaming helpers
// ---------------------------------------------------------------------------

/// Write a PCM WAV header with zeroed size fields.
///
/// Size fields are filled later by [`commit_wav_sizes`] or [`finalize_wav`].
pub fn write_wav_header(
    w: &mut impl Write,
    num_channels: u16,
    bits_per_sample: u16,
    sample_rate: u32,
) -> std::io::Result<()> {
    let block_align = num_channels * bits_per_sample / 8;
    let byte_rate = sample_rate * u32::from(block_align);

    w.write_all(b"RIFF")?;
    w.write_all(&0u32.to_le_bytes())?; // filled by commit/finalize
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&num_channels.to_le_bytes())?;
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&block_align.to_le_bytes())?;
    w.write_all(&bits_per_sample.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&0u32.to_le_bytes())?; // filled by commit/finalize
    Ok(())
}

/// Seek back to the RIFF and data size fields, overwrite them with the
/// correct values, then seek to the end so subsequent writes land correctly.
pub fn commit_wav_sizes<W: Write + Seek>(
    w: &mut W,
    num_channels: u16,
    bits_per_sample: u16,
    frames: u32,
) -> std::io::Result<()> {
    let bytes_per_frame = u32::from(num_channels) * u32::from(bits_per_sample) / 8;
    let data_bytes = frames.saturating_mul(bytes_per_frame);
    let riff_size = data_bytes.saturating_add(36);

    w.seek(SeekFrom::Start(4))?;
    w.write_all(&riff_size.to_le_bytes())?;
    w.seek(SeekFrom::Start(40))?;
    w.write_all(&data_bytes.to_le_bytes())?;
    w.seek(SeekFrom::End(0))?;
    Ok(())
}

/// Commit sizes and flush. Call once when recording is complete.
pub fn finalize_wav<W: Write + Seek>(
    w: &mut W,
    num_channels: u16,
    bits_per_sample: u16,
    frames: u32,
) -> std::io::Result<()> {
    commit_wav_sizes(w, num_channels, bits_per_sample, frames)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn header_round_trip() {
        let mut buf = Cursor::new(Vec::new());
        write_wav_header(&mut buf, 2, 16, 44100).unwrap();

        let data = buf.into_inner();
        assert_eq!(&data[0..4], b"RIFF");
        assert_eq!(&data[8..12], b"WAVE");
        assert_eq!(&data[12..16], b"fmt ");

        // PCM format tag
        assert_eq!(u16::from_le_bytes([data[20], data[21]]), 1);
        // Channels
        assert_eq!(u16::from_le_bytes([data[22], data[23]]), 2);
        // Sample rate
        assert_eq!(
            u32::from_le_bytes([data[24], data[25], data[26], data[27]]),
            44100
        );
        // Bits per sample
        assert_eq!(u16::from_le_bytes([data[34], data[35]]), 16);
    }

    #[test]
    fn commit_sizes_correct() {
        let mut buf = Cursor::new(Vec::new());
        write_wav_header(&mut buf, 2, 16, 44100).unwrap();

        // Write 100 frames of stereo 16-bit = 400 bytes of sample data.
        let sample_data = vec![0u8; 400];
        buf.write_all(&sample_data).unwrap();

        commit_wav_sizes(&mut buf, 2, 16, 100).unwrap();

        let data = buf.into_inner();
        let data_bytes = 100u32 * 4; // 2ch * 2bytes
        let riff_size = data_bytes + 36;
        assert_eq!(
            u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
            riff_size
        );
        assert_eq!(
            u32::from_le_bytes([data[40], data[41], data[42], data[43]]),
            data_bytes
        );
    }

    #[test]
    fn write_and_read_round_trip() {
        let left = vec![0.5f32, -0.5, 0.25];
        let right = vec![-0.25f32, 0.75, 0.0];

        // Build the WAV in memory to avoid temp-dir portability issues.
        let channels: &[&[f32]] = &[&left, &right];
        let num_frames = left.len();
        let n_ch = channels.len() as u16;

        let mut buf = Cursor::new(Vec::new());
        write_wav_header(&mut buf, n_ch, 16, 44100).unwrap();
        for frame in 0..num_frames {
            for ch in channels {
                let s = (ch[frame].clamp(-1.0, 1.0) * 32_767.0) as i16;
                buf.write_all(&s.to_le_bytes()).unwrap();
            }
        }
        finalize_wav(&mut buf, n_ch, 16, num_frames as u32).unwrap();

        // Read it back with our WAV parser.
        let parsed = crate::wav_read::parse_wav(buf.get_ref()).unwrap();
        assert_eq!(parsed.channels.len(), 2);
        assert_eq!(parsed.channels[0].len(), 3);
        assert!((parsed.sample_rate - 44100.0).abs() < 0.01);

        // 16-bit quantisation gives ~1/32768 error.
        for i in 0..3 {
            assert!(
                (parsed.channels[0][i] - left[i]).abs() < 0.001,
                "L[{i}]: {} vs {}",
                parsed.channels[0][i],
                left[i]
            );
            assert!(
                (parsed.channels[1][i] - right[i]).abs() < 0.001,
                "R[{i}]: {} vs {}",
                parsed.channels[1][i],
                right[i]
            );
        }
    }
}
