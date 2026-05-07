//! Spectrum tab: log-frequency curve view + rolling heatmap.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use patches_dsl::manifest::TapType;
use patches_observation::processor::spectrum_bin_count;
use patches_observation::subscribers::SubscribersHandle;

use crate::tui::state::{SpectrumMode, TapEntry, View, SPECTRUM_SMOOTH_ALPHA};

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

pub(crate) fn draw_spectrum(
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

fn draw_spectrum_curves(f: &mut Frame, area: Rect, view: &mut View, handle: &SubscribersHandle) {
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

    let expected_bins = spectrum_bin_count(fft_size);
    view.spectrum_smoothed.retain(|_, v| v.len() == expected_bins);

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

fn draw_spectrum_heatmap(f: &mut Frame, area: Rect, view: &mut View, handle: &SubscribersHandle) {
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

    let _ = handle;
    let cap = inner.width as usize;

    let sr = view.header.sample_rate as f64;
    let nyq = sr * 0.5;
    let bin_hz = sr / fft_size as f64;
    let f_min = bin_hz;
    let f_max = nyq.max(f_min * 10.0);
    let log_min = f_min.log10();
    let log_max = f_max.log10();

    let cell_rows = inner.height as usize;
    let freq_cells = cell_rows * 2;
    let buf = f.buffer_mut();
    let history_len = view.heatmap_history.len();
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
            let f_lo = row * 2;
            let f_hi = row * 2 + 1;
            let lo = sample_heat_log(frame, f_lo, freq_cells, bin_hz, log_min, log_max);
            let hi = sample_heat_log(frame, f_hi, freq_cells, bin_hz, log_min, log_max);
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_char('\u{2580}')
                    .set_style(Style::default().fg(heat_colour(hi)).bg(heat_colour(lo)));
            }
        }
    }
}

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
