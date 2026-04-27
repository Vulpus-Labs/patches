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
use std::time::{Duration, SystemTime};

use patches_cpal::{enumerate_devices, DeviceConfig, SoundEngine};
use patches_diagnostics::RenderedDiagnostic;
use patches_engine::{new_event_queue, EventScheduler, MidiConnector, OversamplingFactor};
use patches_host::{CompileError, HostBuilder, LoadedPatch, PathSource};
use patches_observation::{spawn_observer, tap_ring};

mod diagnostic_render;
mod splash;
mod tui;

/// Render a [`CompileError`] to stderr using the source map it carries.
fn render_compile_error(err: &CompileError) {
    for d in err.to_rendered_diagnostics() {
        diagnostic_render::render_to_stderr(&d, &err.source_map);
    }
}

/// Format compile errors as a list of one-line strings for the event log.
fn compile_error_lines(err: &CompileError) -> Vec<String> {
    err.to_rendered_diagnostics()
        .iter()
        .map(|d| format!("compile error: {}", d.message))
        .collect()
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

fn load_warning_lines(loaded: &LoadedPatch) -> Vec<String> {
    let mut out: Vec<String> = loaded
        .expand_warnings
        .iter()
        .map(|w| format!("dsl warning: {w}"))
        .collect();
    for w in &loaded.layering_warnings {
        let d = RenderedDiagnostic::from_layering_warning(w);
        out.push(format!("warning: {}", d.message));
    }
    out
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
    registry: patches_registry::Registry,
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
) -> Result<(), Box<dyn std::error::Error>> {
    let CommonSetup { mut sound, mut runtime, source, registry, sample_rate, .. } =
        common_setup(path, oversampling, device_config, module_paths)?;

    // Stand up the observer thread + tap ring before the first compile so
    // the planner's manifest publication reaches the observer (ADR 0056).
    let (tap_tx, tap_rx) = tap_ring(64);
    let (mut observer, mut diag_rx) = spawn_observer(tap_rx, Duration::from_millis(2));
    runtime.attach_observer(
        observer.take_replans().ok_or("observer replan producer missing")?,
    );
    let subs_handle = observer.subscribers.clone();

    let initial = match runtime.compile_and_push_blocking(&source, &registry) {
        Ok(loaded) => loaded,
        Err(e) => {
            render_compile_error(&e);
            return Err("failed to load patch".into());
        }
    };
    let initial_warnings = load_warning_lines(&initial);
    let initial_taps = tui::taps_from_manifest(&initial.manifest);
    let dependencies = initial.dependencies.clone();
    drop(initial);

    let halt_handle = runtime.halt_handle();
    let (mut processor, plan_rx) = runtime
        .take_audio_endpoints()
        .ok_or("audio endpoints already taken")?;
    processor.set_tap_producer(Some(tap_tx));

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
    view.log.push(format!("loaded {path}"));
    for w in initial_warnings {
        view.log.push(w);
    }
    view.log.push(midi_msg);

    let mut watched: HashMap<PathBuf, SystemTime> = HashMap::new();
    refresh_watched(&mut watched, &dependencies);

    let external_quit = Arc::new(AtomicBool::new(false));
    let mut halt_reported = false;
    let mut frame_counter: u64 = 0;

    let mut terminal = tui::enter_terminal()?;
    let _ = splash::show_until_dismissed(
        &mut terminal,
        Duration::from_secs(5),
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
                    match runtime.compile_and_push_blocking(&source, &registry) {
                        Ok(loaded) => {
                            for w in load_warning_lines(&loaded) {
                                view.log.push(w);
                            }
                            view.set_taps(tui::taps_from_manifest(&loaded.manifest));
                            view.seed_drop_baselines(&subs_handle);
                            view.log.push("reloaded");
                            refresh_watched(&mut watched, &loaded.dependencies);
                        }
                        Err(e) => {
                            view.log.push("parse error (keeping current patch):");
                            for line in compile_error_lines(&e) {
                                view.log.push(line);
                            }
                            for (p, last) in watched.iter_mut() {
                                if let Ok(t) = fs::metadata(p).and_then(|m| m.modified()) {
                                    *last = t;
                                }
                            }
                        }
                    }
                }
            }
        },
    );

    let restore = tui::leave_terminal(&mut terminal);
    sound.stop();
    observer.stop();

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
        )
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        process::exit(1);
    }
}
