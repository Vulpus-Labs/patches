//! Build-and-tick regression: a patch must render without the engine
//! halting. Motivated by ticket 0974, where a planner bug allocated a
//! feedback cable a same-tick scratch slot its delayed consumer read with
//! `fused=false`, tripping the `CablePool::read_raw` debug assert. The
//! panic is caught at the tick boundary (ADR 0051) and recorded as a halt,
//! so a test that merely ticks passes unless it inspects `halt_info()` —
//! which the pre-existing `alloc_trap` test did not.
//!
//! Two layers:
//!
//! - [`fixtures_render_without_halting`] — the **gating** regression. It
//!   sweeps `tests/fixtures/`, a set of frozen known-good patches kept
//!   independent of the work-in-progress `examples/` tree, so the gate does
//!   not break on example churn.
//! - [`all_examples_build_and_render_without_halting`] — an **advisory**
//!   sweep over the live `examples/` tree (run with `--ignored`). Examples
//!   are work-in-progress, so this is a smoke check, not a push gate.
//!
//! The real defence against the 0974 *class* lives lower down: the
//! `patches-core` `output_position`/`input_position` unit tests and the
//! planner's `validate_scratch_fused_consistency` plan-build assert. These
//! sweeps are the outside-in backstop.

use std::path::{Path, PathBuf};

use patches_integration_tests::{build_engine, env};
use patches_modules::default_registry;

fn collect_patches(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_patches(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("patches") {
            out.push(p);
        }
    }
}

/// Build and tick a single patch; `Err` describes a parse/expand/interpret
/// failure or an engine halt.
fn render_once(path: &Path) -> Result<(), String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("read failed: {e}"))?;
    let file = patches_dsl::parse(&src).map_err(|e| format!("parse failed: {e:?}"))?;
    let result = patches_dsl::expand(&file).map_err(|e| format!("expand failed: {e:?}"))?;
    let registry = default_registry();
    let graph = patches_interpreter::build(&result.patch, &registry, &env())
        .map_err(|e| format!("interpret failed: {e:?}"))?
        .graph;
    let mut engine = build_engine(&graph, &registry);
    for _ in 0..512 {
        engine.tick();
        if let Some(info) = engine.halt_info() {
            return Err(format!("engine halted: {info:?}"));
        }
    }
    Ok(())
}

fn payload_string(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Render every patch under `root`, returning a `rel: reason` failure line
/// for each that errors or panics. `catch_unwind` attributes a build-time
/// panic (e.g. the planner's `validate_scratch_fused_consistency` assert)
/// to its file instead of aborting the whole sweep.
fn render_all(root: &Path) -> (usize, Vec<String>) {
    let mut files = Vec::new();
    collect_patches(root, &mut files);
    files.sort();
    let mut failures = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(root).unwrap_or(path).display().to_string();
        match std::panic::catch_unwind(|| render_once(path)) {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => failures.push(format!("{rel}: {msg}")),
            Err(payload) => failures.push(format!("{rel}: panicked: {}", payload_string(payload))),
        }
    }
    (files.len(), failures)
}

/// Gating regression: every frozen fixture must build and render cleanly.
#[test]
fn fixtures_render_without_halting() {
    let root = PathBuf::from(format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR")));
    let (count, failures) = render_all(&root);
    assert!(count > 0, "no fixtures found under {}", root.display());
    assert!(
        failures.is_empty(),
        "{} of {count} fixtures failed to render:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Examples that cannot build with `default_registry()` because they depend
/// on modules from external bundles (drums, vintage). Skipped with a visible
/// reason rather than silently — a new external-dependent example must be
/// added here consciously, while a core-module rename still fails the sweep.
fn requires_external_bundle(rel: &str) -> Option<&'static str> {
    match rel {
        "guitar_solo.patches" => Some("uses Kick (patches-drums) + VDco/VLadder (patches-vintage)"),
        _ => None,
    }
}

/// Advisory smoke check over the work-in-progress `examples/` tree. Run with
/// `cargo test -p patches-integration-tests -- --ignored`. Not a push gate:
/// examples churn, so a half-finished example breaking here is expected and
/// must not block unrelated work. The gating regression is
/// [`fixtures_render_without_halting`] plus the lower-level unit tests.
#[test]
#[ignore = "advisory sweep over work-in-progress examples; run with --ignored"]
fn all_examples_build_and_render_without_halting() {
    let root = PathBuf::from(format!("{}/../examples", env!("CARGO_MANIFEST_DIR")));
    let mut files = Vec::new();
    collect_patches(&root, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no example patches found under {}", root.display());

    let mut failures = Vec::new();
    let mut skipped = Vec::new();
    for path in &files {
        let rel = path.strip_prefix(&root).unwrap_or(path).display().to_string();
        if let Some(reason) = requires_external_bundle(&rel) {
            skipped.push(format!("{rel} ({reason})"));
            continue;
        }
        match std::panic::catch_unwind(|| render_once(path)) {
            Ok(Ok(())) => {}
            Ok(Err(msg)) => failures.push(format!("{rel}: {msg}")),
            Err(payload) => failures.push(format!("{rel}: panicked: {}", payload_string(payload))),
        }
    }

    if !skipped.is_empty() {
        eprintln!("skipped {} external-bundle example(s):", skipped.len());
        for s in &skipped {
            eprintln!("  - {s}");
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} rendered example patches failed ({} skipped):\n{}",
        failures.len(),
        files.len() - skipped.len(),
        skipped.len(),
        failures.join("\n")
    );
}
