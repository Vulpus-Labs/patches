//! CLI: render a `.patches` file to SVG.
//!
//! Usage:
//!
//! ```text
//! patches-svg <input.patches> [-o <output.svg>]
//!             [--include-path DIR]... [--theme light|dark]
//! ```
//!
//! Exits non-zero on parse/expand failure; diagnostics go to stderr.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use patches_modules::default_registry;
use patches_svg::{render_svg, SvgOptions, Theme};

struct Args {
    input: PathBuf,
    output: Option<PathBuf>,
    include_paths: Vec<PathBuf>,
    theme: Theme,
}

fn print_usage() {
    eprintln!(
        "usage: patches-svg <input.patches> [-o <output.svg>] \
[--include-path DIR]... [--theme light|dark]"
    );
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut include_paths: Vec<PathBuf> = Vec::new();
    let mut theme = Theme::Dark;

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
            "--theme" => {
                i += 1;
                let v = raw.get(i).ok_or("missing value for --theme")?;
                theme = match v.as_str() {
                    "light" => Theme::Light,
                    "dark" => Theme::Dark,
                    other => return Err(format!("unknown theme: {other}")),
                };
            }
            s if s.starts_with('-') => {
                return Err(format!("unknown flag: {s}"));
            }
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
    Ok(Args {
        input,
        output,
        include_paths,
        theme,
    })
}

/// Read an included file from disk, searching the master file's parent
/// directory and any `--include-path` entries, in order.
///
/// Note: `--include-path` is CLI-only sugar. The DSL itself resolves
/// includes relative to the directive's source file; this loader adds
/// extra search directories as a convenience for renders kicked off
/// outside a project tree. Do not treat `--include-path` as equivalent
/// to module-search behaviour in other tools.
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

    let opts = SvgOptions {
        theme: args.theme,
        ..SvgOptions::default()
    };
    let registry = default_registry();
    let svg = render_svg(&expanded.patch, &load_result.source_map, &registry, &opts);

    match args.output {
        Some(path) => std::fs::write(&path, svg).map_err(|e| e.to_string())?,
        None => println!("{svg}"),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("patches-svg: {msg}");
            ExitCode::FAILURE
        }
    }
}
