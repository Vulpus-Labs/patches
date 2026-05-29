//! CLI: emit the expanded, port-kind-resolved patch graph as JSON.
//!
//! Usage:
//!
//! ```text
//! patches-graph-json <input.patches> [-o <output.json>]
//!                    [--include-path DIR]... [--compact]
//! ```
//!
//! Pipeline mirrors `patches-svg`: load → expand → emit. Exits non-zero on
//! parse/expand failure; diagnostics go to stderr (no panics).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use patches_graph_json::{graph_doc, to_json, to_json_pretty};
use patches_manifest::bundled_manifest;
use patches_manifest::static_registry::registry_from_manifest;

struct Args {
    input: PathBuf,
    output: Option<PathBuf>,
    include_paths: Vec<PathBuf>,
    compact: bool,
}

fn print_usage() {
    eprintln!(
        "usage: patches-graph-json <input.patches> [-o <output.json>] \
[--include-path DIR]... [--compact]"
    );
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut include_paths: Vec<PathBuf> = Vec::new();
    let mut compact = false;

    let mut i = 0;
    while i < raw.len() {
        let arg = &raw[i];
        match arg.as_str() {
            "-h" | "--help" => return Err("help".into()),
            "-o" | "--output" => {
                i += 1;
                let v = raw.get(i).ok_or("missing value for -o")?;
                output = Some(PathBuf::from(v));
            }
            "--include-path" => {
                i += 1;
                let v = raw.get(i).ok_or("missing value for --include-path")?;
                include_paths.push(PathBuf::from(v));
            }
            "--compact" => compact = true,
            s if s.starts_with('-') => return Err(format!("unknown flag: {s}")),
            _ => {
                if input.is_some() {
                    return Err(format!("unexpected positional argument: {arg}"));
                }
                input = Some(PathBuf::from(arg));
            }
        }
        i += 1;
    }

    let input = input.ok_or("missing input path")?;
    Ok(Args { input, output, include_paths, compact })
}

/// Read an included file from disk, searching the master file's parent
/// directory and any `--include-path` entries, in order.
///
/// `--include-path` is CLI-only sugar for renders kicked off outside a project
/// tree; the DSL itself resolves includes relative to the directive's source
/// file. Mirrors `patches-svg`'s loader.
fn make_loader(
    master_dir: PathBuf,
    extra: Vec<PathBuf>,
) -> impl Fn(&Path) -> std::io::Result<String> {
    move |p: &Path| -> std::io::Result<String> {
        if p.is_absolute() && p.exists() {
            return std::fs::read_to_string(p);
        }
        let candidates = std::iter::once(master_dir.clone())
            .chain(extra.iter().cloned())
            .map(|dir| dir.join(p));
        for candidate in candidates {
            if candidate.exists() {
                return std::fs::read_to_string(candidate);
            }
        }
        std::fs::read_to_string(p)
    }
}

fn run() -> Result<(), String> {
    let args = parse_args().map_err(|e| {
        if e == "help" {
            print_usage();
            std::process::exit(0);
        }
        e
    })?;

    let master_dir = args
        .input
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let loader = make_loader(master_dir, args.include_paths);

    let load_result =
        patches_dsl::load_with(&args.input, &loader).map_err(|e| e.to_string())?;
    let expanded = patches_dsl::expand(&load_result.file).map_err(|e| e.to_string())?;

    let registry = registry_from_manifest(bundled_manifest());
    let doc = graph_doc(&expanded.patch, &registry);

    let json = if args.compact {
        to_json(&doc).map_err(|e| e.to_string())?
    } else {
        to_json_pretty(&doc).map_err(|e| e.to_string())?
    };

    match args.output {
        Some(path) => std::fs::write(&path, json).map_err(|e| e.to_string())?,
        None => print!("{json}"),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("patches-graph-json: {msg}");
            ExitCode::FAILURE
        }
    }
}
