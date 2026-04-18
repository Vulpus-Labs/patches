//! Audio input capture via CPAL.
//!
//! Opens a CPAL input stream on a specified device and bridges incoming audio
//! frames to the output callback via an [`rtrb`] ring buffer.  The output
//! callback pops `[f32; 2]` stereo frames and writes them to the
//! `AUDIO_IN_L`/`AUDIO_IN_R` backplane slots.
//!
//! Mono input devices are handled by duplicating the single channel to both
//! stereo channels before pushing to the ring buffer.

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

use crate::engine::EngineError;

/// Ring-buffer capacity in stereo frames (~1.5 s at 44.1 kHz).
const INPUT_RING_CAPACITY: usize = 65_536;

/// Handle for an open CPAL input stream.
///
/// Holds the CPAL [`Stream`] alive.  Dropping this stops the input stream.
pub(crate) struct InputCapture {
    stream: Stream,
}

impl InputCapture {
    /// Start the input stream.  Must be called after construction and before
    /// the output callback begins popping frames.
    pub(crate) fn play(&self) -> Result<(), cpal::PlayStreamError> {
        self.stream.play()
    }
}

/// Open a CPAL input stream on `device` and return:
/// - An [`InputCapture`] handle (hold it to keep the stream alive).
/// - An `rtrb::Consumer<[f32; 2]>` for the output callback to pop from.
///
/// `expected_sample_rate` is the output device's sample rate in Hz.  If the
/// input device's default configuration reports a different rate,
/// [`EngineError::SampleRateMismatch`] is returned.
pub(crate) fn open_input(
    device: &Device,
    expected_sample_rate: u32,
) -> Result<(InputCapture, rtrb::Consumer<[f32; 2]>), EngineError> {
    let supported = device
        .default_input_config()
        .map_err(EngineError::DefaultConfigError)?;

    let input_rate = supported.sample_rate().0;
    if input_rate != expected_sample_rate {
        return Err(EngineError::SampleRateMismatch {
            input: input_rate,
            output: expected_sample_rate,
        });
    }

    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();

    let (tx, rx) = rtrb::RingBuffer::<[f32; 2]>::new(INPUT_RING_CAPACITY);

    let stream = match sample_format {
        SampleFormat::F32 => build_input_stream::<f32>(device, &config, channels, tx),
        SampleFormat::I16 => build_input_stream::<i16>(device, &config, channels, tx),
        SampleFormat::U16 => build_input_stream::<u16>(device, &config, channels, tx),
        other => return Err(EngineError::UnsupportedSampleFormat(other)),
    }?;

    Ok((InputCapture { stream }, rx))
}

fn build_input_stream<T>(
    device: &Device,
    config: &StreamConfig,
    channels: usize,
    mut tx: rtrb::Producer<[f32; 2]>,
) -> Result<Stream, EngineError>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _info: &cpal::InputCallbackInfo| {
                let frame_count = if channels > 0 { data.len() / channels } else { 0 };
                for i in 0..frame_count {
                    let left: f32 = cpal::FromSample::from_sample_(data[i * channels]);
                    let right: f32 = if channels >= 2 {
                        cpal::FromSample::from_sample_(data[i * channels + 1])
                    } else {
                        left
                    };
                    // Silently drop frames if ring buffer is full (same policy
                    // as WavRecorder).
                    let _ = tx.push([left, right]);
                }
            },
            |err| eprintln!("patches audio input stream error: {err}"),
            None,
        )
        .map_err(EngineError::BuildInputStreamError)
}
