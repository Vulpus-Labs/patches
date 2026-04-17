//! Pure state machine for `PatternPlayer`.
//!
//! See ADR 0042 for the scope boundary.

use patches_core::TrackerData;

use crate::ClockBusFrame;

/// Per-sample state for the pattern player.
///
/// The module wrapper in `patches-modules` owns an instance of this struct
/// plus its input/output port handles. Each audio sample it decodes the
/// poly clock bus into a [`ClockBusFrame`], calls [`PatternPlayerCore::tick`],
/// and reads the core's per-channel output fields back into port buffers.
///
/// Output fields are `pub` to support direct inspection from tests. The
/// module wrapper in `patches-modules` reads them read-only.
pub struct PatternPlayerCore {
    pub sample_rate: f32,
    pub channels: usize,

    /// Absolute step index per channel (may exceed `pattern.steps`; the
    /// core wraps modulo `steps` at read time).
    pub step_index: Vec<usize>,
    /// Current cv1 value per channel.
    pub cv1: Vec<f32>,
    /// Current cv2 value per channel.
    pub cv2: Vec<f32>,
    /// Current gate state per channel.
    pub gate: Vec<bool>,
    /// Whether trigger should fire this sample.
    pub trigger_pending: Vec<bool>,
    /// Whether the player is in stop-sentinel state.
    pub stopped: bool,

    // Slide state per channel.
    pub slide_active: Vec<bool>,
    pub slide_cv1_start: Vec<f32>,
    pub slide_cv1_end: Vec<f32>,
    pub slide_cv2_start: Vec<f32>,
    pub slide_cv2_end: Vec<f32>,
    pub slide_samples_total: Vec<f32>,
    pub slide_samples_elapsed: Vec<f32>,

    // Repeat state per channel.
    pub repeat_active: Vec<bool>,
    pub repeat_count: Vec<u8>,
    pub repeat_index: Vec<u8>,
    pub repeat_interval_samples: Vec<f32>,
    pub repeat_samples_elapsed: Vec<f32>,
    /// Sample count at which the gate should drop before the next sub-trigger.
    pub repeat_gate_off_at: Vec<f32>,

    /// Cached tick duration in samples, set on each clock-bus tick edge.
    pub current_tick_duration_samples: f32,
    /// Previous tick-trigger clock value, for rising-edge detection.
    pub prev_tick_trigger: f32,
    /// Current active pattern bank index (set on the most recent tick edge).
    pub current_bank_index: Option<usize>,
}

impl PatternPlayerCore {
    pub fn new(sample_rate: f32, channels: usize) -> Self {
        Self {
            sample_rate,
            channels,
            step_index: vec![0; channels],
            cv1: vec![0.0; channels],
            cv2: vec![0.0; channels],
            gate: vec![false; channels],
            trigger_pending: vec![false; channels],
            stopped: false,
            slide_active: vec![false; channels],
            slide_cv1_start: vec![0.0; channels],
            slide_cv1_end: vec![0.0; channels],
            slide_cv2_start: vec![0.0; channels],
            slide_cv2_end: vec![0.0; channels],
            slide_samples_total: vec![0.0; channels],
            slide_samples_elapsed: vec![0.0; channels],
            repeat_active: vec![false; channels],
            repeat_count: vec![1; channels],
            repeat_index: vec![0; channels],
            repeat_interval_samples: vec![0.0; channels],
            repeat_samples_elapsed: vec![0.0; channels],
            repeat_gate_off_at: vec![f32::MAX; channels],
            current_tick_duration_samples: 0.0,
            prev_tick_trigger: 0.0,
            current_bank_index: None,
        }
    }

    /// Reset all channels to a silent stopped state.
    ///
    /// Called when the clock bus delivers a stop sentinel (bank index < 0).
    pub fn clear_all(&mut self) {
        for i in 0..self.channels {
            self.cv1[i] = 0.0;
            self.cv2[i] = 0.0;
            self.gate[i] = false;
            self.trigger_pending[i] = false;
            self.slide_active[i] = false;
            self.repeat_active[i] = false;
            self.repeat_gate_off_at[i] = f32::MAX;
        }
        self.stopped = true;
    }

    /// Apply one step event for a single channel.
    ///
    /// Pure state transition: sets `cv1`, `cv2`, `gate`, `trigger_pending`,
    /// slide and repeat state according to the current step of the pattern
    /// at `bank_index`. No effect if the tracker has no pattern at that
    /// index or if the channel is beyond the pattern's channel count
    /// (surplus channels go silent).
    pub fn apply_step(
        &mut self,
        channel: usize,
        bank_index: usize,
        step_fraction: f32,
        tracker: &TrackerData,
    ) {
        let Some(pattern) = tracker.patterns.patterns.get(bank_index) else {
            return;
        };

        if channel >= pattern.channels || channel >= pattern.data.len() {
            self.gate[channel] = false;
            self.trigger_pending[channel] = false;
            self.slide_active[channel] = false;
            self.repeat_active[channel] = false;
            return;
        }

        let step_idx = self.step_index[channel] % pattern.steps;
        let step = &pattern.data[channel][step_idx];

        if !step.gate {
            self.gate[channel] = false;
            self.trigger_pending[channel] = false;
            self.slide_active[channel] = false;
            self.repeat_active[channel] = false;
            return;
        }

        if !step.trigger {
            // Tie: gate stays high, no trigger. Continue any slide that
            // the tie may carry (`cv1_end` / `cv2_end`).
            self.gate[channel] = true;
            self.trigger_pending[channel] = false;
            self.repeat_active[channel] = false;
            if step.cv1_end.is_some() || step.cv2_end.is_some() {
                let elapsed_samples = step_fraction * self.current_tick_duration_samples;
                self.slide_active[channel] = true;
                self.slide_cv1_start[channel] = step.cv1;
                self.slide_cv1_end[channel] = step.cv1_end.unwrap_or(step.cv1);
                self.slide_cv2_start[channel] = step.cv2;
                self.slide_cv2_end[channel] = step.cv2_end.unwrap_or(step.cv2);
                self.slide_samples_total[channel] = self.current_tick_duration_samples;
                self.slide_samples_elapsed[channel] = elapsed_samples;
                let t = if self.current_tick_duration_samples > 0.0 {
                    (elapsed_samples / self.current_tick_duration_samples).min(1.0)
                } else {
                    0.0
                };
                self.cv1[channel] =
                    step.cv1 + t * (step.cv1_end.unwrap_or(step.cv1) - step.cv1);
                self.cv2[channel] =
                    step.cv2 + t * (step.cv2_end.unwrap_or(step.cv2) - step.cv2);
            } else {
                self.slide_active[channel] = false;
            }
            return;
        }

        // Normal step with trigger.
        self.gate[channel] = true;
        self.trigger_pending[channel] = true;

        let elapsed_samples = step_fraction * self.current_tick_duration_samples;

        if step.cv1_end.is_some() || step.cv2_end.is_some() {
            self.slide_active[channel] = true;
            self.slide_cv1_start[channel] = step.cv1;
            self.slide_cv1_end[channel] = step.cv1_end.unwrap_or(step.cv1);
            self.slide_cv2_start[channel] = step.cv2;
            self.slide_cv2_end[channel] = step.cv2_end.unwrap_or(step.cv2);
            self.slide_samples_total[channel] = self.current_tick_duration_samples;
            self.slide_samples_elapsed[channel] = elapsed_samples;
            let t = if self.current_tick_duration_samples > 0.0 {
                (elapsed_samples / self.current_tick_duration_samples).min(1.0)
            } else {
                0.0
            };
            self.cv1[channel] = step.cv1 + t * (step.cv1_end.unwrap_or(step.cv1) - step.cv1);
            self.cv2[channel] = step.cv2 + t * (step.cv2_end.unwrap_or(step.cv2) - step.cv2);
        } else {
            self.slide_active[channel] = false;
            self.cv1[channel] = step.cv1;
            self.cv2[channel] = step.cv2;
        }

        if step.repeat > 1 {
            self.repeat_active[channel] = true;
            self.repeat_count[channel] = step.repeat;
            let interval = self.current_tick_duration_samples / step.repeat as f32;
            self.repeat_interval_samples[channel] = interval;
            self.repeat_samples_elapsed[channel] = elapsed_samples;
            let repeat_idx = if interval > 0.0 {
                ((elapsed_samples / interval).floor() as u8 + 1).min(step.repeat)
            } else {
                1
            };
            self.repeat_index[channel] = repeat_idx;
            let last_trigger_at = (repeat_idx.saturating_sub(1)) as f32 * interval;
            self.repeat_gate_off_at[channel] = last_trigger_at + interval * 0.8;
        } else {
            self.repeat_active[channel] = false;
            self.repeat_gate_off_at[channel] = f32::MAX;
        }
    }

    /// One audio-sample of pattern-player advance.
    ///
    /// On a rising tick-trigger edge in the clock bus, applies the
    /// indicated step to every channel. Between ticks, advances slide
    /// interpolation and fires repeat sub-triggers.
    pub fn tick(&mut self, frame: &ClockBusFrame, tracker: &TrackerData) {
        let tick_rose =
            frame.tick_trigger >= 0.5 && self.prev_tick_trigger < 0.5;
        self.prev_tick_trigger = frame.tick_trigger;

        if tick_rose {
            if frame.bank_index < 0.0 {
                self.clear_all();
                return;
            }
            self.stopped = false;
            let bank_index = frame.bank_index.round() as usize;
            self.current_bank_index = Some(bank_index);
            self.current_tick_duration_samples = frame.tick_duration * self.sample_rate;

            let step_index = frame.step_index.round() as usize;
            let step_fraction = frame.step_fraction;
            for i in 0..self.channels {
                self.step_index[i] = step_index;
            }
            for ch in 0..self.channels {
                self.apply_step(ch, bank_index, step_fraction, tracker);
            }
            return;
        }

        if self.stopped {
            return;
        }

        for ch in 0..self.channels {
            self.trigger_pending[ch] = false;

            if self.slide_active[ch] {
                self.slide_samples_elapsed[ch] += 1.0;
                let t = if self.slide_samples_total[ch] > 0.0 {
                    (self.slide_samples_elapsed[ch] / self.slide_samples_total[ch]).min(1.0)
                } else {
                    1.0
                };
                self.cv1[ch] = self.slide_cv1_start[ch]
                    + t * (self.slide_cv1_end[ch] - self.slide_cv1_start[ch]);
                self.cv2[ch] = self.slide_cv2_start[ch]
                    + t * (self.slide_cv2_end[ch] - self.slide_cv2_start[ch]);
            }

            if self.repeat_active[ch] && self.repeat_index[ch] < self.repeat_count[ch] {
                self.repeat_samples_elapsed[ch] += 1.0;
                let elapsed = self.repeat_samples_elapsed[ch];

                if elapsed >= self.repeat_gate_off_at[ch] {
                    self.gate[ch] = false;
                }

                let next_trigger_at =
                    self.repeat_interval_samples[ch] * self.repeat_index[ch] as f32;
                if elapsed >= next_trigger_at {
                    self.trigger_pending[ch] = true;
                    self.gate[ch] = true;
                    self.repeat_index[ch] += 1;
                    let interval = self.repeat_interval_samples[ch];
                    self.repeat_gate_off_at[ch] = next_trigger_at + interval * 0.8;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
