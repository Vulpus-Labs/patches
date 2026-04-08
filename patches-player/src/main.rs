//! `patch_player` — load a `.patches` file, play it, and hot-reload on change.
//!
//! Usage:
//!   patch_player <path-to-patch.patches>

use std::env;
use std::fs;
use std::io::BufRead;
use std::process;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, SystemTime};

use patches_core::AudioEnvironment;
use patches_engine::{new_event_queue, DeviceConfig, EventScheduler, MidiConnector, OversamplingFactor, PatchEngine, PatchEngineError, enumerate_devices};

fn mtime(path: &str) -> std::io::Result<SystemTime> {
    fs::metadata(path)?.modified()
}

fn load_graph(
    path: &str,
    registry: &patches_core::Registry,
) -> Result<patches_core::ModuleGraph, Box<dyn std::error::Error>> {
    let src = fs::read_to_string(path)?;
    let file = patches_dsl::parse(&src)?;
    let result = patches_dsl::expand(&file)?;
    for w in &result.warnings {
        eprintln!("dsl warning: {w}");
    }
    let env = AudioEnvironment { sample_rate: 44_100.0, poly_voices: 16, periodic_update_interval: 32 };
    Ok(patches_interpreter::build(&result.patch, registry, &env)?)
}

/// Push `graph` to `engine`, retrying if the plan channel is full.
fn push_graph(engine: &mut PatchEngine, graph: &patches_core::ModuleGraph) {
    loop {
        match engine.update(graph) {
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
    let load_registry = patches_modules::default_registry();
    let graph = load_graph(path, &load_registry)?;

    let engine_registry = patches_modules::default_registry();
    let mut engine = PatchEngine::with_device_config(
        engine_registry,
        oversampling,
        device_config,
    )?;

    let (midi_producer, midi_consumer) = new_event_queue(256);
    engine.start(&graph, Some(midi_consumer), record_path)?;

    let sample_rate = engine.sample_rate().unwrap_or(44_100.0);
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

    let mut last_mtime = mtime(path)?;

    loop {
        thread::sleep(Duration::from_millis(500));

        if quit.load(Ordering::Acquire) {
            break;
        }

        let current_mtime = match mtime(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("warn: could not stat {path}: {e}");
                continue;
            }
        };

        if current_mtime != last_mtime {
            last_mtime = current_mtime;
            match load_graph(path, &load_registry) {
                Ok(graph) => push_graph(&mut engine, &graph),
                Err(e) => eprintln!("parse error (keeping current patch): {e}"),
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
        eprintln!("error: {e}");
        process::exit(1);
    }
}
