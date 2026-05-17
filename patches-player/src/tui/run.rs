//! Terminal setup, teardown, and the input/draw loop.

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
use ratatui::Terminal;

use patches_observation::processor::{
    SCOPE_RING_SAMPLES, SPECTRUM_FFT_SIZES,
};
use patches_observation::subscribers::SubscribersHandle;

use super::render::draw;
use super::state::{PromptKind, SpectrumMode, Tab, View};

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

/// Drive the TUI until the user quits or `external_quit` is set.
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
        view.pump_leds(handle);

        terminal.draw(|f| draw(f, view, handle))?;

        let elapsed = last_frame.elapsed();
        let timeout = FRAME_INTERVAL.saturating_sub(elapsed);
        if event::poll(timeout)? {
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Release {
                    if let Some(outcome) = handle_key(view, k.code) {
                        return Ok(outcome);
                    }
                }
            }
        }
        last_frame = Instant::now();
    }
}

fn handle_key(view: &mut View, code: KeyCode) -> Option<LoopOutcome> {
    // While a bundle-dir prompt is open, keys feed the prompt instead
    // of triggering tab / playback controls. Esc cancels; Enter
    // commits; the loop closure picks up the queued action.
    if view.prompt.is_some() {
        match code {
            KeyCode::Esc => view.cancel_prompt(),
            KeyCode::Enter => view.commit_prompt(),
            KeyCode::Backspace => {
                if let Some(p) = view.prompt.as_mut() {
                    p.input.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(p) = view.prompt.as_mut() {
                    p.input.push(c);
                }
            }
            _ => {}
        }
        return None;
    }
    match code {
        KeyCode::Char('q') | KeyCode::Esc => return Some(LoopOutcome::Quit),
        KeyCode::Char('r') => view.toggle_record_mute(),
        KeyCode::Up => scroll_up(view),
        KeyCode::Down => scroll_down(view),
        KeyCode::Char('1') => view.tab = Tab::Events,
        KeyCode::Char('2') => view.tab = Tab::Meters,
        KeyCode::Char('3') => view.tab = Tab::Spectrum,
        KeyCode::Char('4') => view.tab = Tab::Scope,
        KeyCode::Char('5') => {
            if view.cpu_snapshot.is_some() {
                view.tab = Tab::Cpu;
            }
        }
        KeyCode::Tab => cycle_tab(view),
        KeyCode::Char('z') => toggle_scope_snap(view),
        KeyCode::Char('m') => toggle_spectrum_mode(view),
        KeyCode::Char('f') => cycle_fft_size(view),
        KeyCode::Char('-') => scope_window_zoom(view, 0.5),
        KeyCode::Char('=') => scope_window_zoom(view, 2.0),
        // Bundle-dir prompts (ADR 0075, ticket 0898).
        KeyCode::Char('b') => view.open_prompt(PromptKind::AddBundleDir),
        KeyCode::Char('B') => view.open_prompt(PromptKind::RemoveBundleDir),
        _ => {}
    }
    None
}

fn scroll_up(view: &mut View) {
    match view.tab {
        Tab::Meters => view.meter_scroll = view.meter_scroll.saturating_sub(1),
        Tab::Events => view.log_scroll = view.log_scroll.saturating_add(1),
        Tab::Spectrum | Tab::Scope | Tab::Cpu => {}
    }
}

fn scroll_down(view: &mut View) {
    match view.tab {
        Tab::Meters => view.meter_scroll = view.meter_scroll.saturating_add(1),
        Tab::Events => view.log_scroll = view.log_scroll.saturating_sub(1),
        Tab::Spectrum | Tab::Scope | Tab::Cpu => {}
    }
}

fn cycle_tab(view: &mut View) {
    view.tab = view.tab.next();
    if view.tab == Tab::Cpu && view.cpu_snapshot.is_none() {
        view.tab = view.tab.next();
    }
}

fn toggle_scope_snap(view: &mut View) {
    view.scope_snap_zero = !view.scope_snap_zero;
    view.log.push(format!(
        "scope snap-to-zero = {}",
        if view.scope_snap_zero { "on" } else { "off" }
    ));
}

fn toggle_spectrum_mode(view: &mut View) {
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

fn cycle_fft_size(view: &mut View) {
    let cur = view.spectrum_opts.resolve_fft_size();
    let i = SPECTRUM_FFT_SIZES.iter().position(|&n| n == cur).unwrap_or(0);
    let next = SPECTRUM_FFT_SIZES[(i + 1) % SPECTRUM_FFT_SIZES.len()];
    view.spectrum_opts.fft_size = next;
    view.log.push(format!("spectrum FFT size = {next}"));
}

fn scope_window_zoom(view: &mut View, factor: f32) {
    let max_ms = SCOPE_RING_SAMPLES as f32 / view.header.sample_rate as f32 * 1000.0;
    view.scope_window_ms = (view.scope_window_ms * factor).clamp(1.0, max_ms);
    view.log
        .push(format!("scope window = {:.1} ms", view.scope_window_ms));
}
