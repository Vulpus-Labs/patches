//! Ratatui frontend for `patch_player` (ticket 0704, ADR 0055 §5).
//!
//! Layout:
//! - Header: patch path / sample rate / oversampling / engine state.
//! - Meter pane: one peak+RMS bar pair per declared meter tap, dB-coloured.
//! - Event log pane: scrolling log; halt + reload outcomes routed here.
//! - Footer: keybindings.
//!
//! In ticket 0704 the meter pane is fed by a fake publisher thread (see
//! `fake_publisher`). Ticket 0705 swaps the publisher for the live
//! engine→observer plumbing — the `View` shape doesn't change.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};

use patches_dsl::manifest::{Manifest, TapType};
use patches_observation::processor::{
    spectrum_bin_count, ProcessorId, ScopeReadOpts, SpectrumReadOpts, SCOPE_RING_SAMPLES,
    SCOPE_WINDOW_DEFAULT, SPECTRUM_FFT_SIZES, SPECTRUM_FFT_SIZE_DEFAULT, SPECTRUM_FFT_SIZE_MAX,
};
use patches_observation::subscribers::{Diagnostic, SubscribersHandle};
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};
use ratatui::widgets::Tabs;

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

/// Conventional dBFS thresholds for meter colouring.
pub const DB_AMBER_FLOOR: f32 = -18.0;
pub const DB_RED_FLOOR: f32 = -6.0;
/// Lowest dBFS rendered as a non-empty bar. Below this, bars are empty.
pub const DB_FLOOR: f32 = -60.0;

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
    lines: VecDeque<LogEntry>,
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
            // Flush current, then hard-split the long word.
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
}

/// Spectrum-tab render mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpectrumMode {
    /// One overlaid colour-coded curve per tap.
    Curves,
    /// Rolling waterfall of summed magnitudes across all spectrum taps.
    /// Misses inter-tap phase cancellation (sum is on |X|, not X).
    Heatmap,
}

impl Tab {
    pub fn next(self) -> Self {
        match self {
            Tab::Meters => Tab::Spectrum,
            Tab::Spectrum => Tab::Scope,
            Tab::Scope => Tab::Meters,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Tab::Meters => "meters",
            Tab::Spectrum => "spectrum",
            Tab::Scope => "scope",
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

/// Exponential-smoothing weight for spectrum curve magnitudes. Larger
/// = stickier (less shimmer, slower to react). 0.7 keeps a recognisable
/// peak for ~10 frames at 30 Hz.
pub const SPECTRUM_SMOOTH_ALPHA: f32 = 0.7;

/// Mutable view state shared between the input loop and the draw loop.
pub struct View {
    pub header: HeaderInfo,
    pub taps: Vec<TapEntry>,
    pub log: EventLog,
    pub record: RecordState,
    pub engine_state: EngineState,
    /// Vertical scroll offset (first visible tap row) for the meter
    /// pane. Ticket 0708: meters render one tap per row, so scroll is
    /// vertical.
    pub meter_scroll: usize,
    /// Event-log scroll offset, in entries from newest. 0 = pinned to
    /// the latest line; increments scroll back into history.
    pub log_scroll: usize,
    /// Active tab (ticket 0709).
    pub tab: Tab,
    /// Reusable scratch buffer for spectrum reads (TUI thread only).
    /// Pre-allocated to avoid allocating on every redraw.
    spectrum_scratch: Vec<f32>,
    /// Reusable scratch buffer for scope reads (TUI thread only).
    scope_scratch: Vec<f32>,
    /// Display params sent from UI to spectrum processors at read time.
    pub spectrum_opts: SpectrumReadOpts,
    /// Snap scope display to the first rising zero-crossing of the
    /// first scope tap, so the left edge of the trace is phase-stable.
    /// All taps shift by the same amount to stay phase-locked. The
    /// snap is sub-sample precise (linear interp between the two
    /// straddling samples).
    pub scope_snap_zero: bool,
    /// Per-tap exponentially-smoothed spectrum magnitudes. Keyed by
    /// tap name so a rename starts fresh. Cleared when the bin layout
    /// changes (FFT size). Used by the curve view; the heatmap stays
    /// instantaneous (smoothing across time would defeat the waterfall).
    spectrum_smoothed: HashMap<String, Vec<f32>>,
    /// User-controlled scope time-window in milliseconds. The actual
    /// `ScopeReadOpts` (decimation + window_samples) is derived from
    /// this plus the pane width and sample rate at draw time.
    pub scope_window_ms: f32,
    /// Spectrum tab render mode.
    pub spectrum_mode: SpectrumMode,
    /// Rolling history of summed-magnitude frames for the heatmap mode.
    /// Newest frame at the back. Cleared when the bin layout changes
    /// (FFT size). Capped at [`HEATMAP_HISTORY_CAP`] so resizing the
    /// pane doesn't lose history (only the latest pane-width slice is
    /// rendered).
    heatmap_history: VecDeque<Vec<f32>>,
    /// Bin count of the frames currently in `heatmap_history`. Mismatch
    /// with the live FFT size triggers a clear so old / new frames don't
    /// stack with mis-aligned bins.
    heatmap_bins: usize,
    /// Last observed drop counter per tap *name*; used to detect
    /// advances. Keyed by name (not slot) so a tap removed and re-added
    /// under the same slot does not inherit a stale baseline (ticket
    /// 0707).
    drop_seen: HashMap<String, u64>,
    /// Last time a drop-count line was logged per tap name (rate-limit).
    drop_logged_at: HashMap<String, Instant>,
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
        }
    }

    /// Replace the active tap list (e.g. on patch reload). Drop-counter
    /// baselines for slots that survive the change are preserved so a
    /// reload doesn't generate spurious "drops" log lines.
    pub fn set_taps(&mut self, taps: Vec<TapEntry>) {
        let surviving: std::collections::HashSet<&str> =
            taps.iter().map(|t| t.name.as_str()).collect();
        self.drop_seen.retain(|name, _| surviving.contains(name.as_str()));
        self.drop_logged_at.retain(|name, _| surviving.contains(name.as_str()));
        let max_scroll = taps.len().saturating_sub(1);
        if self.meter_scroll > max_scroll {
            self.meter_scroll = max_scroll;
        }
        self.taps = taps;
    }

    /// Seed drop baselines for newly appearing tap names from the
    /// current ring drop counters, so a fresh appearance (or a name
    /// that landed on a slot whose previous occupant left a non-zero
    /// drop count) does not produce a spurious "drops" line on the
    /// next poll. Idempotent — does nothing for names already tracked.
    pub fn seed_drop_baselines(&mut self, handle: &SubscribersHandle) {
        for tap in &self.taps {
            self.drop_seen
                .entry(tap.name.clone())
                .or_insert_with(|| handle.dropped(tap.slot));
        }
    }

    /// Surface advancing per-slot drop counters as event-log lines,
    /// rate-limited per slot. Slot → tap-name resolution uses the
    /// current tap list (the latest manifest snapshot).
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

    /// Capture exactly one summed-magnitude heatmap frame. Called once
    /// per draw cycle (from `run`), so column-shift cadence == draw
    /// cadence — no aliasing between two independent clocks. Runs
    /// regardless of which tab is active so the heatmap shows live data
    /// the instant the user switches to it.
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

/// Linear amplitude → dBFS, clamped at `DB_FLOOR`.
fn amp_to_db(amp: f32) -> f32 {
    if amp <= 0.0 {
        return DB_FLOOR;
    }
    let db = 20.0 * amp.log10();
    db.max(DB_FLOOR)
}

/// dBFS → ratio in `[0.0, 1.0]` for the gauge.
fn db_to_ratio(db: f32) -> f64 {
    let clamped = db.clamp(DB_FLOOR, 0.0);
    ((clamped - DB_FLOOR) / -DB_FLOOR) as f64
}

fn db_colour(db: f32) -> Color {
    if db >= DB_RED_FLOOR {
        Color::Red
    } else if db >= DB_AMBER_FLOOR {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn draw_header(f: &mut Frame, area: Rect, view: &View) {
    let state = match view.engine_state {
        EngineState::Running => Span::styled("running", Style::default().fg(Color::Green)),
        EngineState::Halted => Span::styled("halted", Style::default().fg(Color::Red)),
    };
    let rec = match (&view.record.record_path, &view.record.muted) {
        (Some(p), Some(flag)) => {
            let muted = flag.load(Ordering::Relaxed);
            let style = if muted {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            };
            Span::styled(
                if muted { format!("REC MUTED → {p}") } else { format!("● REC → {p}") },
                style,
            )
        }
        _ => Span::raw("rec off"),
    };
    let line = Line::from(vec![
        Span::raw(format!("{}  ", view.header.patch_path)),
        Span::styled(format!("{} Hz  ", view.header.sample_rate), Style::default().fg(Color::Cyan)),
        Span::raw(format!("OS×{}  ", view.header.oversampling)),
        state,
        Span::raw("  "),
        rec,
    ]);
    let p = Paragraph::new(line).block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(p, area);
}

/// Horizontal-meter row geometry (ticket 0708).
///
/// Each tap occupies one row: `name | bar | peak_dB | rms_dB`. The
/// name column is fixed-width and truncates with an ellipsis; the bar
/// fills the remaining space; the dB readouts are right-aligned and
/// fixed-width. RMS is rendered as a filled bar (sub-cell precision
/// via Unicode 1/8th-block characters) and peak is overlaid as a
/// single trailing tick (`│`) at the peak position so both are
/// visible simultaneously.
const NAME_W: u16 = 16;
const METRIC_W: u16 = 8; // " -60.0dB"
const GAP_W: u16 = 1;
const MIN_BAR_W: u16 = 8;

/// Number of meter rows visible in the given pane height. At least 1.
fn visible_rows(pane_height: u16) -> usize {
    (pane_height.max(1)) as usize
}

/// Stippled fill: braille glyphs at 8 progressive densities (1/8..8/8).
/// Dots accrue in interleaved column/row order so the glyph reads as a
/// growing stipple rather than a sliding solid bar. U+2800 + mask, where
/// mask bits are the standard braille dot positions.
const STIPPLE_BRAILLE: [char; 8] = [
    '\u{2801}', // 0x01            : TL
    '\u{2809}', // 0x01|0x08       : + TR
    '\u{280B}', // |0x02            : + 2nd-L
    '\u{281B}', // |0x10            : + 2nd-R
    '\u{281F}', // |0x04            : + 3rd-L
    '\u{283F}', // |0x20            : + 3rd-R
    '\u{287F}', // |0x40            : + BL
    '\u{28FF}', // |0x80            : full
];

/// Truncate `s` to `max_chars`, replacing the last visible char with
/// `…` if truncated. Width 0 returns empty; width 1 returns first char.
fn truncate_name(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let mut out: String = s.chars().take(max_chars - 1).collect();
    out.push('…');
    out
}

fn draw_meters(
    f: &mut Frame,
    area: Rect,
    view: &View,
    handle: &SubscribersHandle,
) {
    let block = Block::default().borders(Borders::ALL).title("meters");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let meter_taps: Vec<&TapEntry> = view
        .taps
        .iter()
        .filter(|t| t.has(TapType::Meter))
        .collect();

    if meter_taps.is_empty() {
        let p = Paragraph::new("(no meter taps declared)");
        f.render_widget(p, inner);
        return;
    }

    // Reserve space for two metric columns (peak, rms) with gaps.
    let metrics_total = METRIC_W * 2 + GAP_W;
    let min_required = NAME_W + GAP_W + MIN_BAR_W + GAP_W + metrics_total;
    if inner.width < min_required || inner.height < 1 {
        let p = Paragraph::new("(meter pane too narrow)");
        f.render_widget(p, inner);
        return;
    }

    let bar_w = inner.width - NAME_W - GAP_W - GAP_W - metrics_total;
    let bar_x = inner.x + NAME_W + GAP_W;
    let metrics_x = bar_x + bar_w + GAP_W;

    let visible = visible_rows(inner.height);
    let max_scroll = meter_taps.len().saturating_sub(visible);
    let scroll = view.meter_scroll.min(max_scroll);

    let buf = f.buffer_mut();

    for (row_idx, tap_idx) in (scroll..scroll + visible).enumerate() {
        if tap_idx >= meter_taps.len() {
            break;
        }
        let tap = meter_taps[tap_idx];
        let y = inner.y + row_idx as u16;

        // Name (truncated).
        let name = truncate_name(&tap.name, NAME_W as usize);
        buf.set_string(inner.x, y, &name, Style::default());

        // Bar with sub-cell RMS fill and peak tick overlay.
        let peak_db = amp_to_db(handle.read(tap.slot, ProcessorId::MeterPeak));
        let rms_db = amp_to_db(handle.read(tap.slot, ProcessorId::MeterRms));
        let bar_cells_f = bar_w as f64;
        let rms_eighths = (db_to_ratio(rms_db) * bar_cells_f * 8.0).round() as usize;
        let peak_cell = (db_to_ratio(peak_db) * bar_cells_f).round() as usize;
        let rms_color = db_colour(rms_db);
        let peak_color = db_colour(peak_db);

        for cell in 0..bar_w as usize {
            let x = bar_x + cell as u16;
            let cell_eighths_filled =
                rms_eighths.saturating_sub(cell * 8).min(8);
            let (ch, color) = if cell_eighths_filled > 0 {
                (STIPPLE_BRAILLE[cell_eighths_filled - 1], rms_color)
            } else {
                ('·', Color::DarkGray)
            };
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_char(ch).set_style(Style::default().fg(color));
            }
        }
        // Peak tick. Draw only if it lies past the RMS fill so it
        // remains visible; otherwise the hatch already conveys the
        // signal level.
        let peak_cell_clamped = peak_cell.min(bar_w.saturating_sub(1) as usize);
        let rms_cells = rms_eighths / 8;
        if peak_cell_clamped >= rms_cells && peak_db > DB_FLOOR {
            let x = bar_x + peak_cell_clamped as u16;
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_char('│').set_style(Style::default().fg(peak_color));
            }
        }

        // dB readouts: peak then RMS, right-aligned within METRIC_W.
        let peak_str = format_db(peak_db);
        let rms_str = format_db(rms_db);
        buf.set_string(metrics_x, y, &peak_str, Style::default().fg(peak_color));
        buf.set_string(
            metrics_x + METRIC_W + GAP_W,
            y,
            &rms_str,
            Style::default().fg(rms_color),
        );

        // Drop indicator overlays the right edge of the name column.
        let drops = handle.dropped(tap.slot);
        if drops > 0 {
            let s = format!(" d{drops}");
            let max = (NAME_W as usize).saturating_sub(name.chars().count());
            let s: String = s.chars().take(max).collect();
            let xoff = inner.x + name.chars().count() as u16;
            buf.set_string(xoff, y, &s, Style::default().fg(Color::Magenta));
        }
    }

    // Scroll indicator at the bottom-right of the pane.
    if max_scroll > 0 {
        let s = format!("[{}-{}/{}]", scroll + 1, (scroll + visible).min(meter_taps.len()), meter_taps.len());
        let x = inner.x + inner.width.saturating_sub(s.len() as u16);
        let y = inner.y + inner.height - 1;
        buf.set_string(x, y, &s, Style::default().fg(Color::DarkGray));
    }
}

/// Draw the spectrum tab: log-frequency-x, dBFS-y EQ curves, one per
/// declared spectrum tap (ticket 0709). Uses ratatui's `Canvas` for
/// sub-cell-precision line plots.
///
/// The left edge of the x-axis is anchored at the first plottable bin
/// (`bin_hz = sr / FFT_SIZE`), not a hard 20 Hz floor — otherwise the
/// space between 20 Hz and the first bin (~47 Hz at 48 kHz / 1024)
/// shows as an empty strip.
const SPECTRUM_DB_MIN: f64 = -80.0;
const SPECTRUM_DB_MAX: f64 = 6.0;
const SPECTRUM_PALETTE: [Color; 6] = [
    Color::Cyan,
    Color::Yellow,
    Color::LightMagenta,
    Color::LightGreen,
    Color::White,
    Color::LightRed,
];

fn draw_spectrum(
    f: &mut Frame,
    area: Rect,
    view: &mut View,
    handle: &SubscribersHandle,
) {
    match view.spectrum_mode {
        SpectrumMode::Curves => draw_spectrum_curves(f, area, view, handle),
        SpectrumMode::Heatmap => draw_spectrum_heatmap(f, area, view, handle),
    }
}

fn draw_spectrum_curves(
    f: &mut Frame,
    area: Rect,
    view: &mut View,
    handle: &SubscribersHandle,
) {
    let fft_size = view.spectrum_opts.resolve_fft_size();
    let title = format!("spectrum  ({} bins, FFT {}) [curves]", spectrum_bin_count(fft_size), fft_size);
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let spectrum_taps: Vec<(usize, &TapEntry)> = view
        .taps
        .iter()
        .enumerate()
        .filter(|(_, t)| t.has(TapType::Spectrum))
        .collect();

    if spectrum_taps.is_empty() || inner.height < 3 || inner.width < 16 {
        let msg = if spectrum_taps.is_empty() {
            "(no spectrum taps declared — use ~spectrum(...))"
        } else {
            "(spectrum pane too small)"
        };
        f.render_widget(Paragraph::new(msg), inner);
        return;
    }

    let sr = view.header.sample_rate as f64;
    let nyq = sr * 0.5;
    let bin_hz = sr / fft_size as f64;
    let f_min = bin_hz;
    let f_max = nyq.max(f_min * 10.0);
    let log_min = f_min.log10();
    let log_max = f_max.log10();

    // Drop stale smoothed buffers if the bin count changed.
    let expected_bins = spectrum_bin_count(fft_size);
    view.spectrum_smoothed
        .retain(|_, v| v.len() == expected_bins);

    // Snapshot per-tap spectra into Vec<(name, color, mags)>. We
    // collect first to avoid borrowing `view` inside the canvas paint
    // closure (which captures by move). Per-tap exponential smoothing
    // (`SPECTRUM_SMOOTH_ALPHA`) damps frame-to-frame leakage shimmer
    // for tones that fall between bins.
    let mut series: Vec<(String, Color, Vec<f32>)> = Vec::with_capacity(spectrum_taps.len());
    for (i, (_, tap)) in spectrum_taps.iter().enumerate() {
        let color = SPECTRUM_PALETTE[i % SPECTRUM_PALETTE.len()];
        let _ok = handle.read_spectrum_into_with(tap.slot, view.spectrum_opts, &mut view.spectrum_scratch);
        let smoothed = view
            .spectrum_smoothed
            .entry(tap.name.clone())
            .or_insert_with(|| view.spectrum_scratch.clone());
        if smoothed.len() != view.spectrum_scratch.len() {
            *smoothed = view.spectrum_scratch.clone();
        } else {
            let a = SPECTRUM_SMOOTH_ALPHA;
            for (s, &x) in smoothed.iter_mut().zip(view.spectrum_scratch.iter()) {
                *s = a * *s + (1.0 - a) * x;
            }
        }
        series.push((tap.name.clone(), color, smoothed.clone()));
    }

    // Legend along the top of the pane.
    let legend_spans: Vec<Span> = series
        .iter()
        .flat_map(|(name, color, _)| {
            vec![
                Span::styled("● ", Style::default().fg(*color)),
                Span::raw(format!("{name}  ")),
            ]
        })
        .collect();
    let legend_area = Rect { x: inner.x, y: inner.y, width: inner.width, height: 1 };
    let plot_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: inner.height - 1,
    };
    f.render_widget(Paragraph::new(Line::from(legend_spans)), legend_area);

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([log_min, log_max])
        .y_bounds([SPECTRUM_DB_MIN, SPECTRUM_DB_MAX])
        .paint(move |ctx| {
            // Octave gridlines (vertical) at 100 Hz, 1 kHz, 10 kHz.
            for &gf in &[100.0_f64, 1_000.0, 10_000.0] {
                if gf <= f_max {
                    let x = gf.log10();
                    ctx.draw(&CanvasLine {
                        x1: x,
                        y1: SPECTRUM_DB_MIN,
                        x2: x,
                        y2: SPECTRUM_DB_MAX,
                        color: Color::DarkGray,
                    });
                }
            }
            // dB gridlines at -60, -40, -20, 0.
            for &gd in &[-60.0_f64, -40.0, -20.0, 0.0] {
                ctx.draw(&CanvasLine {
                    x1: log_min,
                    y1: gd,
                    x2: log_max,
                    y2: gd,
                    color: Color::DarkGray,
                });
            }

            for (_, color, mags) in &series {
                // Walk bins from 1 (skip DC) up to Nyquist; connect
                // consecutive (log_freq, dB) points. Bins below
                // SPECTRUM_F_MIN are clipped at the left edge.
                let mut prev: Option<(f64, f64)> = None;
                for (k, m) in mags.iter().enumerate().skip(1) {
                    let freq = bin_hz * k as f64;
                    if freq > f_max {
                        prev = None;
                        continue;
                    }
                    let x = freq.log10();
                    let db = if *m <= 0.0 {
                        SPECTRUM_DB_MIN
                    } else {
                        20.0 * (*m as f64).log10()
                    }
                    .clamp(SPECTRUM_DB_MIN, SPECTRUM_DB_MAX);
                    if let Some((px, py)) = prev {
                        ctx.draw(&CanvasLine {
                            x1: px,
                            y1: py,
                            x2: x,
                            y2: db,
                            color: *color,
                        });
                    }
                    prev = Some((x, db));
                }
            }
        });
    f.render_widget(canvas, plot_area);
}

/// Draw the spectrum tab in heatmap mode: rolling waterfall of summed
/// magnitudes across all spectrum taps. Newest column at the right.
///
/// Per-bin magnitudes are summed (not complex-summed), so coherent
/// cancellation between taps reads as constructive — this is a "where
/// is energy concentrated across the patch?" view, not a mix-bus
/// prediction. Y-axis is log-frequency (first plottable bin at bottom,
/// Nyquist at top), matching the curve view; rows are doubled with
/// half-block glyphs (▀) so each terminal row carries two frequency
/// cells (upper bg, lower fg). FFT-size changes rescale the axis
/// automatically via `bin_hz = sr / fft_size`.
fn draw_spectrum_heatmap(
    f: &mut Frame,
    area: Rect,
    view: &mut View,
    handle: &SubscribersHandle,
) {
    let fft_size = view.spectrum_opts.resolve_fft_size();
    let bins = spectrum_bin_count(fft_size);
    let title = format!("spectrum  ({bins} bins, FFT {fft_size}) [heatmap]");
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let spectrum_taps: Vec<&TapEntry> = view
        .taps
        .iter()
        .filter(|t| t.has(TapType::Spectrum))
        .collect();

    if spectrum_taps.is_empty() || inner.height < 2 || inner.width < 4 {
        let msg = if spectrum_taps.is_empty() {
            "(no spectrum taps declared — use ~spectrum(...))"
        } else {
            "(heatmap pane too small)"
        };
        f.render_widget(Paragraph::new(msg), inner);
        return;
    }

    // Frames are captured at fixed cadence by `pump_heatmap`; bin layout
    // checks live there. Here we just render the latest pane-width
    // slice of the rolling buffer.
    let _ = handle; // reads happen in pump
    let cap = inner.width as usize;

    // Log-frequency axis, matching the curve view. Rescales with FFT
    // size via `bin_hz`.
    let sr = view.header.sample_rate as f64;
    let nyq = sr * 0.5;
    let bin_hz = sr / fft_size as f64;
    let f_min = bin_hz;
    let f_max = nyq.max(f_min * 10.0);
    let log_min = f_min.log10();
    let log_max = f_max.log10();

    // Render: rightmost column = newest frame. Each terminal row holds
    // two frequency cells (upper half = bg, lower half = fg).
    let cell_rows = inner.height as usize;
    let freq_cells = cell_rows * 2;
    let buf = f.buffer_mut();
    let history_len = view.heatmap_history.len();
    // Take the latest `cap` frames; pad on the left if not enough yet.
    let visible = history_len.min(cap);
    let pad = cap - visible;
    let start = history_len - visible;
    for col in 0..cap {
        let frame = if col < pad {
            None
        } else {
            view.heatmap_history.get(start + (col - pad))
        };
        let x = inner.x + col as u16;
        for row in 0..cell_rows {
            let y = inner.y + (cell_rows - 1 - row) as u16;
            // Lower freq cell = lower terminal-row half (fg).
            let f_lo = row * 2;
            let f_hi = row * 2 + 1;
            let lo = sample_heat_log(frame, f_lo, freq_cells, bin_hz, log_min, log_max);
            let hi = sample_heat_log(frame, f_hi, freq_cells, bin_hz, log_min, log_max);
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_char('\u{2580}') // ▀
                    .set_style(Style::default().fg(heat_colour(hi)).bg(heat_colour(lo)));
            }
        }
    }
}

/// Sample the heatmap at log-frequency cell `f_idx` (0..freq_cells, low →
/// high). Cell bounds are `[log_min + f_idx*step, log_min + (f_idx+1)*step]`
/// in log10-Hz; converted to a bin range via `bin_hz`. Per cell takes
/// peak magnitude, then maps via dB to [0,1]. FFT-size changes rescale
/// the bin range each call.
fn sample_heat_log(
    frame: Option<&Vec<f32>>,
    f_idx: usize,
    freq_cells: usize,
    bin_hz: f64,
    log_min: f64,
    log_max: f64,
) -> f32 {
    let Some(frame) = frame else { return 0.0 };
    if freq_cells == 0 || frame.is_empty() {
        return 0.0;
    }
    let bins = frame.len();
    let step = (log_max - log_min) / freq_cells as f64;
    let lo_hz = 10f64.powf(log_min + f_idx as f64 * step);
    let hi_hz = 10f64.powf(log_min + (f_idx + 1) as f64 * step);
    // Bin = freq / bin_hz. Skip DC; clamp to last bin.
    let lo = ((lo_hz / bin_hz).floor() as usize).max(1).min(bins - 1);
    let hi = ((hi_hz / bin_hz).ceil() as usize).max(lo + 1).min(bins);
    let mut peak: f32 = 0.0;
    for &v in &frame[lo..hi] {
        if v > peak {
            peak = v;
        }
    }
    if peak <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * peak.log10();
    let lo_db = -80.0_f32;
    let hi_db = 6.0_f32;
    ((db - lo_db) / (hi_db - lo_db)).clamp(0.0, 1.0)
}

/// Map a [0,1] heat value to a colour. Black → cyan → yellow → red.
fn heat_colour(t: f32) -> Color {
    if t <= 0.001 {
        return Color::Black;
    }
    let lerp = |a: f32, b: f32, x: f32| a + (b - a) * x;
    let (r, g, b) = if t < 0.33 {
        let u = t / 0.33;
        (lerp(0.0, 0.0, u), lerp(64.0, 200.0, u), lerp(96.0, 220.0, u))
    } else if t < 0.66 {
        let u = (t - 0.33) / 0.33;
        (lerp(0.0, 240.0, u), lerp(200.0, 220.0, u), lerp(220.0, 60.0, u))
    } else {
        let u = (t - 0.66) / 0.34;
        (lerp(240.0, 255.0, u), lerp(220.0, 60.0, u), lerp(60.0, 30.0, u))
    };
    Color::Rgb(r as u8, g as u8, b as u8)
}

/// Draw the oscilloscope tab. All scope taps render as overlaid
/// waveforms on a single canvas, sharing an x-axis time origin
/// (decimation is anchored to the global `sample_time` grid in the
/// processor, so taps stay phase-locked).
const SCOPE_Y_MIN: f64 = -1.1;
const SCOPE_Y_MAX: f64 = 1.1;
const SCOPE_PALETTE: [Color; 6] = [
    Color::Cyan,
    Color::Yellow,
    Color::LightMagenta,
    Color::LightGreen,
    Color::White,
    Color::LightRed,
];

/// Sub-sample index of the first rising zero-crossing
/// (`prev <= 0 && cur > 0`), via linear interpolation between the two
/// straddling samples. `None` if no such crossing exists.
fn first_rising_zero_crossing_frac(samples: &[f32]) -> Option<f32> {
    for (i, pair) in samples.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        if a <= 0.0 && b > 0.0 {
            // Crossing at i + a / (a - b) (b > a since b > 0 >= a).
            return Some(i as f32 + a / (a - b));
        }
    }
    None
}

/// Derive `ScopeReadOpts` from a user-facing window length in ms.
/// We aim for `decimation = 1` wherever the raw sample count fits a
/// generous display cap ([`SCOPE_DISPLAY_CAP`]), so each cycle of a
/// periodic signal is drawn from many sample points and inter-sample
/// phase drift across cycles isn't visible. Above the cap, decimation
/// scales up to keep the line-segment count bounded.
///
/// `pane_w` is accepted for API stability but unused — Canvas-rendered
/// line segments compress fine into any pane width, and the visual
/// fidelity issue (varying-looking wavelength on a steady tone) is
/// dominated by sample-density-per-cycle, not pane-width matching.
pub fn scope_opts_for_window_ms(
    window_ms: f32,
    sample_rate: u32,
    _pane_w: u16,
) -> ScopeReadOpts {
    let raw = (window_ms.max(0.1) as f64 * sample_rate as f64 / 1000.0).round() as usize;
    let raw = raw.max(2).min(SCOPE_RING_SAMPLES);
    let dec = raw.div_ceil(SCOPE_DISPLAY_CAP).max(1);
    let win = (raw / dec).max(2);
    ScopeReadOpts { decimation: dec, window_samples: win }
}

/// Display-sample target ceiling. Below this, decimation = 1 (raw
/// samples) so each cycle of a periodic input is drawn from many
/// closely-spaced line segments. Above this, decimation scales up.
pub const SCOPE_DISPLAY_CAP: usize = 4096;

fn draw_scope(
    f: &mut Frame,
    area: Rect,
    view: &mut View,
    handle: &SubscribersHandle,
) {
    let block = Block::default().borders(Borders::ALL).title("scope");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let scope_taps: Vec<&TapEntry> = view
        .taps
        .iter()
        .filter(|t| t.has(TapType::Osc))
        .collect();

    if scope_taps.is_empty() || inner.height < 3 || inner.width < 16 {
        let msg = if scope_taps.is_empty() {
            "(no scope taps declared — use ~osc(...))"
        } else {
            "(scope pane too small)"
        };
        f.render_widget(Paragraph::new(msg), inner);
        return;
    }

    let scope_opts = scope_opts_for_window_ms(
        view.scope_window_ms,
        view.header.sample_rate,
        inner.width,
    );
    let snap = if view.scope_snap_zero { ", snap↗0" } else { "" };
    let title_str = format!(
        "scope  ({:.1} ms, ×{} dec, {} samp{snap})",
        view.scope_window_ms, scope_opts.decimation, scope_opts.window_samples
    );
    f.render_widget(
        Block::default().borders(Borders::ALL).title(title_str),
        area,
    );

    // Snapshot per-tap waveforms before entering the canvas closure
    // (which moves its captures). `scope_scratch` is reused per read.
    let mut series: Vec<(String, Color, Vec<f32>)> = Vec::with_capacity(scope_taps.len());
    for (i, tap) in scope_taps.iter().enumerate() {
        let color = SCOPE_PALETTE[i % SCOPE_PALETTE.len()];
        let _ok = handle.read_scope_into_with(tap.slot, scope_opts, &mut view.scope_scratch);
        series.push((tap.name.clone(), color, view.scope_scratch.clone()));
    }

    // Snap-to-zero: sub-sample fractional shift so the first rising
    // zero-crossing of the first tap lands at x=0. All taps share the
    // shift (decimation grid is sample-time aligned across taps).
    let mut x_shift: f32 = 0.0;
    if view.scope_snap_zero {
        if let Some(first) = series.first().map(|(_, _, v)| v.clone()) {
            if let Some(c) = first_rising_zero_crossing_frac(&first) {
                let int_skip = c.floor() as usize;
                x_shift = c - int_skip as f32;
                for (_, _, v) in series.iter_mut() {
                    if int_skip < v.len() {
                        v.drain(..int_skip);
                    }
                }
            }
        }
    }
    let n = series
        .iter()
        .map(|(_, _, v)| v.len())
        .max()
        .unwrap_or(SCOPE_WINDOW_DEFAULT)
        .max(2) as f64;

    // Legend.
    let legend_spans: Vec<Span> = series
        .iter()
        .flat_map(|(name, color, _)| {
            vec![
                Span::styled("● ", Style::default().fg(*color)),
                Span::raw(format!("{name}  ")),
            ]
        })
        .collect();
    let legend_area = Rect { x: inner.x, y: inner.y, width: inner.width, height: 1 };
    let plot_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: inner.height - 1,
    };
    f.render_widget(Paragraph::new(Line::from(legend_spans)), legend_area);

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, n - 1.0])
        .y_bounds([SCOPE_Y_MIN, SCOPE_Y_MAX])
        .paint(move |ctx| {
            // Centre line and ±1.0 amplitude rails.
            for &gy in &[-1.0_f64, 0.0, 1.0] {
                ctx.draw(&CanvasLine {
                    x1: 0.0,
                    y1: gy,
                    x2: n - 1.0,
                    y2: gy,
                    color: Color::DarkGray,
                });
            }
            for (_, color, samples) in &series {
                let mut prev: Option<(f64, f64)> = None;
                for (i, s) in samples.iter().enumerate() {
                    let x = i as f64 - x_shift as f64;
                    let y = (*s as f64).clamp(SCOPE_Y_MIN, SCOPE_Y_MAX);
                    if let Some((px, py)) = prev {
                        ctx.draw(&CanvasLine {
                            x1: px,
                            y1: py,
                            x2: x,
                            y2: y,
                            color: *color,
                        });
                    }
                    prev = Some((x, y));
                }
            }
        });
    f.render_widget(canvas, plot_area);
}

/// Right-align dBFS into a fixed `METRIC_W`-wide string. Floor renders
/// as `   -inf` so the column stays aligned without flicker.
fn format_db(db: f32) -> String {
    let s = if db <= DB_FLOOR + 0.05 {
        "-inf dB".to_string()
    } else {
        format!("{db:>5.1}dB")
    };
    // Pad/truncate to METRIC_W chars.
    let w = METRIC_W as usize;
    if s.chars().count() >= w {
        s.chars().take(w).collect()
    } else {
        format!("{s:>w$}")
    }
}

fn draw_log(f: &mut Frame, area: Rect, view: &View) {
    let block = Block::default().borders(Borders::ALL).title("events");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let width = inner.width as usize;

    // Wrap newest-first into visual rows. log_scroll skips that many
    // newest rows; then we take `height` rows below the cut and reverse
    // for chronological display (newest at bottom).
    let mut wrapped: Vec<String> = Vec::new();
    let target = height.saturating_add(view.log_scroll);
    for entry in view.log.lines.iter().rev() {
        let prefix = format!("{} ", format_hms(entry.epoch_secs));
        let mut lines = wrap_with_prefix(&prefix, &entry.msg, width);
        lines.reverse();
        wrapped.extend(lines);
        if wrapped.len() >= target {
            break;
        }
    }
    let max_scroll = wrapped.len().saturating_sub(height);
    let scroll = view.log_scroll.min(max_scroll);
    let start = scroll;
    let end = (start + height).min(wrapped.len());
    let mut wrapped: Vec<String> = wrapped[start..end].to_vec();
    wrapped.reverse();

    let lines: Vec<Line> = wrapped.into_iter().map(Line::from).collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_footer(f: &mut Frame, area: Rect, _view: &View) {
    let line = Line::from(vec![
        Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quit  "),
        Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" rec mute  "),
        Span::styled("↑/↓", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" log scroll  "),
        Span::styled("j/k", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" meter scroll  "),
        Span::styled("1/2/Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" tab  "),
        Span::styled("f", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" fft  "),
        Span::styled("-/=", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" scope ms  "),
        Span::styled("m", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" spec mode  "),
        Span::styled("z", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" scope snap"),
    ]);
    let p = Paragraph::new(line).block(Block::default().borders(Borders::TOP));
    f.render_widget(p, area);
}

fn draw(f: &mut Frame, view: &mut View, handle: &SubscribersHandle) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(8),
            Constraint::Length(2),
        ])
        .split(area);
    draw_header(f, chunks[0], view);
    draw_tabs(f, chunks[1], view);
    match view.tab {
        Tab::Meters => draw_meters(f, chunks[2], view, handle),
        Tab::Spectrum => draw_spectrum(f, chunks[2], view, handle),
        Tab::Scope => draw_scope(f, chunks[2], view, handle),
    }
    draw_log(f, chunks[3], view);
    draw_footer(f, chunks[4], view);
}

fn draw_tabs(f: &mut Frame, area: Rect, view: &View) {
    let titles: Vec<&str> = [Tab::Meters, Tab::Spectrum, Tab::Scope]
        .iter()
        .map(|t| t.label())
        .collect();
    let active_index = match view.tab {
        Tab::Meters => 0,
        Tab::Spectrum => 1,
        Tab::Scope => 2,
    };
    let tabs = Tabs::new(titles)
        .select(active_index)
        .block(Block::default().borders(Borders::BOTTOM))
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, area);
}

/// Set up an alternate-screen ratatui terminal in raw mode.
pub fn enter_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

/// Restore the terminal to its pre-TUI state. Called on exit and on panic.
pub fn leave_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Outcome of the input/draw loop.
pub enum LoopOutcome {
    Quit,
}

/// Tick frequency for the redraw loop (~30 Hz).
pub const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Drive the TUI until the user quits or `external_quit` is set. The
/// caller owns reload polling and observer integration; both are pumped
/// once per frame via `on_tick`.
pub fn run<F: FnMut(&mut View)>(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    view: &mut View,
    handle: &SubscribersHandle,
    external_quit: &Arc<AtomicBool>,
    mut on_tick: F,
) -> io::Result<LoopOutcome> {
    let mut last_frame = Instant::now();
    loop {
        if external_quit.load(Ordering::Acquire) {
            return Ok(LoopOutcome::Quit);
        }

        on_tick(view);
        view.pump_heatmap(handle);

        terminal.draw(|f| draw(f, view, handle))?;
        // Note: draw takes &mut View to allow reusing `spectrum_scratch`.

        let elapsed = last_frame.elapsed();
        let timeout = FRAME_INTERVAL.saturating_sub(elapsed);
        if event::poll(timeout)? {
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Release {
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(LoopOutcome::Quit),
                        KeyCode::Char('r') => view.toggle_record_mute(),
                        KeyCode::Up => {
                            view.log_scroll = view.log_scroll.saturating_add(1);
                        }
                        KeyCode::Down => {
                            view.log_scroll = view.log_scroll.saturating_sub(1);
                        }
                        KeyCode::Char('k') => {
                            view.meter_scroll = view.meter_scroll.saturating_sub(1);
                        }
                        KeyCode::Char('j') => {
                            view.meter_scroll = view.meter_scroll.saturating_add(1);
                        }
                        KeyCode::Char('1') => view.tab = Tab::Meters,
                        KeyCode::Char('2') => view.tab = Tab::Spectrum,
                        KeyCode::Char('3') => view.tab = Tab::Scope,
                        KeyCode::Tab => view.tab = view.tab.next(),
                        KeyCode::Char('z') => {
                            view.scope_snap_zero = !view.scope_snap_zero;
                            view.log.push(format!(
                                "scope snap-to-zero = {}",
                                if view.scope_snap_zero { "on" } else { "off" }
                            ));
                        }
                        KeyCode::Char('m') => {
                            view.spectrum_mode = match view.spectrum_mode {
                                SpectrumMode::Curves => SpectrumMode::Heatmap,
                                SpectrumMode::Heatmap => SpectrumMode::Curves,
                            };
                            view.heatmap_history.clear();
                            view.log.push(format!(
                                "spectrum mode = {}",
                                match view.spectrum_mode {
                                    SpectrumMode::Curves => "curves",
                                    SpectrumMode::Heatmap => "heatmap",
                                }
                            ));
                        }
                        KeyCode::Char('f') => {
                            // Cycle FFT size through SPECTRUM_FFT_SIZES.
                            let cur = view.spectrum_opts.resolve_fft_size();
                            let i = SPECTRUM_FFT_SIZES.iter().position(|&n| n == cur).unwrap_or(0);
                            let next = SPECTRUM_FFT_SIZES[(i + 1) % SPECTRUM_FFT_SIZES.len()];
                            view.spectrum_opts.fft_size = next;
                            view.log.push(format!("spectrum FFT size = {next}"));
                        }
                        KeyCode::Char('-') => {
                            let max_ms =
                                SCOPE_RING_SAMPLES as f32 / view.header.sample_rate as f32 * 1000.0;
                            view.scope_window_ms =
                                (view.scope_window_ms * 0.5).clamp(1.0, max_ms);
                            view.log
                                .push(format!("scope window = {:.1} ms", view.scope_window_ms));
                        }
                        KeyCode::Char('=') => {
                            let max_ms =
                                SCOPE_RING_SAMPLES as f32 / view.header.sample_rate as f32 * 1000.0;
                            view.scope_window_ms =
                                (view.scope_window_ms * 2.0).clamp(1.0, max_ms);
                            view.log
                                .push(format!("scope window = {:.1} ms", view.scope_window_ms));
                        }
                        _ => {}
                    }
                }
            }
        }
        last_frame = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::provenance::Provenance;
    use patches_core::Span;
    use patches_dsl::manifest::{TapDescriptor, TapType};
    use patches_core::TapBlockFrame;
    use patches_observation::subscribers::Subscribers;
    use patches_observation::tap_ring;

    fn header() -> HeaderInfo {
        HeaderInfo {
            patch_path: "x.patches".into(),
            sample_rate: 48_000,
            oversampling: 1,
        }
    }

    fn record() -> RecordState {
        RecordState { record_path: None, muted: None }
    }

    fn desc(slot: usize, name: &str, comp: TapType) -> TapDescriptor {
        TapDescriptor {
            slot,
            name: name.into(),
            components: vec![comp],
            source: Provenance::root(Span::synthetic()),
        }
    }

    #[test]
    fn taps_from_manifest_sorts_by_slot() {
        let m = vec![
            desc(2, "b", TapType::Meter),
            desc(0, "a", TapType::Meter),
        ];
        let taps = taps_from_manifest(&m);
        assert_eq!(taps[0].slot, 0);
        assert_eq!(taps[0].name, "a");
        assert_eq!(taps[1].slot, 2);
    }

    #[test]
    fn format_diagnostic_renders_unsupported_component() {
        let d = Diagnostic::NotYetImplemented {
            slot: 3,
            tap_name: "scope".into(),
            component: TapType::Spectrum,
        };
        assert_eq!(
            format_diagnostic(&d),
            "tap `scope` (`spectrum`): not yet implemented"
        );
    }

    #[test]
    fn poll_drops_logs_advance_and_rate_limits_repeats() {
        // Real ring with capacity 1. Pushing twice increments drops.
        let (mut tx, _rx) = tap_ring(1);
        let (subs, _diag) = Subscribers::new(tx.shared(), 8);
        let handle = subs.handle();

        let mut view = View::new(
            header(),
            vec![TapEntry { name: "a".into(), slot: 0, components: vec![TapType::Meter] }],
            record(),
        );
        let t0 = Instant::now();
        view.poll_drops(&handle, t0);
        assert!(view.log.is_empty(), "no advance yet, no log");

        // Fill ring then overflow → per-slot drops increment.
        let frame = TapBlockFrame::zeroed();
        assert!(tx.try_push_frame(&frame));
        assert!(!tx.try_push_frame(&frame));
        assert!(handle.dropped(0) > 0);

        view.poll_drops(&handle, t0);
        assert_eq!(view.log.lines.len(), 1, "first advance logs");

        // Overflow again at the same `now` — rate-limited, no new line.
        assert!(!tx.try_push_frame(&frame));
        view.poll_drops(&handle, t0);
        assert_eq!(view.log.lines.len(), 1, "rate-limited, still one line");

        // Past the interval — new advance logs.
        let later = t0 + DROP_LOG_INTERVAL + Duration::from_millis(1);
        assert!(!tx.try_push_frame(&frame));
        view.poll_drops(&handle, later);
        assert_eq!(view.log.lines.len(), 2, "second advance logs after interval");
    }

    #[test]
    fn set_taps_clamps_meter_scroll_and_keeps_baseline_for_surviving_names() {
        let mut view = View::new(
            header(),
            vec![
                TapEntry { name: "a".into(), slot: 0, components: vec![TapType::Meter] },
                TapEntry { name: "b".into(), slot: 1, components: vec![TapType::Meter] },
                TapEntry { name: "c".into(), slot: 2, components: vec![TapType::Meter] },
            ],
            record(),
        );
        view.meter_scroll = 2;
        view.drop_seen.insert("a".into(), 7);
        view.drop_seen.insert("b".into(), 11);

        view.set_taps(vec![TapEntry { name: "a".into(), slot: 0, components: vec![TapType::Meter] }]);
        assert_eq!(view.taps.len(), 1);
        assert_eq!(view.meter_scroll, 0);
        assert_eq!(view.drop_seen.get("a").copied(), Some(7));
        assert!(!view.drop_seen.contains_key("b"));
    }

    #[test]
    fn rename_under_same_slot_is_treated_as_fresh() {
        // Tap "a" had baseline 7 at slot 0. Replan renames slot 0 to "z".
        // The new name has no baseline; first poll seeds from current
        // ring counter (0 in this test), so no spurious drops are
        // logged.
        let (_tx, _rx) = tap_ring(4);
        let (subs, _diag) = Subscribers::new(_tx.shared(), 8);
        let handle = subs.handle();

        let mut view = View::new(
            header(),
            vec![TapEntry { name: "a".into(), slot: 0, components: vec![TapType::Meter] }],
            record(),
        );
        view.drop_seen.insert("a".into(), 7);

        // Rename: same slot, new name.
        view.set_taps(vec![TapEntry { name: "z".into(), slot: 0, components: vec![TapType::Meter] }]);
        view.seed_drop_baselines(&handle);
        view.poll_drops(&handle, Instant::now());
        assert!(view.log.is_empty(), "rename should not log spurious drops");
        // Stale "a" baseline gone.
        assert!(!view.drop_seen.contains_key("a"));
    }

    #[test]
    fn truncate_name_short_pass_through() {
        assert_eq!(truncate_name("foo", 16), "foo");
    }

    #[test]
    fn truncate_name_exact_pass_through() {
        assert_eq!(truncate_name("0123456789abcdef", 16), "0123456789abcdef");
    }

    #[test]
    fn truncate_name_long_appends_ellipsis() {
        assert_eq!(truncate_name("0123456789abcdefghij", 16), "0123456789abcde…");
    }

    #[test]
    fn format_db_floor_is_inf() {
        let s = format_db(DB_FLOOR);
        assert_eq!(s.chars().count(), METRIC_W as usize);
        assert!(s.contains("-inf"));
    }

    #[test]
    fn format_db_normal_value_fits_metric_width() {
        let s = format_db(-12.3);
        assert_eq!(s.chars().count(), METRIC_W as usize);
    }

    #[test]
    fn visible_rows_at_least_one() {
        assert_eq!(visible_rows(0), 1);
        assert_eq!(visible_rows(1), 1);
        assert_eq!(visible_rows(20), 20);
    }

    #[test]
    fn meter_scroll_clamped_when_taps_shrink() {
        let mut view = View::new(
            header(),
            (0..8).map(|i| TapEntry { name: format!("t{i}"), slot: i, components: vec![TapType::Meter] }).collect(),
            record(),
        );
        view.meter_scroll = 7;
        view.set_taps(vec![TapEntry { name: "t0".into(), slot: 0, components: vec![TapType::Meter] }]);
        assert_eq!(view.meter_scroll, 0);
    }

    #[test]
    fn tab_cycles_meters_spectrum_scope_and_back() {
        let mut t = Tab::Meters;
        t = t.next();
        assert_eq!(t, Tab::Spectrum);
        t = t.next();
        assert_eq!(t, Tab::Scope);
        t = t.next();
        assert_eq!(t, Tab::Meters);
    }

    #[test]
    fn taps_from_manifest_carries_components() {
        let m = vec![
            desc(0, "a", TapType::Meter),
            desc(1, "b", TapType::Spectrum),
        ];
        let taps = taps_from_manifest(&m);
        assert!(taps[0].has(TapType::Meter));
        assert!(!taps[0].has(TapType::Spectrum));
        assert!(taps[1].has(TapType::Spectrum));
    }

    #[test]
    fn re_added_name_does_not_inherit_predecessors_count() {
        // Slot 0 sees drops=5 (from a previous tap "a" we'll model
        // by directly using a real ring). Now tap "b" appears at slot
        // 0. seed_drop_baselines should baseline "b" at the current
        // counter so the next poll doesn't log delta=5.
        let (mut tx, _rx) = tap_ring(1);
        let (subs, _diag) = Subscribers::new(tx.shared(), 8);
        let handle = subs.handle();

        // Drive the slot-0 drop counter up by overflowing the ring.
        let frame = patches_core::TapBlockFrame::zeroed();
        assert!(tx.try_push_frame(&frame));
        for _ in 0..5 {
            assert!(!tx.try_push_frame(&frame));
        }
        assert!(handle.dropped(0) >= 5);

        let mut view = View::new(
            header(),
            vec![TapEntry { name: "b".into(), slot: 0, components: vec![TapType::Meter] }],
            record(),
        );
        view.seed_drop_baselines(&handle);
        view.poll_drops(&handle, Instant::now());
        assert!(view.log.is_empty(), "fresh name should not inherit predecessor drops");
    }

    #[test]
    fn format_hms_zero_and_wraparound() {
        assert_eq!(format_hms(0), "00:00:00");
        assert_eq!(format_hms(3661), "01:01:01");
        assert_eq!(format_hms(86_400), "00:00:00");
        assert_eq!(format_hms(86_399), "23:59:59");
    }

    #[test]
    fn wrap_with_prefix_short_message_one_line() {
        let lines = wrap_with_prefix("12:34:56 ", "hi", 40);
        assert_eq!(lines, vec!["12:34:56 hi"]);
    }

    #[test]
    fn wrap_with_prefix_wraps_and_indents_continuation() {
        // prefix = 9 chars; width = 20 ⇒ avail = 11. Words pack across lines.
        let lines = wrap_with_prefix("12:34:56 ", "alpha bravo charlie delta", 20);
        assert!(lines.len() >= 2, "expected wrap, got {lines:?}");
        assert!(lines[0].starts_with("12:34:56 "));
        for cont in &lines[1..] {
            assert!(cont.starts_with("         "), "continuation not indented: {cont:?}");
        }
    }

    #[test]
    fn wrap_with_prefix_hard_splits_long_word() {
        let lines = wrap_with_prefix("> ", "abcdefghij", 6);
        // avail = 4; "abcdefghij" → "abcd","efgh","ij".
        assert_eq!(lines, vec!["> abcd", "  efgh", "  ij"]);
    }

    #[test]
    fn event_log_push_stamps_timestamp() {
        let mut log = EventLog::new(4);
        log.push_at(3661, "hello");
        let e = log.lines.front().unwrap();
        assert_eq!(e.epoch_secs, 3661);
        assert_eq!(e.msg, "hello");
    }
}
