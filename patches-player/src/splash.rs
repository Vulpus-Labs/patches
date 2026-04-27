//! Startup splash. The image is baked into the binary at build time
//! by `build.rs` (see `assets/splash.png`).
//!
//! Render strategy: each terminal cell holds 2×4 sub-pixels via a
//! Braille character (U+2800 + dot mask). Each sub-pixel is "lit" if
//! its luminance exceeds [`LIT_LUMA_THRESHOLD`]. The cell's foreground
//! colour is the average RGB of its lit sub-pixels (so dark regions
//! don't desaturate brighter neighbours). Cells with no lit
//! sub-pixels render as space (no character, transparent).
//!
//! Trade-off vs half-block: 4× vertical resolution and 2× horizontal,
//! at the cost of one colour per cell instead of two. Better for
//! detail-heavy or high-contrast art; less good for smooth gradients.

use std::io::{self, Stdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Style};
use ratatui::Terminal;

include!(concat!(env!("OUT_DIR"), "/splash.rs"));

/// True when a splash image was baked into the binary.
pub fn has_splash() -> bool {
    WIDTH > 0 && HEIGHT > 0
}

fn pixel(x: usize, y: usize) -> (u8, u8, u8) {
    let i = (y * WIDTH + x) * 3;
    (PIXELS[i], PIXELS[i + 1], PIXELS[i + 2])
}

/// 8-bit luminance from sRGB triplet (Rec. 601 weights, integer math).
fn luma(r: u8, g: u8, b: u8) -> u32 {
    (77 * r as u32 + 150 * g as u32 + 29 * b as u32) >> 8
}

/// Sub-pixel luminance threshold (0..255). Anything above is "lit".
/// Tuned conservatively so dark backgrounds stay empty while
/// mid-tones and highlights paint.
const LIT_LUMA_THRESHOLD: u32 = 8;

/// Bit mask for sub-pixel `(dx, dy)` within a Braille 2×4 cell.
/// Standard Unicode Braille layout:
/// ```text
///   1 4
///   2 5
///   3 6
///   7 8
/// ```
fn braille_bit(dx: usize, dy: usize) -> u32 {
    match (dx, dy) {
        (0, 0) => 0x01,
        (0, 1) => 0x02,
        (0, 2) => 0x04,
        (0, 3) => 0x40,
        (1, 0) => 0x08,
        (1, 1) => 0x10,
        (1, 2) => 0x20,
        (1, 3) => 0x80,
        _ => 0,
    }
}

/// Render the splash centred in the terminal's current viewport.
/// Each terminal cell is a Braille glyph covering 2×4 image pixels.
/// If the terminal is smaller than the image the splash is clipped.
/// Ripple amplitude in source pixels (vertical displacement peak).
const RIPPLE_AMPLITUDE_PX: f32 = 2.0;
/// Ripple spatial wavelength in source pixels (along x).
const RIPPLE_WAVELENGTH_PX: f32 = 120.0;
/// Ripple temporal period in seconds.
const RIPPLE_PERIOD_S: f32 = 2.0;

pub fn draw_splash(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    phase_s: f32,
) -> io::Result<()> {
    if !has_splash() {
        return Ok(());
    }
    terminal.draw(|f| {
        let two_pi = std::f32::consts::TAU;
        let t_phase = (phase_s / RIPPLE_PERIOD_S) * two_pi;
        let area = f.area();
        // Fit the baked image into the available cell grid while
        // preserving aspect. Sub-pixel grid available is `term_w*2 ×
        // term_h*4`; choose the largest scale where both axes fit.
        // Sub-pixels are visually square (cell aspect ≈ 1:2 cancels
        // the 2×4 sub-pixel layout), so a single scale factor works.
        let avail_sx = (area.width as usize) * 2;
        let avail_sy = (area.height as usize) * 4;
        if avail_sx == 0 || avail_sy == 0 || WIDTH == 0 || HEIGHT == 0 {
            return;
        }
        let scale_num;
        let scale_den;
        if avail_sx * HEIGHT < avail_sy * WIDTH {
            // Width-limited.
            scale_num = avail_sx;
            scale_den = WIDTH;
        } else {
            // Height-limited.
            scale_num = avail_sy;
            scale_den = HEIGHT;
        }
        // Output sub-pixel dimensions, snapped to multiples of (2, 4)
        // so the cell grid is whole.
        let out_sx = (WIDTH * scale_num / scale_den) & !1;
        let out_sy = (HEIGHT * scale_num / scale_den) & !3;
        if out_sx == 0 || out_sy == 0 {
            return;
        }
        let cell_w = out_sx / 2;
        let cell_h = out_sy / 4;
        let ox = (area.width as usize - cell_w) / 2;
        let oy = (area.height as usize - cell_h) / 2;
        let buf = f.buffer_mut();
        for cy in 0..cell_h {
            for cx in 0..cell_w {
                let mut mask: u32 = 0;
                let mut sum_r: u32 = 0;
                let mut sum_g: u32 = 0;
                let mut sum_b: u32 = 0;
                let mut lit: u32 = 0;
                for dy in 0..4 {
                    for dx in 0..2 {
                        // Nearest-neighbour sample from the baked grid.
                        let sx_base = ((cx * 2 + dx) * WIDTH) / out_sx;
                        let sy = ((cy * 4 + dy) * HEIGHT) / out_sy;
                        let wave = (sy as f32 / RIPPLE_WAVELENGTH_PX) * two_pi - t_phase;
                        let dx_off = (RIPPLE_AMPLITUDE_PX * wave.sin()).round() as i32;
                        let sx_i = sx_base as i32 + dx_off;
                        let sx = sx_i.clamp(0, WIDTH as i32 - 1) as usize;
                        let (r, g, b) = pixel(sx, sy);
                        if luma(r, g, b) > LIT_LUMA_THRESHOLD {
                            mask |= braille_bit(dx, dy);
                            sum_r += r as u32;
                            sum_g += g as u32;
                            sum_b += b as u32;
                            lit += 1;
                        }
                    }
                }
                let x = (ox + cx) as u16;
                let y = (oy + cy) as u16;
                let Some(cell) = buf.cell_mut((x, y)) else { continue };
                if lit == 0 {
                    cell.set_char(' ').set_style(Style::default());
                } else {
                    let r = (sum_r / lit) as u8;
                    let g = (sum_g / lit) as u8;
                    let b = (sum_b / lit) as u8;
                    let glyph = char::from_u32(0x2800 + mask).unwrap_or(' ');
                    cell.set_char(glyph)
                        .set_style(Style::default().fg(Color::Rgb(r, g, b)));
                }
            }
        }
    })?;
    Ok(())
}

/// Show the splash until any key is pressed or `timeout` elapses, or
/// `external_quit` fires. Returns immediately if no splash was baked.
pub fn show_until_dismissed(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    timeout: Duration,
    external_quit: &Arc<AtomicBool>,
) -> io::Result<()> {
    if !has_splash() {
        return Ok(());
    }
    let start = Instant::now();
    let frame = Duration::from_millis(40);
    let mut next_frame = start;
    loop {
        if external_quit.load(Ordering::Acquire) {
            return Ok(());
        }
        let now = Instant::now();
        let elapsed = now.duration_since(start);
        if elapsed >= timeout {
            return Ok(());
        }
        if now >= next_frame {
            draw_splash(terminal, elapsed.as_secs_f32())?;
            next_frame = now + frame;
        }
        let remaining = timeout - elapsed;
        let until_frame = next_frame.saturating_duration_since(Instant::now());
        let poll_for = remaining.min(until_frame);
        if event::poll(poll_for)? {
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Release {
                    return Ok(());
                }
            }
        }
    }
}
