//! `patch_player` — load a `.patches` file, play it, and hot-reload on change.
//!
//! Usage:
//!   patch_player <path-to-patch.patches>

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::BufRead;
use std::path::PathBuf;
use std::process;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, SystemTime};

use patches_cpal::{enumerate_devices, DeviceConfig, SoundEngine};
use patches_diagnostics::RenderedDiagnostic;
use patches_engine::{new_event_queue, EventScheduler, MidiConnector, OversamplingFactor};
use patches_host::{CompileError, HostBuilder, LoadedPatch, PathSource};

mod diagnostic_render;

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

fn refresh_watched(watched: &mut HashMap<PathBuf, SystemTime>, deps: &[PathBuf]) {
    watched.clear();
    for dep in deps {
        if let Ok(t) = fs::metadata(dep).and_then(|m| m.modified()) {
            watched.insert(dep.clone(), t);
        }
    }
}

fn run(
    path: &str,
    record_path: Option<&str>,
    oversampling: OversamplingFactor,
    no_stdin: bool,
    device_config: DeviceConfig,
    module_paths: Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
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

    // Open the audio device first so we know the actual sample rate before
    // building the planner / processor / patch.
    let mut sound = SoundEngine::new(oversampling);
    let env = sound.open(&device_config)?;
    let sample_rate = env.sample_rate;

    let mut runtime = HostBuilder::new()
        .oversampling_factor(oversampling.factor())
        .build(env)?;

    let source = PathSource::new(path);
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

    let (processor, plan_rx) = runtime
        .take_audio_endpoints()
        .ok_or("audio endpoints already taken")?;

    let (midi_producer, midi_consumer) = new_event_queue(256);
    sound.start(processor, plan_rx, Some(midi_consumer), record_path)?;

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

        // A clean shutdown via stdin lets all destructors run (e.g. the WAV
        // recorder flushes its file) rather than relying on Ctrl-C / SIGKILL.
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

    loop {
        thread::sleep(Duration::from_millis(500));

        if quit.load(Ordering::Acquire) {
            break;
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
                    // Update mtimes so we don't spam errors on every poll.
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

fn print_usage() {
    eprintln!("usage: patch_player [options] <path-to-patch.patches>");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --oversampling <1|2|4|8>   Oversampling factor (default: 1)");
    eprintln!("  --record <path.wav>        Record output to WAV file");
    eprintln!("  --output-device <name>     Use named output device (default: system default)");
    eprintln!("  --input-device <name>      Open named input device for audio capture");
    eprintln!("  --list-devices             List available audio devices and exit");
    eprintln!("  --no-stdin                 Run without stdin (kill process to stop)");
    eprintln!("  --module-path <DIR|FILE>   Scan directory or file for FFI plugin bundles (repeatable)");
}

fn main() {
    let mut patch_path: Option<String> = None;
    let mut record_path: Option<String> = None;
    let mut oversampling = OversamplingFactor::None;
    let mut no_stdin = false;
    let mut list_devices = false;
    let mut device_config = DeviceConfig::default();
    let mut module_paths: Vec<PathBuf> = Vec::new();

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-stdin" => no_stdin = true,
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

    if let Err(e) = run(&path, record_path.as_deref(), oversampling, no_stdin, device_config, module_paths) {
        // Structured diagnostics (CompileError) have already been rendered
        // to stderr at the source. Non-source errors surface as plain text.
        eprintln!("error: {e}");
        process::exit(1);
    }
}

