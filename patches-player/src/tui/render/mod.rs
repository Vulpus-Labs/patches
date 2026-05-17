//! Pure ratatui draw routines (no input, no state mutation outside of
//! reusable scratch buffers on `View`).

mod scope;
mod spectrum;

use scope::draw_scope;
use spectrum::draw_spectrum;

use std::time::Instant;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Frame;
use std::sync::atomic::Ordering;

use patches_dsl::manifest::TapType;
use patches_observation::processor::ProcessorId;
use patches_observation::subscribers::SubscribersHandle;

use super::state::{format_hms, wrap_with_prefix, EngineState, Tab, TapEntry, View};

/// Conventional dBFS thresholds for meter colouring.
pub const DB_AMBER_FLOOR: f32 = -18.0;
pub const DB_RED_FLOOR: f32 = -6.0;
/// Lowest dBFS rendered as a non-empty bar. Below this, bars are empty.
pub const DB_FLOOR: f32 = -60.0;

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

const NAME_W: u16 = 16;
pub(crate) const METRIC_W: u16 = 8;
const GAP_W: u16 = 1;
const MIN_BAR_W: u16 = 8;
const LED_W: u16 = 4;
const TRIGGER_DECAY_MS: f32 = 140.0;
const LED_GAMMA: f32 = 2.4;
const GATE_RGB: (u8, u8, u8) = (64, 192, 96);
const TRIGGER_RGB: (u8, u8, u8) = (224, 160, 64);

fn led_color(base: (u8, u8, u8), intensity: f32) -> Color {
    let v = intensity.clamp(0.0, 1.0).powf(LED_GAMMA);
    Color::Rgb(
        (base.0 as f32 * v) as u8,
        (base.1 as f32 * v) as u8,
        (base.2 as f32 * v) as u8,
    )
}

fn trigger_intensity(age_ms: f32) -> f32 {
    if age_ms.is_nan() || age_ms < 0.0 {
        return 0.0;
    }
    let v = (-age_ms / TRIGGER_DECAY_MS).exp();
    if v < 0.001 { 0.0 } else { v }
}

/// Number of meter rows visible in the given pane height. At least 1.
pub(crate) fn visible_rows(pane_height: u16) -> usize {
    (pane_height.max(1)) as usize
}

const STIPPLE_BRAILLE: [char; 8] = [
    '\u{2801}', '\u{2809}', '\u{280B}', '\u{281B}', '\u{281F}', '\u{283F}', '\u{287F}', '\u{28FF}',
];

/// Truncate `s` to `max_chars`, replacing the last visible char with
/// `…` if truncated.
pub(crate) fn truncate_name(s: &str, max_chars: usize) -> String {
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

pub(crate) enum MeterRow<'a> {
    Mono(&'a TapEntry),
    StereoLeft { stem: &'a str, tap: &'a TapEntry },
    StereoRight(&'a TapEntry),
}

impl<'a> MeterRow<'a> {
    fn tap(&self) -> &'a TapEntry {
        match self {
            MeterRow::Mono(t) | MeterRow::StereoLeft { tap: t, .. } | MeterRow::StereoRight(t) => t,
        }
    }

    pub(crate) fn label(&self) -> String {
        match self {
            MeterRow::Mono(t) => t.name.clone(),
            MeterRow::StereoLeft { stem, .. } => format!("{stem} ▎L"),
            MeterRow::StereoRight(_) => "  ▎R".to_string(),
        }
    }
}

pub(crate) fn group_meter_rows<'a>(taps: &[&'a TapEntry]) -> Vec<MeterRow<'a>> {
    let mut out = Vec::with_capacity(taps.len());
    let mut i = 0;
    while i < taps.len() {
        let cur = taps[i];
        let stem_l = cur
            .name
            .strip_suffix("/left")
            .filter(|_| cur.has(TapType::StereoMeter));
        if let (Some(stem), Some(next)) = (stem_l, taps.get(i + 1)) {
            let pairs = next.has(TapType::StereoMeter)
                && next.name.strip_suffix("/right") == Some(stem)
                && next.slot == cur.slot + 1;
            if pairs {
                out.push(MeterRow::StereoLeft { stem, tap: cur });
                out.push(MeterRow::StereoRight(next));
                i += 2;
                continue;
            }
        }
        out.push(MeterRow::Mono(cur));
        i += 1;
    }
    out
}

fn draw_meters(f: &mut Frame, area: Rect, view: &View, handle: &SubscribersHandle) {
    let block = Block::default().borders(Borders::ALL).title("meters");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let meter_taps: Vec<&TapEntry> = view
        .taps
        .iter()
        .filter(|t| {
            t.has(TapType::Meter)
                || t.has(TapType::StereoMeter)
                || t.has(TapType::GateLed)
                || t.has(TapType::TriggerLed)
        })
        .collect();

    let meter_rows: Vec<MeterRow<'_>> = group_meter_rows(&meter_taps);

    if meter_rows.is_empty() {
        let p = Paragraph::new("(no meter taps declared)");
        f.render_widget(p, inner);
        return;
    }

    let metrics_total = METRIC_W * 2 + GAP_W;
    let min_required = NAME_W + GAP_W + MIN_BAR_W + GAP_W + metrics_total + GAP_W + LED_W;
    if inner.width < min_required || inner.height < 1 {
        let p = Paragraph::new("(meter pane too narrow)");
        f.render_widget(p, inner);
        return;
    }

    let bar_w = inner.width - NAME_W - GAP_W - GAP_W - metrics_total - GAP_W - LED_W;
    let bar_x = inner.x + NAME_W + GAP_W;
    let metrics_x = bar_x + bar_w + GAP_W;
    let led_x = metrics_x + METRIC_W + GAP_W + METRIC_W + GAP_W;
    let now = Instant::now();

    let visible = visible_rows(inner.height);
    let max_scroll = meter_rows.len().saturating_sub(visible);
    let scroll = view.meter_scroll.min(max_scroll);

    let buf = f.buffer_mut();

    for (row_idx, meter_row_idx) in (scroll..scroll + visible).enumerate() {
        if meter_row_idx >= meter_rows.len() {
            break;
        }
        let row = &meter_rows[meter_row_idx];
        let tap = row.tap();
        let y = inner.y + row_idx as u16;

        let label = row.label();
        let name = truncate_name(&label, NAME_W as usize);
        buf.set_string(inner.x, y, &name, Style::default());

        let has_meter = tap.has(TapType::Meter) || tap.has(TapType::StereoMeter);

        let peak_db = amp_to_db(handle.read(tap.slot, ProcessorId::MeterPeak));
        let rms_db = amp_to_db(handle.read(tap.slot, ProcessorId::MeterRms));
        let bar_cells_f = bar_w as f64;
        let rms_eighths = (db_to_ratio(rms_db) * bar_cells_f * 8.0).round() as usize;
        let peak_cell = (db_to_ratio(peak_db) * bar_cells_f).round() as usize;
        let rms_color = db_colour(rms_db);
        let peak_color = db_colour(peak_db);

        if has_meter {
            for cell in 0..bar_w as usize {
                let x = bar_x + cell as u16;
                let cell_eighths_filled = rms_eighths.saturating_sub(cell * 8).min(8);
                let (ch, color) = if cell_eighths_filled > 0 {
                    (STIPPLE_BRAILLE[cell_eighths_filled - 1], rms_color)
                } else {
                    ('·', Color::DarkGray)
                };
                if let Some(c) = buf.cell_mut((x, y)) {
                    c.set_char(ch).set_style(Style::default().fg(color));
                }
            }
            let peak_cell_clamped = peak_cell.min(bar_w.saturating_sub(1) as usize);
            let rms_cells = rms_eighths / 8;
            if peak_cell_clamped >= rms_cells && peak_db > DB_FLOOR {
                let x = bar_x + peak_cell_clamped as u16;
                if let Some(c) = buf.cell_mut((x, y)) {
                    c.set_char('│').set_style(Style::default().fg(peak_color));
                }
            }

            let peak_str = format_db(peak_db);
            let rms_str = format_db(rms_db);
            buf.set_string(metrics_x, y, &peak_str, Style::default().fg(peak_color));
            buf.set_string(
                metrics_x + METRIC_W + GAP_W,
                y,
                &rms_str,
                Style::default().fg(rms_color),
            );
        }

        if tap.has(TapType::GateLed) {
            let v = handle.read(tap.slot, ProcessorId::GateLed).clamp(0.0, 1.0);
            if let Some(c) = buf.cell_mut((led_x + 1, y)) {
                c.set_char('●').set_style(Style::default().fg(led_color(GATE_RGB, v)));
            }
        }
        if tap.has(TapType::TriggerLed) {
            let v = view
                .trigger_flash_at
                .get(&tap.name)
                .map(|t| trigger_intensity(now.duration_since(*t).as_secs_f32() * 1000.0))
                .unwrap_or(0.0);
            if let Some(c) = buf.cell_mut((led_x + 3, y)) {
                c.set_char('●').set_style(Style::default().fg(led_color(TRIGGER_RGB, v)));
            }
        }

        let drops = handle.dropped(tap.slot);
        if drops > 0 {
            let s = format!(" d{drops}");
            let max = (NAME_W as usize).saturating_sub(name.chars().count());
            let s: String = s.chars().take(max).collect();
            let xoff = inner.x + name.chars().count() as u16;
            buf.set_string(xoff, y, &s, Style::default().fg(Color::Magenta));
        }
    }

    if max_scroll > 0 {
        let s = format!("[{}-{}/{}]", scroll + 1, (scroll + visible).min(meter_rows.len()), meter_rows.len());
        let x = inner.x + inner.width.saturating_sub(s.len() as u16);
        let y = inner.y + inner.height - 1;
        buf.set_string(x, y, &s, Style::default().fg(Color::DarkGray));
    }
}
pub(crate) fn format_db(db: f32) -> String {
    let s = if db <= DB_FLOOR + 0.05 {
        "-inf dB".to_string()
    } else {
        format!("{db:>5.1}dB")
    };
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

fn draw_footer(f: &mut Frame, area: Rect, view: &View) {
    // While a bundle-dir prompt is open, the footer becomes the input
    // line. Trailing underscore marks the cursor position.
    if let Some(prompt) = view.prompt.as_ref() {
        let line = Line::from(vec![
            Span::styled(prompt.label(), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(": "),
            Span::raw(prompt.input.clone()),
            Span::raw("_"),
            Span::raw("    "),
            Span::styled("Enter", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" commit  "),
            Span::styled("Esc", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]);
        let p = Paragraph::new(line).block(Block::default().borders(Borders::TOP));
        f.render_widget(p, area);
        return;
    }
    let line = Line::from(vec![
        Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" quit  "),
        Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" rec mute  "),
        Span::styled("↑/↓", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" scroll  "),
        Span::styled("1-5/Tab", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" tab  "),
        Span::styled("f", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" fft  "),
        Span::styled("-/=", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" scope ms  "),
        Span::styled("m", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" spec mode  "),
        Span::styled("z", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" scope snap  "),
        Span::styled("b/B", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" bundle dir"),
    ]);
    let p = Paragraph::new(line).block(Block::default().borders(Borders::TOP));
    f.render_widget(p, area);
}

pub(crate) fn draw(f: &mut Frame, view: &mut View, handle: &SubscribersHandle) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(2),
        ])
        .split(area);
    draw_header(f, chunks[0], view);
    draw_tabs(f, chunks[1], view);
    match view.tab {
        Tab::Meters => draw_meters(f, chunks[2], view, handle),
        Tab::Spectrum => draw_spectrum(f, chunks[2], view, handle),
        Tab::Scope => draw_scope(f, chunks[2], view, handle),
        Tab::Events => draw_log(f, chunks[2], view),
        Tab::Cpu => draw_cpu(f, chunks[2], view),
    }
    draw_footer(f, chunks[3], view);
}

fn draw_cpu(f: &mut Frame, area: Rect, view: &View) {
    let snap = match view.cpu_snapshot.as_ref().and_then(|m| m.lock().ok().map(|g| g.clone())) {
        Some(s) => s,
        None => return,
    };
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(format!(
        "block: {:.1}% (samples {})  periodic: {:.1}%  window: {} blocks",
        snap.block_pct, snap.block_samples, snap.periodic_pct, snap.window_blocks,
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "{:<32}  {:<16}  {:>6}",
        "instance", "type", "% blk"
    )));
    for row in &snap.rows {
        lines.push(Line::from(format!(
            "{:<32}  {:<16}  {:>5.1}%",
            truncate_name(&row.name, 32),
            truncate_name(&row.type_name, 16),
            row.pct,
        )));
    }
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_tabs(f: &mut Frame, area: Rect, view: &View) {
    let mut tabs_present: Vec<Tab> =
        vec![Tab::Events, Tab::Meters, Tab::Spectrum, Tab::Scope];
    if view.cpu_snapshot.is_some() {
        tabs_present.push(Tab::Cpu);
    }
    let titles: Vec<&str> = tabs_present.iter().map(|t| t.label()).collect();
    let active_index = tabs_present.iter().position(|t| *t == view.tab).unwrap_or(0);
    let tabs = Tabs::new(titles)
        .select(active_index)
        .block(Block::default().borders(Borders::BOTTOM))
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, area);
}
