//! Walk `cargo metadata` and fail on forbidden direct dep edges.
//!
//! The forbidden-edge set is the executable form of ADR 0067. Update it
//! here when cuts evolve; treat changes as ADR-worthy.
//!
//! Two rule kinds today:
//!
//! 1. Specific `from -> to` bans.
//! 2. Leaf-binary bans: a "leaf" crate must not appear as a direct dep
//!    of any other workspace crate.

use std::process::{Command, ExitCode};

use serde_json::Value;

/// Direct-dep bans. Format: (from-crate, to-crate, reason).
const FORBIDDEN: &[(&str, &str, &str)] = &[
    ("patches-svg", "patches-modules", "renderer must stay manifest-only"),
    ("patches-lsp", "patches-modules", "LSP must stay shape-only via patches-manifest"),
];

/// Crates whose *normal* (non-dev, non-build) direct dependency set is
/// restricted to an allowlist. Any normal dep not in the allowlist is a
/// violation — this turns a documented layering claim into an enforced
/// one. Additions to an allowlist must be deliberate (and ADR-worthy).
///
/// Format: (crate, allowed-normal-deps, reason).
const DEP_ALLOWLIST: &[(&str, &[&str], &str)] = &[
    (
        // CLAUDE.md's load-bearing leaf claim: patches-dsp has no
        // patches-core / CPAL / serde footprint. rtrb (lock-free ring
        // buffers) is the only permitted dep. Previously true only by
        // luck (ticket 1002).
        "patches-dsp",
        &["rtrb"],
        "DSP leaf crate must stay free of patches-core / CPAL / serde \
         (CLAUDE.md); only rtrb is permitted",
    ),
];

/// Leaf crates that must not appear as a direct dep of any other crate.
const LEAF_CRATES: &[&str] = &[
    "patches-player",
    "patches-clap",
    "patches-lsp",
    "patches-tools",
    "patches-forbidden-edges",
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

            // Allowlist enforcement on normal deps only — dev / build
            // dependencies (tests, build scripts) are exempt. `kind` is
            // absent / null for a normal dependency.
            let is_normal = dep.get("kind").and_then(Value::as_str).is_none();
            if is_normal {
                for (crate_name, allowed, reason) in DEP_ALLOWLIST {
                    if name == *crate_name && !allowed.contains(&dep_name) {
                        violations.push(format!(
                            "  {name} -> {dep_name}\n      reason: {reason}"
                        ));
                    }
                }
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
