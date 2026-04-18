use std::time::Instant;

use cpal::traits::DeviceTrait;
use cpal::{Stream, StreamConfig};

use patches_planner::ExecutionPlan;
use patches_engine::decimator::Decimator;
use patches_engine::execution_state::SUB_BLOCK_SIZE;
use patches_engine::midi::{AudioClock, EventQueueConsumer};
use patches_engine::processor::PatchProcessor;

use crate::engine::EngineError;

/// CPAL output callback.
///
/// Wraps a [`PatchProcessor`] (the backend-agnostic engine core) with
/// CPAL-specific concerns: plan delivery via ring buffer, output format
/// conversion, oversampling / decimation, WAV recording, audio input
/// capture, and MIDI sub-block scheduling.
pub(crate) struct AudioCallback {
    processor: PatchProcessor,
    plan_rx: rtrb::Consumer<ExecutionPlan>,
    channels: usize,
    /// `channels.trailing_zeros()` — the right-shift to convert a sample count to a frame count.
    channel_shift: u32,
    /// Samples remaining until the next MIDI sub-block boundary (in output frames).
    samples_until_next_midi: usize,
    /// Running sample counter, incremented by `control_period` (inner ticks) after each sub-block.
    sample_counter: u64,
    /// Consumer end of the MIDI event queue.
    event_queue: Option<EventQueueConsumer>,
    /// Raw pointer to the shared audio clock.
    ///
    /// # Safety
    /// Valid for the entire lifetime of the callback: `SoundEngine` drops the stream
    /// (and thus this callback) before releasing its `Arc<AudioClock>`.
    clock: *const AudioClock,
    /// Number of inner ticks executed per output frame (1, 2, 4, or 8).
    oversampling_factor: usize,
    /// Inner-tick count per MIDI sub-block.
    control_period: u64,
    /// Anti-aliasing decimator for the left output channel.
    decimator_l: Decimator,
    /// Anti-aliasing decimator for the right output channel.
    decimator_r: Decimator,
    /// Optional ring-buffer producer for WAV recording.
    record_tx: Option<rtrb::Producer<[f32; 2]>>,
    /// Scratch buffer for recording, flushed once per callback.
    record_scratch: Vec<[f32; 2]>,
    /// Optional ring-buffer consumer for audio input frames.
    input_rx: Option<rtrb::Consumer<[f32; 2]>>,
    /// Previous input frame for linear interpolation under oversampling.
    prev_input_frame: [f32; 2],
}

// SAFETY: `AudioCallback` is sent to the audio thread exactly once (when the
// CPAL stream is built) and never accessed from any other thread after that.
// The raw `*const AudioClock` is read-only on the audio thread and points to
// data owned by `SoundEngine`'s `Arc<AudioClock>`, which outlives the stream.
unsafe impl Send for AudioCallback {}

impl AudioCallback {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        plan_rx: rtrb::Consumer<ExecutionPlan>,
        processor: PatchProcessor,
        channels: usize,
        event_queue: Option<EventQueueConsumer>,
        clock: *const AudioClock,
        record_tx: Option<rtrb::Producer<[f32; 2]>>,
        oversampling_factor: usize,
        control_period_base: usize,
        input_rx: Option<rtrb::Consumer<[f32; 2]>>,
    ) -> Self {
        use patches_engine::oversampling::OversamplingFactor;
        let factor_enum = match oversampling_factor {
            2 => OversamplingFactor::X2,
            4 => OversamplingFactor::X4,
            8 => OversamplingFactor::X8,
            _ => OversamplingFactor::None,
        };
        Self {
            processor,
            plan_rx,
            channels,
            channel_shift: channels.trailing_zeros(),
            samples_until_next_midi: SUB_BLOCK_SIZE as usize,
            sample_counter: 0,
            event_queue,
            clock,
            oversampling_factor,
            control_period: (control_period_base * oversampling_factor) as u64,
            decimator_l: Decimator::new(factor_enum),
            decimator_r: Decimator::new(factor_enum),
            record_tx,
            record_scratch: Vec::with_capacity(8192),
            input_rx,
            prev_input_frame: [0.0, 0.0],
        }
    }

    fn process_chunk<T: cpal::SizedSample + cpal::FromSample<f32>>(
        &mut self,
        data: &mut [T],
        out_i: &mut usize,
        chunk: usize,
    ) {
        for _ in 0..chunk {
            let mut out_l = 0.0_f32;
            let mut out_r = 0.0_f32;

            // Pop one input frame per output frame.  Under oversampling the
            // value is linearly interpolated across inner ticks.
            let current_input = match self.input_rx {
                Some(ref mut rx) => rx.pop().unwrap_or(self.prev_input_frame),
                None => [0.0, 0.0],
            };

            for j in 0..self.oversampling_factor {
                // Write audio input to backplane with linear interpolation.
                if self.input_rx.is_some() {
                    let t = (j as f32 + 1.0) / self.oversampling_factor as f32;
                    let in_l = self.prev_input_frame[0]
                        + t * (current_input[0] - self.prev_input_frame[0]);
                    let in_r = self.prev_input_frame[1]
                        + t * (current_input[1] - self.prev_input_frame[1]);
                    self.processor.write_input(in_l, in_r);
                }

                let (inner_l, inner_r) = self.processor.tick();

                if let Some(l) = self.decimator_l.push(inner_l) {
                    out_l = l;
                }
                if let Some(r) = self.decimator_r.push(inner_r) {
                    out_r = r;
                }
            }

            if self.input_rx.is_some() {
                self.prev_input_frame = current_input;
            }

            if self.channels == 1 {
                data[*out_i] = T::from_sample((out_l + out_r) * 0.5_f32);
            } else {
                data[*out_i * self.channels] = T::from_sample(out_l);
                data[*out_i * self.channels + 1] = T::from_sample(out_r);
                for c in 2..self.channels {
                    data[*out_i * self.channels + c] = T::from_sample(0.0_f32);
                }
            }

            if self.record_tx.is_some() {
                self.record_scratch.push([out_l, out_r]);
            }

            *out_i += 1;
        }
    }

    /// Adopt a new plan if one has been published — wait-free, no allocation.
    fn receive_plan(&mut self) {
        if let Ok(new_plan) = self.plan_rx.pop() {
            self.processor.adopt_plan(new_plan);
        }
    }

    pub(crate) fn fill_buffer<T: cpal::SizedSample + cpal::FromSample<f32>>(
        &mut self,
        data: &mut [T],
        _info: &cpal::OutputCallbackInfo,
    ) {
        let playback_time = Instant::now();

        self.receive_plan();

        self.record_scratch.clear();

        let frames = if self.channels > 0 {
            data.len() >> self.channel_shift
        } else {
            0
        };
        let mut remaining = frames;
        let mut out_i: usize = 0;

        while remaining > 0 {
            if self.samples_until_next_midi == SUB_BLOCK_SIZE as usize {
                self.processor.dispatch_midi(
                    &mut self.event_queue,
                    self.sample_counter,
                    self.control_period,
                );
            }

            let chunk = self.samples_until_next_midi.min(remaining);

            self.process_chunk(data, &mut out_i, chunk);

            self.samples_until_next_midi -= chunk;
            remaining -= chunk;

            if self.samples_until_next_midi == 0 {
                self.sample_counter += self.control_period;
                self.samples_until_next_midi = SUB_BLOCK_SIZE as usize;
            }
        }

        if let Some(ref mut tx) = self.record_tx {
            for &frame in &self.record_scratch {
                let _ = tx.push(frame);
            }
        }

        // SAFETY: see field doc.
        unsafe { &*self.clock }.publish(self.sample_counter, playback_time);
    }
}

pub(crate) fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut callback: AudioCallback,
) -> Result<Stream, EngineError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                callback.fill_buffer(data, info);
            },
            |err| eprintln!("patches audio stream error: {err}"),
            None,
        )
        .map_err(EngineError::BuildStreamError)
}
