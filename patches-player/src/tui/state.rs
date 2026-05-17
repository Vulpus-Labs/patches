//! TUI state types and pure helpers (no rendering).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use patches_dsl::manifest::{Manifest, TapType};
use patches_plugin_common::Action;
use std::path::PathBuf;
use patches_observation::processor::{
    spectrum_bin_count, SpectrumReadOpts, SCOPE_RING_SAMPLES, SPECTRUM_FFT_SIZE_DEFAULT,
    SPECTRUM_FFT_SIZE_MAX,
};
use patches_observation::subscribers::{Diagnostic, SubscribersHandle};

/// Build the TUI's tap list from a manifest snapshot. Sort by slot so
/// the meter pane order is deterministic.
pub fn taps_from_manifest(manifest: &Manifest) -> Vec<TapEntry> {
    let mut taps: Vec<TapEntry> = manifest
        .iter()
        .map(|d| TapEntry {
            name: d.name.clone(),
            slot: d.slot,
            components: d.components.clone(),
        })
        .collect();
    taps.sort_by_key(|t| t.slot);
    taps
}

/// Format an observer diagnostic for the event log per ticket 0705
/// acceptance criteria.
pub fn format_diagnostic(d: &Diagnostic) -> String {
    d.render()
}

/// One event-log entry: wall-clock timestamp + message text.
#[derive(Clone, Debug)]
pub struct LogEntry {
    /// Seconds since the UNIX epoch when the entry was pushed. Rendered
    /// as `HH:MM:SS` UTC in the event pane.
    pub epoch_secs: u64,
    pub msg: String,
}

/// Format an epoch-second count as `HH:MM:SS` in UTC.
pub fn format_hms(epoch_secs: u64) -> String {
    let s = epoch_secs % 86_400;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    format!("{h:02}:{m:02}:{sec:02}")
}

/// Bounded ring of event-log lines.
pub struct EventLog {
    pub(crate) lines: VecDeque<LogEntry>,
    cap: usize,
}

impl EventLog {
    pub fn new(cap: usize) -> Self {
        Self { lines: VecDeque::with_capacity(cap), cap }
    }

    pub fn push(&mut self, msg: impl Into<String>) {
        let epoch_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.push_at(epoch_secs, msg);
    }

    /// Push with an explicit timestamp. Test helper; production callers
    /// use [`Self::push`] which stamps via `SystemTime::now`.
    pub fn push_at(&mut self, epoch_secs: u64, msg: impl Into<String>) {
        if self.lines.len() == self.cap {
            self.lines.pop_front();
        }
        self.lines.push_back(LogEntry { epoch_secs, msg: msg.into() });
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

/// Word-wrap `msg` into visual lines no wider than `width`. The first
/// line is prefixed with `prefix`; continuation lines are indented by
/// `prefix.chars().count()` spaces so the message column stays aligned.
/// Words longer than `width` are hard-split.
pub fn wrap_with_prefix(prefix: &str, msg: &str, width: usize) -> Vec<String> {
    let prefix_w = prefix.chars().count();
    let indent: String = " ".repeat(prefix_w);
    let avail = width.saturating_sub(prefix_w).max(1);

    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    let mut first = true;

    let push_line = |out: &mut Vec<String>, line: String, first: &mut bool| {
        if *first {
            out.push(format!("{prefix}{line}"));
            *first = false;
        } else {
            out.push(format!("{indent}{line}"));
        }
    };

    for word in msg.split_whitespace() {
        let w = word.chars().count();
        if w > avail {
            if !cur.is_empty() {
                push_line(&mut out, std::mem::take(&mut cur), &mut first);
                cur_w = 0;
            }
            let mut chars = word.chars().peekable();
            while chars.peek().is_some() {
                let chunk: String = chars.by_ref().take(avail).collect();
                push_line(&mut out, chunk, &mut first);
            }
            continue;
        }
        let needed = if cur.is_empty() { w } else { cur_w + 1 + w };
        if needed > avail {
            push_line(&mut out, std::mem::take(&mut cur), &mut first);
            cur_w = 0;
        }
        if cur.is_empty() {
            cur.push_str(word);
            cur_w = w;
        } else {
            cur.push(' ');
            cur.push_str(word);
            cur_w += 1 + w;
        }
    }
    if !cur.is_empty() {
        push_line(&mut out, cur, &mut first);
    }
    if out.is_empty() {
        out.push(prefix.to_string());
    }
    out
}

/// One declared tap (name, slot index, declared component types).
#[derive(Clone, Debug)]
pub struct TapEntry {
    pub name: String,
    pub slot: usize,
    pub components: Vec<TapType>,
}

impl TapEntry {
    pub fn has(&self, t: TapType) -> bool {
        self.components.contains(&t)
    }
}

/// Active tab in the TUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Meters,
    Spectrum,
    Scope,
    Events,
    Cpu,
}

/// Spectrum-tab render mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpectrumMode {
    Curves,
    Heatmap,
}

impl Tab {
    pub fn next(self) -> Self {
        match self {
            Tab::Events => Tab::Meters,
            Tab::Meters => Tab::Spectrum,
            Tab::Spectrum => Tab::Scope,
            Tab::Scope => Tab::Cpu,
            Tab::Cpu => Tab::Events,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Tab::Meters => "meters",
            Tab::Spectrum => "spectrum",
            Tab::Scope => "scope",
            Tab::Events => "events",
            Tab::Cpu => "cpu",
        }
    }
}

/// Engine-level header info displayed above the meter pane.
#[derive(Clone, Debug)]
pub struct HeaderInfo {
    pub patch_path: String,
    pub sample_rate: u32,
    pub oversampling: u32,
}

/// Recording state visible to the user. The audio side honours the
/// `muted` flag if `record_path` is `Some`; otherwise `r` is a no-op
/// that logs a hint.
pub struct RecordState {
    pub record_path: Option<String>,
    pub muted: Option<Arc<AtomicBool>>,
}

/// Engine run state surfaced in the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineState {
    Running,
    Halted,
}

/// Minimum interval between successive drop-count log entries for the
/// same slot. Keeps the event log readable when the observer is slow.
pub const DROP_LOG_INTERVAL: Duration = Duration::from_secs(2);

/// Maximum heatmap history depth (frames). Wider than any plausible
/// terminal so pane resize / tab switch never loses data.
pub const HEATMAP_HISTORY_CAP: usize = 1024;

/// Exponential-smoothing weight for spectrum curve magnitudes.
pub const SPECTRUM_SMOOTH_ALPHA: f32 = 0.7;

/// Mode of an open bundle-dir prompt (ADR 0075, ticket 0898).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    AddBundleDir,
    RemoveBundleDir,
}

/// One-shot inline text prompt — captures keystrokes until the user
/// commits with Enter or cancels with Esc. The TUI renders the live
/// input at the bottom of the events pane (or wherever the renderer
/// chooses to surface it).
#[derive(Clone, Debug)]
pub struct Prompt {
    pub kind: PromptKind,
    pub input: String,
}

impl Prompt {
    pub fn label(&self) -> &'static str {
        match self.kind {
            PromptKind::AddBundleDir => "Add bundle dir",
            PromptKind::RemoveBundleDir => "Remove bundle dir",
        }
    }
}

/// Mutable view state shared between the input loop and the draw loop.
pub struct View {
    pub header: HeaderInfo,
    pub taps: Vec<TapEntry>,
    pub log: EventLog,
    pub record: RecordState,
    pub engine_state: EngineState,
    pub meter_scroll: usize,
    pub log_scroll: usize,
    pub tab: Tab,
    pub(crate) spectrum_scratch: Vec<f32>,
    pub(crate) scope_scratch: Vec<f32>,
    pub spectrum_opts: SpectrumReadOpts,
    pub scope_snap_zero: bool,
    pub(crate) spectrum_smoothed: HashMap<String, Vec<f32>>,
    pub scope_window_ms: f32,
    pub spectrum_mode: SpectrumMode,
    pub(crate) heatmap_history: VecDeque<Vec<f32>>,
    heatmap_bins: usize,
    pub(crate) drop_seen: HashMap<String, u64>,
    drop_logged_at: HashMap<String, Instant>,
    pub(crate) trigger_flash_at: HashMap<String, Instant>,
    pub cpu_snapshot: Option<Arc<std::sync::Mutex<crate::cpu_monitor::CpuSnapshot>>>,
    /// Open bundle-dir prompt, if any. Set by 'b' / 'B' key handlers;
    /// cleared on Enter (commit) or Esc (cancel).
    pub prompt: Option<Prompt>,
    /// Queue of bundle-dir actions waiting for the main loop to apply.
    /// `tui::run`'s `on_tick` closure drains this and dispatches into
    /// the controller — keeping action plumbing out of the TUI module.
    pending_actions: VecDeque<Action>,
}

impl View {
    pub fn new(header: HeaderInfo, taps: Vec<TapEntry>, record: RecordState) -> Self {
        Self {
            header,
            taps,
            log: EventLog::new(256),
            record,
            engine_state: EngineState::Running,
            meter_scroll: 0,
            log_scroll: 0,
            tab: Tab::Meters,
            spectrum_scratch: Vec::with_capacity(spectrum_bin_count(SPECTRUM_FFT_SIZE_MAX)),
            scope_scratch: Vec::with_capacity(SCOPE_RING_SAMPLES),
            spectrum_opts: SpectrumReadOpts { fft_size: SPECTRUM_FFT_SIZE_DEFAULT },
            scope_window_ms: 50.0,
            scope_snap_zero: false,
            spectrum_smoothed: HashMap::new(),
            spectrum_mode: SpectrumMode::Curves,
            heatmap_history: VecDeque::new(),
            heatmap_bins: 0,
            drop_seen: HashMap::new(),
            drop_logged_at: HashMap::new(),
            trigger_flash_at: HashMap::new(),
            cpu_snapshot: None,
            prompt: None,
            pending_actions: VecDeque::new(),
        }
    }

    /// Take the next queued bundle-dir action, if any. Returns `None`
    /// when the prompt is idle and the queue is empty.
    pub fn take_pending_bundle_action(&mut self) -> Option<Action> {
        self.pending_actions.pop_front()
    }

    /// Commit the open prompt by enqueueing the matching action and
    /// closing the prompt. Empty input cancels with a log line so the
    /// user sees the no-op.
    pub fn commit_prompt(&mut self) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        let trimmed = prompt.input.trim();
        if trimmed.is_empty() {
            self.log.push(format!("{}: cancelled (empty path)", prompt.label()));
            return;
        }
        let path = PathBuf::from(trimmed);
        let action = match prompt.kind {
            PromptKind::AddBundleDir => Action::AddBundleDir(path),
            PromptKind::RemoveBundleDir => Action::RemoveBundleDir(path),
        };
        self.pending_actions.push_back(action);
    }

    /// Open a new prompt; if one is already open, leave it alone — the
    /// user should commit or cancel first. Re-pressing the same key is
    /// a deliberate no-op rather than a confusing reset.
    pub fn open_prompt(&mut self, kind: PromptKind) {
        if self.prompt.is_some() {
            return;
        }
        self.prompt = Some(Prompt {
            kind,
            input: String::new(),
        });
    }

    /// Cancel any open prompt. Idempotent.
    pub fn cancel_prompt(&mut self) {
        if let Some(p) = self.prompt.take() {
            self.log.push(format!("{}: cancelled", p.label()));
        }
    }

    pub fn attach_cpu_snapshot(
        &mut self,
        snap: Arc<std::sync::Mutex<crate::cpu_monitor::CpuSnapshot>>,
    ) {
        self.cpu_snapshot = Some(snap);
    }

    /// Replace the active tap list (e.g. on patch reload). Drop-counter
    /// baselines for slots that survive the change are preserved so a
    /// reload doesn't generate spurious "drops" log lines.
    pub fn set_taps(&mut self, taps: Vec<TapEntry>) {
        let surviving: std::collections::HashSet<&str> =
            taps.iter().map(|t| t.name.as_str()).collect();
        self.drop_seen.retain(|name, _| surviving.contains(name.as_str()));
        self.drop_logged_at.retain(|name, _| surviving.contains(name.as_str()));
        self.trigger_flash_at.retain(|name, _| surviving.contains(name.as_str()));
        let max_scroll = taps.len().saturating_sub(1);
        if self.meter_scroll > max_scroll {
            self.meter_scroll = max_scroll;
        }
        self.taps = taps;
    }

    /// Seed drop baselines for newly appearing tap names from the
    /// current ring drop counters.
    pub fn seed_drop_baselines(&mut self, handle: &SubscribersHandle) {
        for tap in &self.taps {
            self.drop_seen
                .entry(tap.name.clone())
                .or_insert_with(|| handle.dropped(tap.slot));
        }
    }

    /// Surface advancing per-slot drop counters as event-log lines,
    /// rate-limited per slot.
    pub fn poll_drops(&mut self, handle: &SubscribersHandle, now: Instant) {
        for tap in &self.taps {
            let cur = handle.dropped(tap.slot);
            let prev = self.drop_seen.get(&tap.name).copied().unwrap_or(0);
            if cur <= prev {
                continue;
            }
            let allow = self
                .drop_logged_at
                .get(&tap.name)
                .map(|t| now.duration_since(*t) >= DROP_LOG_INTERVAL)
                .unwrap_or(true);
            if allow {
                let delta = cur - prev;
                self.log.push(format!(
                    "tap `{}` (slot {}): {delta} dropped block(s) (total {cur})",
                    tap.name, tap.slot
                ));
                self.drop_logged_at.insert(tap.name.clone(), now);
                self.drop_seen.insert(tap.name.clone(), cur);
            }
        }
    }

    /// Capture exactly one summed-magnitude heatmap frame.
    pub fn pump_heatmap(&mut self, handle: &SubscribersHandle) {
        let fft_size = self.spectrum_opts.resolve_fft_size();
        let bins = spectrum_bin_count(fft_size);
        if self.heatmap_bins != bins {
            self.heatmap_history.clear();
            self.heatmap_bins = bins;
        }
        let spectrum_taps: Vec<&TapEntry> = self
            .taps
            .iter()
            .filter(|t| t.has(TapType::Spectrum))
            .collect();
        if spectrum_taps.is_empty() {
            return;
        }
        let mut frame_sum: Vec<f32> = vec![0.0; bins];
        for tap in &spectrum_taps {
            let _ = handle.read_spectrum_into_with(
                tap.slot,
                self.spectrum_opts,
                &mut self.spectrum_scratch,
            );
            let n = self.spectrum_scratch.len().min(bins);
            for (dst, src) in frame_sum
                .iter_mut()
                .zip(self.spectrum_scratch.iter())
                .take(n)
            {
                *dst += *src;
            }
        }
        self.heatmap_history.push_back(frame_sum);
        if self.heatmap_history.len() > HEATMAP_HISTORY_CAP {
            self.heatmap_history.pop_front();
        }
    }

    /// Drain `trigger_led` fires from the observer and stamp the UI-side
    /// flash time.
    pub fn pump_leds(&mut self, handle: &SubscribersHandle) {
        let now = Instant::now();
        for tap in &self.taps {
            if tap.has(TapType::TriggerLed) && handle.take_trigger(tap.slot) {
                self.trigger_flash_at.insert(tap.name.clone(), now);
            }
        }
    }

    pub fn toggle_record_mute(&mut self) {
        match (&self.record.record_path, &self.record.muted) {
            (Some(_), Some(flag)) => {
                let new = !flag.load(Ordering::Relaxed);
                flag.store(new, Ordering::Relaxed);
                self.log.push(if new { "recording: muted" } else { "recording: unmuted" });
            }
            _ => {
                self.log.push("recording: no record path; pass --record <path> to enable");
            }
        }
    }
}

