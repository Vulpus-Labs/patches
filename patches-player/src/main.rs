//! `patch_player` — load a `.patches` file, play it, and hot-reload on change.
//!
//! Usage:
//!   patch_player <path-to-patch.patches>

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, SystemTime};

use patches_core::{AudioEnvironment, SourceMap};
use patches_diagnostics::RenderedDiagnostic;
use patches_engine::{new_event_queue, DeviceConfig, EventScheduler, MidiConnector, OversamplingFactor, PatchEngine, PatchEngineError, enumerate_devices};

mod diagnostic_render;

struct LoadedPatch {
    build_result: patches_interpreter::BuildResult,
    dependencies: Vec<PathBuf>,
}

/// Errors surfaced by `load_patch`. Carries a `SourceMap` for path/line
/// resolution when the failure has a [`patches_core::Provenance`].
#[derive(Debug)]
enum LoadPatchError {
    Load(patches_dsl::LoadError),
    Expand {
        err: patches_dsl::ExpandError,
        source_map: SourceMap,
    },
    Interpret {
        err: patches_interpreter::InterpretError,
        source_map: SourceMap,
    },
}

impl std::fmt::Display for LoadPatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadPatchError::Load(e) => write!(f, "{e}"),
            LoadPatchError::Expand { err, .. } => write!(f, "expand error: {}", err.message),
            LoadPatchError::Interpret { err, .. } => write!(f, "build error: {}", err.message),
        }
    }
}

impl std::error::Error for LoadPatchError {}

impl LoadPatchError {
    /// Render this error to stderr as a structured diagnostic (when
    /// source-located). `LoadError` variants have no provenance and fall
    /// back to plain-text printing.
    fn render_to_stderr(&self) {
        match self {
            LoadPatchError::Load(e) => eprintln!("error: {e}"),
            LoadPatchError::Expand { err, source_map } => {
                let d = RenderedDiagnostic::from_expand_error(err, source_map);
                diagnostic_render::render_to_stderr(&d, source_map);
            }
            LoadPatchError::Interpret { err, source_map } => {
                let d = interpret_diagnostic(err);
                diagnostic_render::render_to_stderr(&d, source_map);
            }
        }
    }
}

/// Build a structured diagnostic from an [`patches_interpreter::InterpretError`].
/// Interpret errors carry a full [`patches_core::Provenance`] chain but no
/// [`patches_core::build_error::BuildError`] code — synthesise a minimal one.
fn interpret_diagnostic(err: &patches_interpreter::InterpretError) -> RenderedDiagnostic {
    use patches_diagnostics::{Severity, Snippet, SnippetKind};
    let primary = Snippet {
        source: err.provenance.site.source,
        range: err.provenance.site.start..err.provenance.site.end,
        label: "here".to_string(),
        kind: SnippetKind::Primary,
    };
    let related = err
        .provenance
        .expansion
        .iter()
        .map(|s| Snippet {
            source: s.source,
            range: s.start..s.end,
            label: "expanded from here".to_string(),
            kind: SnippetKind::Expansion,
        })
        .collect();
    RenderedDiagnostic {
        severity: Severity::Error,
        code: Some("interpret".to_string()),
        message: err.message.clone(),
        primary,
        related,
    }
}

fn load_patch(
    path: &str,
    registry: &patches_core::Registry,
    sample_rate: f32,
) -> Result<LoadedPatch, LoadPatchError> {
    let master_path = Path::new(path);
    let load_result = patches_dsl::load_with(master_path, |p| fs::read_to_string(p))
        .map_err(LoadPatchError::Load)?;
    let source_map = load_result.source_map;
    let result = patches_dsl::expand(&load_result.file).map_err(|err| {
        LoadPatchError::Expand { err, source_map: source_map.clone() }
    })?;
    for w in &result.warnings {
        eprintln!("dsl warning: {w}");
    }
    let env = AudioEnvironment { sample_rate, poly_voices: 16, periodic_update_interval: 32, hosted: false };
    let base_dir = master_path.parent();
    let build_result = patches_interpreter::build_with_base_dir(&result.patch, registry, &env, base_dir)
        .map_err(|err| LoadPatchError::Interpret { err, source_map })?;
    Ok(LoadedPatch {
        build_result,
        dependencies: load_result.dependencies,
    })
}

/// Push a build result to `engine`, retrying if the plan channel is full.
fn push_build_result(engine: &mut PatchEngine, result: &patches_interpreter::BuildResult) {
    loop {
        match engine.update_with_tracker_data(&result.graph, result.tracker_data.clone()) {
            Ok(()) => {
                println!("Reloaded.");
                return;
            }
            Err(PatchEngineError::ChannelFull) => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                eprintln!("reload error: {e}");
                return;
            }
        }
    }
}

fn run(
    path: &str,
    record_path: Option<&str>,
    oversampling: OversamplingFactor,
    no_stdin: bool,
    device_config: DeviceConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let registry = patches_modules::default_registry();

    // Build with a placeholder rate; we rebuild after starting the engine to
    // use the actual device sample rate.
    let loaded = match load_patch(path, &registry, 44_100.0) {
        Ok(l) => l,
        Err(e) => {
            e.render_to_stderr();
            return Err("failed to load patch".into());
        }
    };

    let engine_registry = patches_modules::default_registry();
    let mut engine = PatchEngine::with_device_config(
        engine_registry,
        oversampling,
        device_config,
    )?;

    let (midi_producer, midi_consumer) = new_event_queue(256);
    engine.start_with_tracker_data(
        &loaded.build_result.graph,
        loaded.build_result.tracker_data.clone(),
        Some(midi_consumer),
        record_path,
    )?;

    let sample_rate = engine.sample_rate().unwrap_or(44_100.0);

    // Rebuild with the actual device sample rate if it differs from the placeholder.
    if (sample_rate - 44_100.0).abs() > 1.0 {
        match load_patch(path, &registry, sample_rate) {
            Ok(reloaded) => push_build_result(&mut engine, &reloaded.build_result),
            Err(e) => {
                eprintln!("warn: rebuild at device sample rate failed");
                e.render_to_stderr();
            }
        }
    }
    let scheduler = EventScheduler::new(sample_rate, 128);
    let _midi_connector = match MidiConnector::open(engine.clock(), midi_producer, scheduler) {
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

        // Spawn a thread that blocks waiting for any stdin input (or EOF).
        // Either signals a clean shutdown so all destructors run (e.g. the WAV
        // recorder flushes its file) rather than relying on Ctrl-C / SIGKILL.
        let quit_flag = Arc::clone(&quit);
        thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            // Any input or EOF triggers shutdown.
            let _ = stdin.lock().read_line(&mut line);
            quit_flag.store(true, Ordering::Release);
        });
    }

    // Track mtimes for all files in the dependency set.
    let mut watched: HashMap<PathBuf, SystemTime> = HashMap::new();
    for dep in &loaded.dependencies {
        if let Ok(t) = fs::metadata(dep).and_then(|m| m.modified()) {
            watched.insert(dep.clone(), t);
        }
    }

    loop {
        thread::sleep(Duration::from_millis(500));

        if quit.load(Ordering::Acquire) {
            break;
        }

        // Check if any watched file has changed.
        let changed = watched.iter().any(|(p, last)| {
            fs::metadata(p)
                .and_then(|m| m.modified())
                .map(|t| t != *last)
                .unwrap_or(false)
        });

        if changed {
            match load_patch(path, &registry, sample_rate) {
                Ok(loaded) => {
                    push_build_result(&mut engine, &loaded.build_result);
                    // Refresh the watched set from the new dependency list.
                    watched.clear();
                    for dep in &loaded.dependencies {
                        if let Ok(t) = fs::metadata(dep).and_then(|m| m.modified()) {
                            watched.insert(dep.clone(), t);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("parse error (keeping current patch):");
                    e.render_to_stderr();
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
}

fn main() {
    let mut patch_path: Option<String> = None;
    let mut record_path: Option<String> = None;
    let mut oversampling = OversamplingFactor::None;
    let mut no_stdin = false;
    let mut list_devices = false;
    let mut device_config = DeviceConfig::default();

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--no-stdin" => {
                no_stdin = true;
            }
            "--list-devices" => {
                list_devices = true;
            }
            "--output-device" => {
                match args.next() {
                    Some(name) => device_config.output_device = Some(name),
                    None => {
                        eprintln!("error: --output-device requires a device name argument");
                        process::exit(1);
                    }
                }
            }
            "--input-device" => {
                match args.next() {
                    Some(name) => device_config.input_device = Some(name),
                    None => {
                        eprintln!("error: --input-device requires a device name argument");
                        process::exit(1);
                    }
                }
            }
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

    if let Err(e) = run(&path, record_path.as_deref(), oversampling, no_stdin, device_config) {
        // Structured diagnostics (LoadPatchError) have already been rendered
        // to stderr at the source. Non-source errors surface as plain text.
        eprintln!("error: {e}");
        process::exit(1);
    }
}
