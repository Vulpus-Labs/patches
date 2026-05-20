//! Pure state machine for `PatternPlayer`.
//!
//! See ADR 0042 for the scope boundary.

use patches_core::{StepEffect, TrackerData};

use super::ClockBusFrame;

/// Maximum number of sub-events per channel. Caps the `*N` roll count
/// (and future multi-tick slide segment count). Sized so the per-channel
/// `Vec<SubEvent>` can be preallocated and never reallocates on the
/// audio thread.
pub const SUB_EVENT_CAPACITY: usize = 16;

/// One scheduled sub-trigger within an active `*N` roll span.
///
/// `tick_idx_in_span` is the tick index inside the roll's span (0 for the
/// anchor tick, 1 for the first absorbed-tie tick, etc.). On every
/// absorbed-roll tick rise the field is decremented for all *remaining*
/// (unfired) sub-events, so once decremented to 0 the sub-event fires in
/// the current tick when `current_tick_elapsed_samples >= fraction *
/// current_tick_duration_samples`. `fraction` is the position within
/// that tick (`t_k - floor(t_k)` where `t_k = k / N * span`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SubEvent {
    pub tick_idx_in_span: u8,
    pub fraction: f32,
}

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
    /// Per-channel sub-event schedule. Capacity is fixed at
    /// [`SUB_EVENT_CAPACITY`] and `clear()` is used on reset so no
    /// audio-thread allocations occur.
    pub sub_events: Vec<Vec<SubEvent>>,
    /// Index of the next pending sub-event in `sub_events[ch]`. When
    /// `>= sub_events[ch].len()` the roll has fired its last sub-trigger.
    pub sub_event_head: Vec<usize>,
    /// Sample countdown until the gate should drop ahead of the next
    /// sub-trigger. Set to `0.8 * samples_to_next_sub_event` at each
    /// firing; decremented per inter-tick sample. `f32::MAX` means
    /// "no pending drop".
    pub repeat_gate_off_countdown: Vec<f32>,

    /// Cached tick duration in samples, set on each clock-bus tick edge.
    pub current_tick_duration_samples: f32,
    /// Sample count elapsed inside the current tick. Reset on each tick
    /// rise (to `step_fraction * tick_duration_samples`, normally 0),
    /// incremented by 1 per inter-tick sample. Sub-event firings are
    /// compared against this value using the *current* tick's duration,
    /// so rolls that straddle a swung tick boundary place their
    /// sub-triggers correctly.
    pub current_tick_elapsed_samples: f32,
    /// Previous tick-trigger clock value, for rising-edge detection.
    pub prev_tick_trigger: f32,
    /// Current active pattern bank index (set on the most recent tick edge).
    pub current_bank_index: Option<usize>,

    /// Number of channels with `slide_active = true`. Lets the inter-tick
    /// advance loop short-circuit when no channel needs slide interpolation.
    pub slide_active_count: usize,
    /// Number of channels with `repeat_active = true` and sub-triggers
    /// still pending. Lets the inter-tick loop short-circuit when no
    /// channel has unfired repeats.
    pub repeat_active_count: usize,
    /// Any `trigger_pending[ch]` is true and must be cleared on the next
    /// sample. Avoids per-sample writes to a vector that is overwhelmingly
    /// all-`false`.
    pub trigger_clear_needed: bool,
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
            sub_events: (0..channels)
                .map(|_| Vec::with_capacity(SUB_EVENT_CAPACITY))
                .collect(),
            sub_event_head: vec![0; channels],
            repeat_gate_off_countdown: vec![f32::MAX; channels],
            current_tick_duration_samples: 0.0,
            current_tick_elapsed_samples: 0.0,
            prev_tick_trigger: 0.0,
            current_bank_index: None,
            slide_active_count: 0,
            repeat_active_count: 0,
            trigger_clear_needed: false,
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
            self.sub_events[i].clear();
            self.sub_event_head[i] = 0;
            self.repeat_gate_off_countdown[i] = f32::MAX;
        }
        self.stopped = true;
        self.slide_active_count = 0;
        self.repeat_active_count = 0;
        self.trigger_clear_needed = false;
        self.current_tick_elapsed_samples = 0.0;
    }

    /// Apply one step event for a single channel.
    ///
    /// Pure state transition: dispatches on `step.effect` (the
    /// channel-stateful row-build pass set this in
    /// [`patches_core::resolve_step_effects`]) and updates `cv1`, `cv2`,
    /// `gate`, `trigger_pending`, slide and repeat state. No effect if
    /// the tracker has no pattern at `bank_index` or the channel is
    /// beyond the pattern's channel count (surplus channels go silent).
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
        let elapsed_samples = step_fraction * self.current_tick_duration_samples;

        match step.effect {
            StepEffect::Silent => {
                self.gate[channel] = false;
                self.trigger_pending[channel] = false;
                self.slide_active[channel] = false;
                self.repeat_active[channel] = false;
                self.repeat_gate_off_countdown[channel] = f32::MAX;
            }
            StepEffect::AbsorbedRoll => {
                // E152/E153: tie cells absorbed by a preceding `*N` roll
                // anchor must not re-run slide/repeat/trigger logic — the
                // anchor's in-flight schedule owns the channel until it
                // finishes. Normalise every remaining sub-event's
                // `tick_idx_in_span` against the new current tick, then
                // check whether the head sub-event fires at the rising
                // edge (e.g. a fraction-0 sub-event at the tick boundary).
                if self.repeat_active[channel] {
                    let head = self.sub_event_head[channel];
                    for ev in &mut self.sub_events[channel][head..] {
                        ev.tick_idx_in_span = ev.tick_idx_in_span.saturating_sub(1);
                    }
                    self.check_sub_event_fire(channel);
                }
            }
            StepEffect::Hold => {
                self.gate[channel] = true;
                self.trigger_pending[channel] = false;
                self.slide_active[channel] = false;
                self.repeat_active[channel] = false;
                self.repeat_gate_off_countdown[channel] = f32::MAX;
            }
            StepEffect::SlideFlow => {
                // The head opened a multi-tick slide; this tick is a
                // flow-through. Don't re-arm — just advance by one sample
                // to account for the rising-edge sample missed by the
                // inter-tick loop (mirrors `AbsorbedRoll`'s logic).
                self.gate[channel] = true;
                self.trigger_pending[channel] = false;
                self.repeat_active[channel] = false;
                if self.slide_active[channel] {
                    self.advance_slide_one_sample(channel);
                }
            }
            StepEffect::StepCv { cv1, cv2 } => {
                self.gate[channel] = true;
                self.trigger_pending[channel] = false;
                self.slide_active[channel] = false;
                self.repeat_active[channel] = false;
                self.repeat_gate_off_countdown[channel] = f32::MAX;
                self.cv1[channel] = cv1;
                if let Some(c) = cv2 {
                    self.cv2[channel] = c;
                }
            }
            StepEffect::SlideCloseInTick { cv1, cv2: cv2_opt } => {
                self.gate[channel] = true;
                self.trigger_pending[channel] = false;
                self.repeat_active[channel] = false;
                self.repeat_gate_off_countdown[channel] = f32::MAX;
                let start_cv1 = self.cv1[channel];
                let start_cv2 = self.cv2[channel];
                let end_cv2 = cv2_opt.unwrap_or(start_cv2);
                self.slide_active[channel] = true;
                self.slide_cv1_start[channel] = start_cv1;
                self.slide_cv1_end[channel] = cv1;
                self.slide_cv2_start[channel] = start_cv2;
                self.slide_cv2_end[channel] = end_cv2;
                self.slide_samples_total[channel] = self.current_tick_duration_samples;
                self.slide_samples_elapsed[channel] = elapsed_samples;
                let t = if self.current_tick_duration_samples > 0.0 {
                    (elapsed_samples / self.current_tick_duration_samples).min(1.0)
                } else {
                    0.0
                };
                self.cv1[channel] = start_cv1 + t * (cv1 - start_cv1);
                self.cv2[channel] = start_cv2 + t * (end_cv2 - start_cv2);
            }
            StepEffect::OpenSlide { ref slide } => {
                // `>_` opening a new slide from the channel's current cv.
                // No trigger; gate stays. The slide schedule covers
                // `slide.span` ticks ramping from current cv to
                // slide.close_cv1.
                self.gate[channel] = true;
                self.trigger_pending[channel] = false;
                self.repeat_active[channel] = false;
                self.repeat_gate_off_countdown[channel] = f32::MAX;
                let span = slide.span.max(1) as f32;
                let total = self.current_tick_duration_samples * span;
                let start_cv1 = self.cv1[channel];
                let start_cv2 = self.cv2[channel];
                self.slide_active[channel] = true;
                self.slide_cv1_start[channel] = start_cv1;
                self.slide_cv1_end[channel] = slide.close_cv1;
                self.slide_cv2_start[channel] = start_cv2;
                self.slide_cv2_end[channel] = slide.close_cv2.unwrap_or(start_cv2);
                self.slide_samples_total[channel] = total;
                self.slide_samples_elapsed[channel] = elapsed_samples;
                let t = if total > 0.0 {
                    (elapsed_samples / total).min(1.0)
                } else {
                    0.0
                };
                self.cv1[channel] = start_cv1 + t * (slide.close_cv1 - start_cv1);
                self.cv2[channel] = start_cv2 + t * (self.slide_cv2_end[channel] - start_cv2);
            }
            StepEffect::StartNote { cv1, cv2, ref slide, ref roll } => {
                self.gate[channel] = true;
                self.trigger_pending[channel] = true;
                self.cv1[channel] = cv1;
                self.cv2[channel] = cv2;

                if let Some(so) = slide {
                    let span = so.span.max(1) as f32;
                    let total = self.current_tick_duration_samples * span;
                    self.slide_active[channel] = true;
                    self.slide_cv1_start[channel] = cv1;
                    self.slide_cv1_end[channel] = so.close_cv1;
                    self.slide_cv2_start[channel] = cv2;
                    self.slide_cv2_end[channel] = so.close_cv2.unwrap_or(cv2);
                    self.slide_samples_total[channel] = total;
                    self.slide_samples_elapsed[channel] = elapsed_samples;
                    let t = if total > 0.0 {
                        (elapsed_samples / total).min(1.0)
                    } else {
                        0.0
                    };
                    self.cv1[channel] = cv1 + t * (so.close_cv1 - cv1);
                    self.cv2[channel] =
                        cv2 + t * (self.slide_cv2_end[channel] - cv2);
                } else {
                    self.slide_active[channel] = false;
                }

                if let Some(r) = roll {
                    // E152/E153: build a per-channel sub-event schedule
                    // `t_k = k / N * S` for `k = 0..N-1`, split into
                    // `(tick_idx = floor(t_k), fraction = t_k - tick_idx)`.
                    // The anchor (k = 0) is consumed inline (the
                    // `trigger_pending` set above fires it); remaining
                    // entries are queued and matched against the
                    // *current* tick's duration on firing, so swung
                    // patterns place their sub-triggers at the right
                    // clock time even across the tick boundary.
                    let count = r.count.max(1) as usize;
                    let span = r.span.max(1) as f32;
                    let sched = &mut self.sub_events[channel];
                    sched.clear();
                    for k in 0..count {
                        let t_k = (k as f32 / count as f32) * span;
                        let tick_idx = t_k.floor();
                        let fraction = t_k - tick_idx;
                        sched.push(SubEvent {
                            tick_idx_in_span: tick_idx as u8,
                            fraction,
                        });
                    }
                    self.sub_event_head[channel] = 1.min(sched.len());
                    self.repeat_active[channel] = self.sub_event_head[channel] < sched.len();

                    // Gate-off countdown to the next sub-event (if any).
                    // Cross-tick distance uses the anchor tick's duration
                    // as an approximation; non-swung patterns stay
                    // bit-identical with the previous formula.
                    self.repeat_gate_off_countdown[channel] = match sched.get(1) {
                        Some(next) => {
                            let dur = self.current_tick_duration_samples;
                            let dist = if next.tick_idx_in_span == 0 {
                                next.fraction * dur - elapsed_samples
                            } else {
                                let to_end = dur - elapsed_samples;
                                let across = (next.tick_idx_in_span as f32 - 1.0
                                    + next.fraction)
                                    * dur;
                                to_end + across
                            };
                            dist * 0.8
                        }
                        None => f32::MAX,
                    };
                } else {
                    self.repeat_active[channel] = false;
                    self.repeat_gate_off_countdown[channel] = f32::MAX;
                    self.sub_events[channel].clear();
                    self.sub_event_head[channel] = 0;
                }
            }
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
            self.current_tick_elapsed_samples =
                frame.step_fraction * self.current_tick_duration_samples;

            let step_index = frame.step_index.round() as usize;
            let step_fraction = frame.step_fraction;
            for ch in 0..self.channels {
                self.step_index[ch] = step_index;
                self.apply_step(ch, bank_index, step_fraction, tracker);
            }

            // Rebuild activity counts from the per-channel flags settled by
            // apply_step. A channel's repeat is "done" if its schedule
            // head has caught up with the schedule length (can happen
            // when entering mid-step with `step_fraction` near 1.0).
            let mut slide_count = 0;
            let mut repeat_count = 0;
            let mut any_trigger = false;
            for ch in 0..self.channels {
                if self.slide_active[ch] {
                    slide_count += 1;
                }
                if self.repeat_active[ch] {
                    if self.sub_event_head[ch] >= self.sub_events[ch].len() {
                        self.repeat_active[ch] = false;
                    } else {
                        repeat_count += 1;
                    }
                }
                if self.trigger_pending[ch] {
                    any_trigger = true;
                }
            }
            self.slide_active_count = slide_count;
            self.repeat_active_count = repeat_count;
            self.trigger_clear_needed = any_trigger;
            return;
        }

        if self.stopped {
            return;
        }

        if self.trigger_clear_needed {
            for ch in 0..self.channels {
                self.trigger_pending[ch] = false;
            }
            self.trigger_clear_needed = false;
        }

        if self.slide_active_count == 0 && self.repeat_active_count == 0 {
            return;
        }

        self.current_tick_elapsed_samples += 1.0;

        for ch in 0..self.channels {
            if self.slide_active[ch] {
                self.advance_slide_one_sample(ch);
            }

            if self.repeat_active[ch] {
                self.advance_roll_one_sample(ch);
            }
        }
    }

    /// Advance an in-flight slide on channel `ch` by a single sample:
    /// bumps elapsed, snaps to end + deactivates when the schedule is
    /// exhausted, otherwise lerps cv1/cv2 at the new fractional position.
    /// Used by the inter-tick loop and by `apply_step` for `SlideFlow`
    /// cells (a head opens a multi-tick slide; tails just need to count
    /// the rising-edge sample so the schedule doesn't drift).
    fn advance_slide_one_sample(&mut self, ch: usize) {
        self.slide_samples_elapsed[ch] += 1.0;
        let total = self.slide_samples_total[ch];
        if total > 0.0 && self.slide_samples_elapsed[ch] >= total {
            self.cv1[ch] = self.slide_cv1_end[ch];
            self.cv2[ch] = self.slide_cv2_end[ch];
            self.slide_active[ch] = false;
            self.slide_active_count = self.slide_active_count.saturating_sub(1);
        } else {
            let t = if total > 0.0 {
                self.slide_samples_elapsed[ch] / total
            } else {
                1.0
            };
            self.cv1[ch] = self.slide_cv1_start[ch]
                + t * (self.slide_cv1_end[ch] - self.slide_cv1_start[ch]);
            self.cv2[ch] = self.slide_cv2_start[ch]
                + t * (self.slide_cv2_end[ch] - self.slide_cv2_start[ch]);
        }
    }

    /// Advance one in-flight `*N` roll on channel `ch` by a single
    /// sample: ticks the gate-off countdown and checks whether the
    /// head sub-event fires. Used by the inter-tick loop.
    fn advance_roll_one_sample(&mut self, ch: usize) {
        let countdown = self.repeat_gate_off_countdown[ch];
        if countdown < f32::MAX {
            let next = countdown - 1.0;
            if next <= 0.0 {
                self.gate[ch] = false;
                self.repeat_gate_off_countdown[ch] = f32::MAX;
            } else {
                self.repeat_gate_off_countdown[ch] = next;
            }
        }

        self.check_sub_event_fire(ch);
    }

    /// Fire the head sub-event on channel `ch` if its scheduled time
    /// has been reached in the current tick. Called from
    /// `advance_roll_one_sample` (per inter-tick sample) and from
    /// `apply_step` on an absorbed-tie tick rise (after normalising the
    /// head's `tick_idx_in_span` against the new tick).
    fn check_sub_event_fire(&mut self, ch: usize) {
        let head_idx = self.sub_event_head[ch];
        if head_idx >= self.sub_events[ch].len() {
            return;
        }
        let head = self.sub_events[ch][head_idx];
        if head.tick_idx_in_span != 0 {
            return;
        }
        let dur = self.current_tick_duration_samples;
        let fire_at = head.fraction * dur;
        // Inclusive comparison with a sub-sample tolerance so the
        // float-precision representation of `k / N * dur` aligns with
        // integer sample boundaries (matters for `x*3` where
        // `1/3 * 300 ≈ 100.0000076`).
        if self.current_tick_elapsed_samples + 1e-3 >= fire_at {
            self.trigger_pending[ch] = true;
            self.trigger_clear_needed = true;
            self.gate[ch] = true;
            self.sub_event_head[ch] += 1;

            let new_head = self.sub_event_head[ch];
            if new_head >= self.sub_events[ch].len() {
                self.repeat_active[ch] = false;
                self.repeat_active_count = self.repeat_active_count.saturating_sub(1);
                self.repeat_gate_off_countdown[ch] = f32::MAX;
            } else {
                let next = self.sub_events[ch][new_head];
                let dist = if next.tick_idx_in_span == 0 {
                    next.fraction * dur - self.current_tick_elapsed_samples
                } else {
                    let to_end = dur - self.current_tick_elapsed_samples;
                    let across =
                        (next.tick_idx_in_span as f32 - 1.0 + next.fraction) * dur;
                    to_end + across
                };
                self.repeat_gate_off_countdown[ch] = dist * 0.8;
            }
        }
    }
}

#[cfg(test)]
mod tests;
