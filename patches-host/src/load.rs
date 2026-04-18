//! Shared patch-load helper.
//!
//! Drives stages 3 (expand), 3b (descriptor bind), 5 (graph build) on top
//! of whatever stages 1–2 the [`HostFileSource`] supplied. Stage 4
//! (planner build) is left to [`crate::HostBuilder`] / [`crate::HostRuntime`]
//! because it requires the host's planner instance and audio environment.

use std::path::PathBuf;

use patches_core::{source_map::SourceMap, AudioEnvironment};
use patches_dsl::pipeline::{LayeringWarning, PipelineAudit};
use patches_interpreter::BuildResult;
use patches_registry::Registry;

use crate::{CompileError, HostFileSource};

/// Output of [`load_patch`]: the build result ready for planner ingestion
/// plus everything a host needs to render diagnostics and watch
/// dependencies.
pub struct LoadedPatch {
    pub build_result: BuildResult,
    pub source_map: SourceMap,
    pub dependencies: Vec<PathBuf>,
    pub layering_warnings: Vec<LayeringWarning>,
    pub expand_warnings: Vec<patches_dsl::Warning>,
}

/// Run the post-load DSL pipeline against a host source: expand, bind,
/// and build the runtime `ModuleGraph`.
///
/// Stops at the first failing stage and returns a [`CompileError`]
/// tagged with that stage. Bind errors are aggregated into a single
/// `Bind` variant (matching the player and CLAP behaviour pre-host).
pub fn load_patch(
    source: &dyn HostFileSource,
    registry: &Registry,
    env: &AudioEnvironment,
) -> Result<LoadedPatch, CompileError> {
    let loaded = source.load()?;
    let expanded = patches_dsl::pipeline::expand_file(&loaded.file)?;
    let flat = expanded.patch;

    let bound = patches_interpreter::bind_with_base_dir(&flat, registry, source.base_dir());
    let layering_warnings = bound.layering_warnings();
    if !bound.errors.is_empty() {
        return Err(CompileError::Bind(bound.graph.errors));
    }

    let build_result = patches_interpreter::build_from_bound(&bound, env)?;

    Ok(LoadedPatch {
        build_result,
        source_map: loaded.source_map,
        dependencies: loaded.dependencies,
        layering_warnings,
        expand_warnings: expanded.warnings,
    })
}
