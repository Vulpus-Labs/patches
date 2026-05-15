//! `patch_player` — load a `.patches` file, play it, hot-reload on change.
//!
//! Default frontend: ratatui TUI (ticket 0704, ADR 0055 §5). Pass
//! `--no-tui` for the legacy stdout flow (kept for headless smoke runs).

#[cfg(feature = "audio-thread-allocator-trap")]
#[global_allocator]
static ALLOC: patches_alloc_trap::TrappingAllocator = patches_alloc_trap::TrappingAllocator;

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::BufRead;
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use patches_cpal::{enumerate_devices, DeviceConfig, SoundEngine};
use patches_diagnostics::RenderedDiagnostic;
use patches_engine::{
    monitor_channel, new_event_queue, EventScheduler, MidiConnector, MonitorAttach,
    MonitorConfig, OversamplingFactor, DEFAULT_MONITOR_CAPACITY,
};
use patches_host::{CompileError, HostBuilder, LoadedPatch, PathSource};
use patches_observation::{spawn_observer, tap_ring};

mod controller_env;
mod cpu_monitor;
mod diagnostic_render;
mod splash;
mod tui;

use controller_env::{EnvSideChannel, RatatuiEnv};
use patches_plugin_common::{Action, Controller, Env as _};

/// Render a [`CompileError`] to stderr using the source map it carries.
fn render_compile_error(err: &CompileError) {
    for d in err.to_rendered_diagnostics() {
        diagnostic_render::render_to_stderr(&d, &err.source_map);
    }
}

/// Render the warnings carried by a successful load.
fn render_load_warnings(loaded: &LoadedPatch) {
    for w in &loaded.expand_warnings {
        eprintln!("dsl warning: {w}");
    }
    for w in &loaded.layering_warnings {
        let d = RenderedDiagnostic::from_layering_warning(w);
        diagnostic_render::render_to_stderr(&d, &loaded.source_map);
    }
}

/// Persist current controller settings to the sidecar (ADR 0063 §5;
/// ticket 0776). Failure is non-fatal — surface in the view log.
fn flush_sidecar(
    controller: &Controller,
    runtime: &mut patches_host::HostRuntime,
    side: &mut EnvSideChannel,
    view: &mut tui::View,
) {
    let file_path = match controller.file_path.as_ref() {
        Some(p) => p.clone(),
        None => return,
    };
    let mut env = RatatuiEnv { runtime, side };
    let sidecar = match env.sidecar_path(&file_path) {
        Some(p) => p,
        None => return,
    };
    let settings = controller.persisted_settings();
    if let Err(e) = patches_plugin_common::Env::save_sidecar(&mut env, &sidecar, &settings) {
        view.log.push(format!("sidecar save failed: {e}"));
    }
}

/// Drain newly-appended controller status entries into the view's
/// event log. `cursor` tracks how many we've already drained so that
/// repeated calls don't duplicate lines.
fn drain_status(view: &mut tui::View, controller: &Controller, cursor: &mut usize) {
    let total = controller.status_log.len();
    if *cursor > total {
        *cursor = 0; // log was rotated; replay from the start.
    }
    for line in controller.status_log.iter().skip(*cursor) {
        view.log.push(line.clone());
    }
    *cursor = total;
}

fn refresh_watched(watched: &mut HashMap<PathBuf, SystemTime>, deps: &[PathBuf]) {
    watched.clear();
    for dep in deps {
        if let Ok(t) = fs::metadata(dep).and_then(|m| m.modified()) {
            watched.insert(dep.clone(), t);
        }
    }
}

struct CommonSetup {
    sound: SoundEngine,
    runtime: patches_host::HostRuntime,
    source: PathSource,
    registry: patches_core::registry::Registry,
    sample_rate: f32,
}

fn common_setup(
    path: &str,
    oversampling: OversamplingFactor,
    device_config: DeviceConfig,
    module_paths: Vec<PathBuf>,
) -> Result<CommonSetup, Box<dyn std::error::Error>> {
    let mut registry = patches_modules::default_registry();

    if !module_paths.is_empty() {
        let scanner = patches_ffi::PluginScanner::new(module_paths);
        let report = scanner.scan(&mut registry);
        println!("module scan: {}", report.summary());
        for m in &report.loaded {
            println!("  loaded  {} v{:#x} ({})", m.name, m.version, m.path.display());
        }
        for r in &report.replaced {
            println!("  replaced {} v{:#x} → v{:#x} ({})", r.name, r.from, r.to, r.path.display());
        }
        for s in &report.skipped {
            println!("  skipped  {s:?}");
        }
        for (p, e) in &report.errors {
            eprintln!("  error   {}: {e}", p.display());
        }
    }

    let mut sound = SoundEngine::new(oversampling);
    let env = sound.open(&device_config)?;
    let sample_rate = env.sample_rate;

    let runtime = HostBuilder::new()
        .oversampling_factor(oversampling.factor())
        .build(env)?;

    let source = PathSource::new(path);

    Ok(CommonSetup {
        sound,
        runtime,
        source,
        registry,
        sample_rate,
    })
}

fn run_headless(
    path: &str,
    record_path: Option<&str>,
    oversampling: OversamplingFactor,
    no_stdin: bool,
    device_config: DeviceConfig,
    module_paths: Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let CommonSetup { mut sound, mut runtime, source, registry, sample_rate, .. } =
        common_setup(path, oversampling, device_config, module_paths)?;

    let loaded = match runtime.compile_and_push_blocking(&source, &registry) {
        Ok(loaded) => loaded,
        Err(e) => {
            render_compile_error(&e);
            return Err("failed to load patch".into());
        }
    };
    render_load_warnings(&loaded);

    let dependencies = loaded.dependencies.clone();
    drop(loaded);

    let halt_handle = runtime.halt_handle();
    let (processor, plan_rx) = runtime
        .take_audio_endpoints()
        .ok_or("audio endpoints already taken")?;

    let (midi_producer, midi_consumer) = new_event_queue(256);
    sound.start(processor, plan_rx, Some(midi_consumer), record_path, None)?;

    let scheduler = EventScheduler::new(sample_rate, 128);
    let _midi_connector = match MidiConnector::open(sound.clock(), midi_producer, scheduler) {
        Ok(c) => {
            println!("MIDI input open.");
            Some(c)
        }
        Err(e) => {
            eprintln!("warn: could not open MIDI input: {e}");
            None
        }
    };

    println!("Loaded {path}");

    let quit = Arc::new(AtomicBool::new(false));

    if no_stdin {
        println!("Running (kill process to stop)…");
    } else {
        println!("Watching for changes… (press Enter to stop)");
        let quit_flag = Arc::clone(&quit);
        thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            let _ = stdin.lock().read_line(&mut line);
            quit_flag.store(true, Ordering::Release);
        });
    }

    let mut watched: HashMap<PathBuf, SystemTime> = HashMap::new();
    refresh_watched(&mut watched, &dependencies);

    let mut halt_reported = false;
    loop {
        thread::sleep(Duration::from_millis(500));

        if quit.load(Ordering::Acquire) {
            break;
        }

        match halt_handle.halt_info() {
            Some(info) if !halt_reported => {
                let first_line = info.payload.lines().next().unwrap_or("").to_string();
                eprintln!(
                    "engine halted: module {:?} (slot {}) panicked: {}\n  edit the patch to reload.",
                    info.module_name, info.slot, first_line
                );
                halt_reported = true;
            }
            None if halt_reported => {
                halt_reported = false;
            }
            _ => {}
        }

        let changed = watched.iter().any(|(p, last)| {
            fs::metadata(p)
                .and_then(|m| m.modified())
                .map(|t| t != *last)
                .unwrap_or(false)
        });

        if changed {
            match runtime.compile_and_push_blocking(&source, &registry) {
                Ok(loaded) => {
                    render_load_warnings(&loaded);
                    println!("Reloaded.");
                    refresh_watched(&mut watched, &loaded.dependencies);
                }
                Err(e) => {
                    eprintln!("parse error (keeping current patch):");
                    render_compile_error(&e);
                    for (p, last) in watched.iter_mut() {
                        if let Ok(t) = fs::metadata(p).and_then(|m| m.modified()) {
                            *last = t;
                        }
                    }
                }
            }
        }
    }

    sound.stop();
    Ok(())
}

fn run_tui(
    path: &str,
    record_path: Option<&str>,
    oversampling: OversamplingFactor,
    device_config: DeviceConfig,
    module_paths: Vec<PathBuf>,
    monitor_enabled: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let CommonSetup { mut sound, mut runtime, source: _source, registry, sample_rate, .. } =
        common_setup(path, oversampling, device_config, module_paths.clone())?;
    if monitor_enabled {
        runtime.set_monitor(true);
    }

    // Stand up the observer thread + tap ring before the first compile so
    // the planner's manifest publication reaches the observer (ADR 0056).
    let (tap_tx, tap_rx) = tap_ring(64);
    let (mut observer, mut diag_rx) = spawn_observer(tap_rx, Duration::from_millis(2));
    runtime.attach_observer(
        observer.take_replans().ok_or("observer replan producer missing")?,
    );
    let subs_handle = observer.subscribers.clone();

    // Build the unified controller; seed persistable state from CLI args.
    // The registry was already populated with module-path scans by
    // `common_setup`; hand it to the controller verbatim so a subsequent
    // controller-driven scan would rediscover the same modules.
    let mut controller = Controller::new();
    controller.registry = registry;
    controller.module_paths = module_paths.clone();
    controller.module_names = controller
        .registry
        .module_names()
        .map(|s| s.to_string())
        .collect();
    controller.module_names.sort();
    controller.file_path = Some(PathBuf::from(path));
    let mut side = EnvSideChannel::default();

    let initial_delta = {
        let mut env = RatatuiEnv { runtime: &mut runtime, side: &mut side };
        controller.apply(Action::LoadPath(PathBuf::from(path)), &mut env)
    };
    let _ = initial_delta;
    if !controller.diagnostic_view.diagnostics.is_empty() && side.last_manifest.is_none() {
        for d in &controller.diagnostic_view.diagnostics {
            diagnostic_render::render_to_stderr(
                d,
                controller
                    .diagnostic_view
                    .source_map
                    .as_ref()
                    .unwrap_or(&patches_core::source_map::SourceMap::new()),
            );
        }
        return Err("failed to load patch".into());
    }
    let initial_taps = side
        .last_manifest
        .as_ref()
        .map(tui::taps_from_manifest)
        .unwrap_or_default();
    let dependencies = std::mem::take(&mut side.last_dependencies);
    let initial_expand_warnings = std::mem::take(&mut side.last_expand_warnings);

    let halt_handle = runtime.halt_handle();
    let (mut processor, plan_rx) = runtime
        .take_audio_endpoints()
        .ok_or("audio endpoints already taken")?;
    processor.set_tap_producer(Some(tap_tx));

    let cpu_monitor = if monitor_enabled {
        let (mon_tx, mon_rx) = monitor_channel(DEFAULT_MONITOR_CAPACITY);
        processor.set_monitor(Some(MonitorAttach {
            config: MonitorConfig::default(),
            tx: mon_tx,
        }));
        Some(cpu_monitor::spawn_cpu_monitor(mon_rx, sample_rate))
    } else {
        None
    };

    let record_muted = record_path.map(|_| Arc::new(AtomicBool::new(true)));

    let (midi_producer, midi_consumer) = new_event_queue(256);
    sound.start(
        processor,
        plan_rx,
        Some(midi_consumer),
        record_path,
        record_muted.clone(),
    )?;

    let scheduler = EventScheduler::new(sample_rate, 128);
    let midi_status = match MidiConnector::open(sound.clock(), midi_producer, scheduler) {
        Ok(c) => (Some(c), "MIDI input open".to_string()),
        Err(e) => (None, format!("warn: could not open MIDI input: {e}")),
    };
    let (_midi_connector, midi_msg) = midi_status;

    let header = tui::HeaderInfo {
        patch_path: path.to_string(),
        sample_rate: sample_rate as u32,
        oversampling: oversampling.factor() as u32,
    };
    let record = tui::RecordState {
        record_path: record_path.map(|s| s.to_string()),
        muted: record_muted,
    };
    let mut view = tui::View::new(header, initial_taps, record);
    if let Some(m) = cpu_monitor.as_ref() {
        view.attach_cpu_snapshot(m.snapshot.clone());
    }
    // Drain controller status log into the view; subsequent reloads
    // append more entries which we drain incrementally below.
    let mut last_status_drained: usize = 0;
    drain_status(&mut view, &controller, &mut last_status_drained);
    for w in initial_expand_warnings {
        view.log.push(w);
    }
    let snap = controller.snapshot();
    if !snap.module_paths.is_empty() {
        view.log.push(format!("module paths: {}", snap.module_paths.join(", ")));
    }
    view.log.push(midi_msg);

    let mut watched: HashMap<PathBuf, SystemTime> = HashMap::new();
    refresh_watched(&mut watched, &dependencies);
    let _ = dependencies;

    // Sidecar debounce window (ADR 0063 §5; ticket 0776). Settings
    // edits flush after the loop has been quiet for this long.
    const SIDECAR_DEBOUNCE: Duration = Duration::from_millis(500);
    let mut dirty_at: Option<Instant> = None;

    let external_quit = Arc::new(AtomicBool::new(false));
    let mut halt_reported = false;
    let mut frame_counter: u64 = 0;

    let mut terminal = tui::enter_terminal()?;
    let _ = splash::show_until_dismissed(
        &mut terminal,
        Duration::from_secs(3),
        &external_quit,
    );
    let outcome = tui::run(
        &mut terminal,
        &mut view,
        &subs_handle,
        &external_quit,
        |view| {
            // Drain observer-side diagnostics into the event log.
            for d in diag_rx.drain() {
                view.log.push(tui::format_diagnostic(&d));
            }

            // Per-slot drop counters → event log (rate-limited per slot).
            view.poll_drops(&subs_handle, std::time::Instant::now());

            // Halt info → event log.
            match halt_handle.halt_info() {
                Some(info) if !halt_reported => {
                    let first_line = info.payload.lines().next().unwrap_or("").to_string();
                    view.log.push(format!(
                        "engine halted: module {:?} (slot {}): {}",
                        info.module_name, info.slot, first_line
                    ));
                    view.engine_state = tui::EngineState::Halted;
                    halt_reported = true;
                }
                None if halt_reported => {
                    view.engine_state = tui::EngineState::Running;
                    halt_reported = false;
                }
                _ => {}
            }

            // Reload check, ~2 Hz to keep redraw cheap.
            frame_counter = frame_counter.wrapping_add(1);
            if frame_counter.is_multiple_of(15) {
                let changed = watched.iter().any(|(p, last)| {
                    fs::metadata(p)
                        .and_then(|m| m.modified())
                        .map(|t| t != *last)
                        .unwrap_or(false)
                });
                if changed {
                    // Clear last_manifest so a failed compile is
                    // distinguishable from a successful one (the env
                    // populates these only on Ok).
                    side.last_manifest = None;
                    side.last_dependencies.clear();
                    side.last_expand_warnings.clear();
                    let delta = {
                        let mut env = RatatuiEnv {
                            runtime: &mut runtime,
                            side: &mut side,
                        };
                        controller.apply(Action::Reload, &mut env)
                    };
                    if delta.persistable_changed {
                        dirty_at = Some(Instant::now());
                    }
                    drain_status(view, &controller, &mut last_status_drained);
                    if let Some(m) = side.last_manifest.take() {
                        for w in std::mem::take(&mut side.last_expand_warnings) {
                            view.log.push(w);
                        }
                        view.set_taps(tui::taps_from_manifest(&m));
                        view.seed_drop_baselines(&subs_handle);
                        view.log.push("reloaded");
                        let new_deps = std::mem::take(&mut side.last_dependencies);
                        refresh_watched(&mut watched, &new_deps);
                    } else {
                        view.log.push("parse error (keeping current patch):");
                        for d in &controller.diagnostic_view.diagnostics {
                            view.log.push(format!("compile error: {}", d.message));
                        }
                        for (p, last) in watched.iter_mut() {
                            if let Ok(t) = fs::metadata(p).and_then(|m| m.modified()) {
                                *last = t;
                            }
                        }
                    }
                }
            }

            // Sidecar debounce flush. Only fires when the loop has been
            // quiet for SIDECAR_DEBOUNCE; sequential mutations within
            // the window collapse to a single save.
            if let Some(t) = dirty_at {
                if t.elapsed() >= SIDECAR_DEBOUNCE {
                    flush_sidecar(&controller, &mut runtime, &mut side, view);
                    dirty_at = None;
                }
            }
        },
    );

    // Final flush so a pending debounced save isn't lost on quit.
    if dirty_at.is_some() {
        flush_sidecar(&controller, &mut runtime, &mut side, &mut view);
    }

    let restore = tui::leave_terminal(&mut terminal);
    sound.stop();
    observer.stop();
    drop(cpu_monitor);

    outcome?;
    restore?;
    Ok(())
}

fn print_usage() {
    eprintln!("usage: patch_player [options] <path-to-patch.patches>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --oversampling <1|2|4|8>   Oversampling factor (default: 1)");
    eprintln!("  --record <path.wav>        Record output to WAV file");
    eprintln!("  --output-device <name>     Use named output device (default: system default)");
    eprintln!("  --input-device <name>      Open named input device for audio capture");
    eprintln!("  --list-devices             List available audio devices and exit");
    eprintln!("  --no-stdin                 (legacy/--no-tui) run without stdin");
    eprintln!("  --no-tui                   Use the legacy stdout frontend");
    eprintln!("  --module-path <DIR|FILE>   Scan directory or file for FFI plugin bundles (repeatable)");
    eprintln!("  --monitor                  Enable per-instance CPU monitor tab (ADR 0065)");
}

fn main() {
    let mut patch_path: Option<String> = None;
    let mut record_path: Option<String> = None;
    let mut oversampling = OversamplingFactor::None;
    let mut no_stdin = false;
    let mut no_tui = false;
    let mut list_devices = false;
    let mut device_config = DeviceConfig::default();
    let mut module_paths: Vec<PathBuf> = Vec::new();
    let mut monitor_enabled = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-stdin" => no_stdin = true,
            "--no-tui" => no_tui = true,
            "--module-path" => match args.next() {
                Some(p) => module_paths.push(PathBuf::from(p)),
                None => {
                    eprintln!("error: --module-path requires a directory or file argument");
                    process::exit(1);
                }
            },
            "--list-devices" => list_devices = true,
            "--monitor" | "--cpu-monitor" => monitor_enabled = true,
            "--output-device" => match args.next() {
                Some(name) => device_config.output_device = Some(name),
                None => {
                    eprintln!("error: --output-device requires a device name argument");
                    process::exit(1);
                }
            },
            "--input-device" => match args.next() {
                Some(name) => device_config.input_device = Some(name),
                None => {
                    eprintln!("error: --input-device requires a device name argument");
                    process::exit(1);
                }
            },
            "--record" => {
                record_path = args.next();
                if record_path.is_none() {
                    eprintln!("error: --record requires a file path argument");
                    process::exit(1);
                }
            }
            "--oversampling" => {
                let val = args.next().unwrap_or_default();
                oversampling = match val.as_str() {
                    "1" => OversamplingFactor::None,
                    "2" => OversamplingFactor::X2,
                    "4" => OversamplingFactor::X4,
                    "8" => OversamplingFactor::X8,
                    _ => {
                        print_usage();
                        process::exit(1);
                    }
                };
            }
            _ => patch_path = Some(arg),
        }
    }

    if list_devices {
        let devices = enumerate_devices();
        if devices.is_empty() {
            println!("No audio devices found.");
        } else {
            println!("Available audio devices:\n");
            for d in &devices {
                let caps = match (d.is_input, d.is_output) {
                    (true, true) => "input/output",
                    (true, false) => "input",
                    (false, true) => "output",
                    (false, false) => "unknown",
                };
                println!("  {:<50} [{}]", d.name, caps);
            }
        }
        return;
    }

    let path = match patch_path {
        Some(p) => p,
        None => {
            print_usage();
            process::exit(1);
        }
    };

    let result = if no_tui {
        run_headless(
            &path,
            record_path.as_deref(),
            oversampling,
            no_stdin,
            device_config,
            module_paths,
        )
    } else {
        run_tui(
            &path,
            record_path.as_deref(),
            oversampling,
            device_config,
            module_paths,
            monitor_enabled,
        )
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        process::exit(1);
    }
}
