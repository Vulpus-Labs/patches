//! `patches-check` — validate a `.patches` file and print diagnostics in a
//! compact, machine-readable form. Non-zero exit on any error diagnostic.
//!
//! Usage:
//!   patches-check [--module-path DIR|FILE]... <path-to-patch.patches>
//!
//! Output format (one per line):
//!   <path>:<line>:<col>: <severity>: [<code>] <message>
//! Paths without source locations appear as `<synthetic>:0:0`.

use std::path::PathBuf;
use std::process;

use patches_core::source_map::{line_col, SourceMap};
use patches_core::source_span::SourceId;
use patches_core::AudioEnvironment;
use patches_diagnostics::{RenderedDiagnostic, Severity, Snippet};
use patches_host::{load::load_patch, source::PathSource};

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Note => "note",
    }
}

fn resolve(source_map: &SourceMap, snippet: &Snippet) -> (String, u32, u32) {
    let path = source_map
        .path(snippet.source)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unknown>".to_string());
    let text = source_map
        .source_text(snippet.source)
        .unwrap_or("");
    let (line, col) = if snippet.source == SourceId(0) {
        (0, 0)
    } else {
        line_col(text, snippet.range.start)
    };
    (path, line, col)
}

fn print_diag(d: &RenderedDiagnostic, source_map: &SourceMap) {
    let (path, line, col) = resolve(source_map, &d.primary);
    let code = d.code.as_deref().unwrap_or("-");
    println!(
        "{path}:{line}:{col}: {}: [{code}] {}",
        severity_str(d.severity),
        d.message,
    );
    if !d.primary.label.is_empty() {
        println!("  ^ {}", d.primary.label);
    }
    for snippet in &d.related {
        let (p, l, c) = resolve(source_map, snippet);
        println!("  note at {p}:{l}:{c}: {}", snippet.label);
    }
}

fn print_usage() {
    eprintln!("usage: patches-check [--module-path DIR|FILE]... <path-to-patch.patches>");
}

fn main() {
    let mut patch_path: Option<String> = None;
    let mut module_paths: Vec<PathBuf> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--module-path" => match args.next() {
                Some(p) => module_paths.push(PathBuf::from(p)),
                None => {
                    eprintln!("error: --module-path requires an argument");
                    process::exit(2);
                }
            },
            "-h" | "--help" => {
                print_usage();
                return;
            }
            _ => patch_path = Some(a),
        }
    }

    let Some(path) = patch_path else {
        print_usage();
        process::exit(2);
    };

    let mut registry = patches_modules::default_registry();
    if !module_paths.is_empty() {
        let scanner = patches_ffi::PluginScanner::new(module_paths);
        let report = scanner.scan(&mut registry);
        for (p, e) in &report.errors {
            eprintln!("plugin scan error: {}: {e}", p.display());
        }
    }

    // A fixed, realistic environment. Validation is independent of sample
    // rate for well-formed patches, but modules need *some* value.
    let env = AudioEnvironment {
        sample_rate: 48_000.0,
        poly_voices: 16,
        periodic_update_interval: 32,
        hosted: false,
    };

    let source = PathSource::new(&path);
    match load_patch(&source, &registry, &env) {
        Ok(loaded) => {
            for w in &loaded.expand_warnings {
                // Expand warnings don't carry source spans reliably; print raw.
                println!("<expand>:0:0: warning: [-] {w}");
            }
            for w in &loaded.layering_warnings {
                let d = RenderedDiagnostic::from_layering_warning(w);
                print_diag(&d, &loaded.source_map);
            }
            // 0 errors → success.
            process::exit(0);
        }
        Err(err) => {
            let diagnostics = err.to_rendered_diagnostics();
            let mut had_error = false;
            for d in &diagnostics {
                if d.severity == Severity::Error {
                    had_error = true;
                }
                print_diag(d, &err.source_map);
            }
            process::exit(if had_error { 1 } else { 0 });
        }
    }
}
