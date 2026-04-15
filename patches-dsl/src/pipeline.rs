//! Staged patch loading pipeline (ADR 0038).
//!
//! Exposes each stage from the ADR as a named entry point so every
//! consumer (player, CLAP, LSP) can compose them the same way and differ
//! only in where they stop on failure.
//!
//! The stages owned by this module are:
//!
//! 1. [`load`] — resolve includes from a root path to a merged
//!    [`LoadResult`].
//! 2. [`parse`] — pest is already driven inside [`load`]; exposed here
//!    as a pass-through for symmetry (the load step consumes pest
//!    errors into [`LoadError`]).
//! 3. [`expand`] — mechanical template expansion, emits a
//!    [`FlatPatch`]. Every structural error is classified inline and
//!    returned as an [`ExpandError`] with a [`crate::structural::StructuralCode`].
//! 4. [`bind`] lives in `patches-interpreter` because it requires the
//!    module registry and audio environment — callers compose it
//!    themselves or use [`run_all`] with a supplied closure.
//!
//! Tree-sitter fallback (stage 4 of ADR 0038) is **not** part of this
//! orchestrator; LSP invokes it directly when [`load`] or [`expand`]
//! fail. Keeping the orchestrator pest-only matches the ADR's "TS stays
//! parallel" decision.

use std::path::Path;

use crate::expand::{expand as expand_fn, ExpandError, ExpandResult};
use crate::flat::FlatPatch;
use crate::loader::{load_with, LoadError, LoadResult};

/// Stage 1: load the root file and resolve includes.
pub fn load<F>(master_path: &Path, read_file: F) -> Result<LoadResult, LoadError>
where
    F: Fn(&Path) -> std::io::Result<String>,
{
    load_with(master_path, read_file)
}

/// Stage 2: pest parse is executed inside [`load`]. This helper exists
/// so callers can name the stage — it simply returns the already-merged
/// [`LoadResult`].
pub fn parse(load_result: LoadResult) -> LoadResult {
    load_result
}

/// Stage 2, inline-source form. Used by consumers that carry DSL text in
/// memory with no filesystem backing (e.g. a CLAP plugin restored from
/// host state without the original `.patches` file on disk). Wraps
/// [`crate::parse`]; no include resolution is performed.
pub fn parse_source(source: &str) -> Result<crate::File, crate::ParseError> {
    crate::parse(source)
}

/// Stage 3: mechanical template expansion.
///
/// Produces a [`FlatPatch`] on success. Any structural failure surfaces
/// as an [`ExpandError`] classified by [`crate::structural::StructuralCode`].
pub fn expand(result: &LoadResult) -> Result<ExpandResult, ExpandError> {
    expand_fn(&result.file)
}

/// Stage 3, file-input form. Used by consumers (notably `patches-clap`)
/// that sometimes enter the pipeline with a [`crate::File`] already in
/// hand — e.g. state restored without the original file on disk — rather
/// than driving stages 1–2 fresh. Equivalent to [`expand`] otherwise.
pub fn expand_file(file: &crate::File) -> Result<ExpandResult, ExpandError> {
    expand_fn(file)
}

/// Combined stage 1–3 driver. Stage 3b (binding) requires the module
/// registry which lives in `patches-interpreter`; pass a closure that
/// performs the bind step.
pub fn run_all<F, T, E>(
    master_path: &Path,
    read_file: impl Fn(&Path) -> std::io::Result<String>,
    bind: F,
) -> Result<Staged<T>, PipelineError<E>>
where
    F: FnOnce(&LoadResult, &FlatPatch) -> Result<T, E>,
{
    let loaded = load(master_path, read_file).map_err(PipelineError::Load)?;
    let expanded = expand(&loaded).map_err(PipelineError::Expand)?;
    let bound = bind(&loaded, &expanded.patch).map_err(PipelineError::Bind)?;
    Ok(Staged {
        loaded,
        patch: expanded.patch,
        bound,
        warnings: expanded.warnings,
    })
}

/// Accumulate-and-continue driver for LSP.
///
/// Runs stages 1–3 best-effort: each stage's errors are collected but the
/// pipeline keeps going on whatever partial artifact the stage produced.
/// Returns an [`AccumulatedRun`] carrying every stage's output (as
/// `Option`s) plus all errors collected along the way.
///
/// Stage 3b (binding) is injected as a closure so this crate doesn't depend
/// on `patches-interpreter`. Callers typically pass `|flat| descriptor_bind::bind(flat, registry)`.
pub fn run_accumulate<F, T>(
    master_path: &Path,
    read_file: impl Fn(&Path) -> std::io::Result<String>,
    bind: F,
) -> AccumulatedRun<T>
where
    F: FnOnce(&FlatPatch) -> T,
{
    let mut run = AccumulatedRun {
        loaded: None,
        patch: None,
        bound: None,
        load_errors: Vec::new(),
        expand_errors: Vec::new(),
        warnings: Vec::new(),
    };

    let loaded = match load(master_path, read_file) {
        Ok(l) => l,
        Err(e) => {
            run.load_errors.push(e);
            return run;
        }
    };
    run.loaded = Some(loaded);
    let loaded_ref = run.loaded.as_ref().expect("just assigned");

    let expanded = match expand(loaded_ref) {
        Ok(r) => r,
        Err(e) => {
            run.expand_errors.push(e);
            return run;
        }
    };
    run.warnings = expanded.warnings;
    run.patch = Some(expanded.patch);

    let patch_ref = run.patch.as_ref().expect("just assigned");
    run.bound = Some(bind(patch_ref));

    run
}

impl<T> AccumulatedRun<T> {
    /// Returns `true` when stage 2 (pest parse) produced an error.
    ///
    /// `load_with` is fail-fast, so any parse failure aborts stages 1–2
    /// and the resulting [`LoadError`] lands in [`Self::load_errors`]
    /// as a [`crate::loader::LoadErrorKind::Parse`]. LSP uses this to
    /// decide whether to invoke the tree-sitter fallback (ADR 0038
    /// stages 4a–4c).
    pub fn stage_2_failed(&self) -> bool {
        self.load_errors
            .iter()
            .any(|e| matches!(e.kind, crate::loader::LoadErrorKind::Parse { .. }))
    }
}

/// Artifacts + accumulated errors produced by [`run_accumulate`].
///
/// Every field is populated best-effort: `loaded` is `Some` if stage 1
/// succeeded, `patch` is `Some` if stage 3 produced a FlatPatch, `bound`
/// is `Some` whenever `patch` is. Bind-stage errors live *inside* the
/// bound artifact (`BoundPatch::errors`) so callers don't need a separate
/// vector.
#[derive(Debug)]
pub struct AccumulatedRun<T> {
    pub loaded: Option<LoadResult>,
    pub patch: Option<FlatPatch>,
    pub bound: Option<T>,
    pub load_errors: Vec<LoadError>,
    pub expand_errors: Vec<ExpandError>,
    pub warnings: Vec<crate::expand::Warning>,
}

/// The set of artifacts produced by a successful end-to-end pipeline run.
#[derive(Debug)]
pub struct Staged<T> {
    pub loaded: LoadResult,
    pub patch: FlatPatch,
    pub bound: T,
    pub warnings: Vec<crate::expand::Warning>,
}

/// Aggregated pipeline error. The variant identifies which stage
/// failed; the inner error carries the stage-specific details.
#[derive(Debug)]
pub enum PipelineError<E> {
    Load(LoadError),
    Expand(ExpandError),
    Bind(E),
}

impl<E: std::fmt::Display> std::fmt::Display for PipelineError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(e) => write!(f, "load: {e}"),
            Self::Expand(e) => write!(f, "expand: {e}"),
            Self::Bind(e) => write!(f, "bind: {e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for PipelineError<E> {}
