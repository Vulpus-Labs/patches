//! Walk `cargo metadata` and fail on forbidden direct dep edges.
//!
//! The forbidden-edge set is the executable form of ADR 0067. Update it
//! here when cuts evolve; treat changes as ADR-worthy.
//!
//! Two rule kinds today:
//!
//! 1. Specific `from -> to` bans (e.g. `patches-svg -> patches-registry`).
//! 2. Leaf-binary bans: a "leaf" crate must not appear as a direct dep
//!    of any other workspace crate.

use std::process::{Command, ExitCode};

use serde_json::Value;

/// Direct-dep bans. Format: (from-crate, to-crate, reason).
const FORBIDDEN: &[(&str, &str, &str)] = &[
    ("patches-svg", "patches-modules", "renderer must stay manifest-only"),
    ("patches-svg", "patches-registry", "renderer routes registry via patches-manifest"),
    ("patches-lsp", "patches-modules", "LSP must stay shape-only via patches-manifest"),
];

/// Leaf crates that must not appear as a direct dep of any other crate.
const LEAF_CRATES: &[&str] = &[
    "patches-player",
    "patches-clap",
    "patches-lsp",
    "patches-svg-cli",
    "patches-tools",
    "patches-vscode",
];

fn main() -> ExitCode {
    let output = match Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("failed to run cargo metadata: {e}");
            return ExitCode::FAILURE;
        }
    };

    let metadata: Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("failed to parse cargo metadata json: {e}");
            return ExitCode::FAILURE;
        }
    };

    let Some(packages) = metadata.get("packages").and_then(Value::as_array) else {
        eprintln!("cargo metadata missing `packages` array");
        return ExitCode::FAILURE;
    };

    let mut violations: Vec<String> = Vec::new();

    for pkg in packages {
        let Some(name) = pkg.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(deps) = pkg.get("dependencies").and_then(Value::as_array) else {
            continue;
        };
        for dep in deps {
            let Some(dep_name) = dep.get("name").and_then(Value::as_str) else {
                continue;
            };

            for (from, to, reason) in FORBIDDEN {
                if name == *from && dep_name == *to {
                    violations.push(format!(
                        "  {from} -> {to}\n      reason: {reason}"
                    ));
                }
            }

            if LEAF_CRATES.contains(&dep_name) {
                violations.push(format!(
                    "  {name} -> {dep_name}\n      reason: leaf crate `{dep_name}` must not appear as a dep"
                ));
            }
        }
    }

    if violations.is_empty() {
        println!("forbidden-edges: ok");
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "forbidden-edges: {} violation(s) (see ADR 0067):",
            violations.len()
        );
        for v in &violations {
            eprintln!("{v}");
        }
        ExitCode::FAILURE
    }
}
