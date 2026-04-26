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

use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

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

use patches_observation::processor::ProcessorId;
use patches_observation::subscribers::SubscribersHandle;

/// Conventional dBFS thresholds for meter colouring.
pub const DB_AMBER_FLOOR: f32 = -18.0;
pub const DB_RED_FLOOR: f32 = -6.0;
/// Lowest dBFS rendered as a non-empty bar. Below this, bars are empty.
pub const DB_FLOOR: f32 = -60.0;

/// Bounded ring of event-log lines.
pub struct EventLog {
    lines: VecDeque<String>,
    cap: usize,
}

impl EventLog {
    pub fn new(cap: usize) -> Self {
        Self { lines: VecDeque::with_capacity(cap), cap }
    }

    pub fn push(&mut self, msg: impl Into<String>) {
        if self.lines.len() == self.cap {
            self.lines.pop_front();
        }
        self.lines.push_back(msg.into());
    }
}

/// One declared meter tap (name, slot index in the observation surface).
#[derive(Clone, Debug)]
pub struct TapEntry {
    pub name: String,
    pub slot: usize,
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

/// Mutable view state shared between the input loop and the draw loop.
pub struct View {
    pub header: HeaderInfo,
    pub taps: Vec<TapEntry>,
    pub log: EventLog,
    pub record: RecordState,
    pub engine_state: EngineState,
    /// Horizontal scroll offset (in tap columns) for the meter pane.
    pub meter_scroll: usize,
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

/// Per-bar geometry: 2 cells wide + 1 cell gap. Two label rows underneath.
const BAR_WIDTH: u16 = 2;
const BAR_GAP: u16 = 1;
const COL_WIDTH: u16 = BAR_WIDTH + BAR_GAP;
const LABEL_ROWS: u16 = 2;

/// Number of meter columns visible at the given pane width. At least 1.
fn visible_cols(pane_width: u16) -> usize {
    ((pane_width / COL_WIDTH).max(1)) as usize
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

    if view.taps.is_empty() || inner.height < LABEL_ROWS + 1 {
        let p = Paragraph::new("(no meter taps declared)");
        f.render_widget(p, inner);
        return;
    }

    let bar_height = (inner.height - LABEL_ROWS) as usize;
    let visible = visible_cols(inner.width);
    let max_scroll = view.taps.len().saturating_sub(visible);
    let scroll = view.meter_scroll.min(max_scroll);

    let buf = f.buffer_mut();

    for (col_idx, tap_idx) in (scroll..scroll + visible).enumerate() {
        if tap_idx >= view.taps.len() {
            break;
        }
        let tap = &view.taps[tap_idx];
        let col_x = inner.x + col_idx as u16 * COL_WIDTH;

        let peak_db = amp_to_db(handle.read(tap.slot, ProcessorId::MeterPeak));
        let rms_db = amp_to_db(handle.read(tap.slot, ProcessorId::MeterRms));
        let peak_h = (db_to_ratio(peak_db) * bar_height as f64).round() as usize;
        let rms_h = (db_to_ratio(rms_db) * bar_height as f64).round() as usize;
        let peak_color = db_colour(peak_db);
        let rms_color = db_colour(rms_db);

        for row in 0..bar_height {
            let from_bottom = row + 1;
            let y = inner.y + (bar_height - 1 - row) as u16;
            let (ch, color) = if from_bottom <= rms_h {
                ('█', rms_color)
            } else if from_bottom <= peak_h {
                ('▒', peak_color)
            } else {
                (' ', Color::DarkGray)
            };
            for bx in 0..BAR_WIDTH {
                if let Some(cell) = buf.cell_mut((col_x + bx, y)) {
                    cell.set_char(ch).set_style(Style::default().fg(color));
                }
            }
        }

        let label_y = inner.y + (bar_height as u16);
        let max_label = COL_WIDTH as usize;
        let label: String = tap.name.chars().take(max_label).collect();
        buf.set_string(col_x, label_y, &label, Style::default());

        let drops = handle.dropped(tap.slot);
        if drops > 0 {
            let s = format!("d{drops}");
            let s: String = s.chars().take(max_label).collect();
            buf.set_string(col_x, label_y + 1, &s, Style::default().fg(Color::Magenta));
        }
    }

    // Scroll indicator at the bottom-right of the pane.
    if max_scroll > 0 {
        let s = format!("[{}/{}]", scroll + 1, view.taps.len());
        let x = inner.x + inner.width.saturating_sub(s.len() as u16);
        let y = inner.y + inner.height - 1;
        buf.set_string(x, y, &s, Style::default().fg(Color::DarkGray));
    }
}

fn draw_log(f: &mut Frame, area: Rect, view: &View) {
    let block = Block::default().borders(Borders::ALL).title("events");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    let lines: Vec<Line> = view
        .log
        .lines
        .iter()
        .rev()
        .take(height)
        .rev()
        .map(|s| Line::from(s.as_str()))
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_footer(f: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quit  "),
        Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" record mute  "),
        Span::styled("←/→", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" scroll meters"),
    ]);
    let p = Paragraph::new(line).block(Block::default().borders(Borders::TOP));
    f.render_widget(p, area);
}

fn draw(f: &mut Frame, view: &View, handle: &SubscribersHandle) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(8),
            Constraint::Length(2),
        ])
        .split(area);
    draw_header(f, chunks[0], view);
    draw_meters(f, chunks[1], view, handle);
    draw_log(f, chunks[2], view);
    draw_footer(f, chunks[3]);
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

        terminal.draw(|f| draw(f, view, handle))?;

        let elapsed = last_frame.elapsed();
        let timeout = FRAME_INTERVAL.saturating_sub(elapsed);
        if event::poll(timeout)? {
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Release {
                    match k.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(LoopOutcome::Quit),
                        KeyCode::Char('r') => view.toggle_record_mute(),
                        KeyCode::Left | KeyCode::Char('h') => {
                            view.meter_scroll = view.meter_scroll.saturating_sub(1);
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            view.meter_scroll = view.meter_scroll.saturating_add(1);
                        }
                        _ => {}
                    }
                }
            }
        }
        last_frame = Instant::now();
    }
}
