//! Oscilloscope tab.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use patches_dsl::manifest::TapType;
use patches_observation::processor::{ScopeReadOpts, SCOPE_RING_SAMPLES, SCOPE_WINDOW_DEFAULT};
use patches_observation::subscribers::SubscribersHandle;

use crate::tui::state::{TapEntry, View};

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

fn first_rising_zero_crossing_frac(samples: &[f32]) -> Option<f32> {
    for (i, pair) in samples.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        if a <= 0.0 && b > 0.0 {
            return Some(i as f32 + a / (a - b));
        }
    }
    None
}

/// Derive `ScopeReadOpts` from a user-facing window length in ms.
pub fn scope_opts_for_window_ms(
    window_ms: f32,
    sample_rate: u32,
    _pane_w: u16,
) -> ScopeReadOpts {
    let raw = (window_ms.max(0.1) as f64 * sample_rate as f64 / 1000.0).round() as usize;
    let raw = raw.clamp(2, SCOPE_RING_SAMPLES);
    let dec = raw.div_ceil(SCOPE_DISPLAY_CAP).max(1);
    let win = (raw / dec).max(2);
    ScopeReadOpts { decimation: dec, window_samples: win }
}

pub const SCOPE_DISPLAY_CAP: usize = 4096;

pub(crate) fn draw_scope(f: &mut Frame, area: Rect, view: &mut View, handle: &SubscribersHandle) {
    let title = format!(
        "scope  ({:.1} ms) [snap {}]",
        view.scope_window_ms,
        if view.scope_snap_zero { "on" } else { "off" },
    );
    let block = Block::default().borders(Borders::ALL).title(title);
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

    let mut series: Vec<(String, Color, Vec<f32>)> = Vec::with_capacity(scope_taps.len());
    for (i, tap) in scope_taps.iter().enumerate() {
        let color = SCOPE_PALETTE[i % SCOPE_PALETTE.len()];
        let _ok = handle.read_scope_into_with(tap.slot, scope_opts, &mut view.scope_scratch);
        series.push((tap.name.clone(), color, view.scope_scratch.clone()));
    }

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
