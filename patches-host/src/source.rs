//! `HostFileSource` — abstracts how a host obtains a parsed `File`.
//!
//! Two starting impls:
//! - [`PathSource`]: the player-style case — read a file from disk and
//!   resolve includes relative to it.
//! - [`InMemorySource`]: the CLAP-style case — DSL source supplied as a
//!   string with an optional master path (used to resolve includes when
//!   the file is present on disk) and an optional base dir (used by
//!   `bind_with_base_dir` to resolve relative asset references).

use std::path::{Path, PathBuf};

use patches_core::source_map::SourceMap;
use patches_dsl::pipeline;

use crate::CompileError;

/// Parsed master file (with includes resolved when applicable) plus the
/// source map needed to render diagnostics and the dependency list used
/// for file-polling hot-reload.
pub struct LoadedSource {
    pub file: patches_dsl::File,
    pub source_map: SourceMap,
    pub dependencies: Vec<PathBuf>,
}

/// Where a host obtains its DSL `File`.
///
/// Implementors are responsible for stages 1–2 of the DSL pipeline
/// (load + pest parse). The crate-level `load_patch` helper drives the
/// remainder (expand, bind, build).
pub trait HostFileSource {
    fn load(&self) -> Result<LoadedSource, CompileError>;

    /// Base directory used by the interpreter to resolve relative asset
    /// paths (e.g. impulse-response WAVs). Defaults to `None`.
    fn base_dir(&self) -> Option<&Path> { None }
}

/// File-system-backed source. Reads the master file from disk and
/// resolves includes relative to it.
pub struct PathSource {
    pub path: PathBuf,
    base_dir: Option<PathBuf>,
}

impl PathSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let base_dir = path.parent().map(|p| p.to_path_buf());
        Self { path, base_dir }
    }
}

impl HostFileSource for PathSource {
    fn load(&self) -> Result<LoadedSource, CompileError> {
        let lr = pipeline::load(&self.path, |p| std::fs::read_to_string(p))?;
        Ok(LoadedSource {
            file: lr.file,
            source_map: lr.source_map,
            dependencies: lr.dependencies,
        })
    }

    fn base_dir(&self) -> Option<&Path> { self.base_dir.as_deref() }
}

/// In-memory source. If `master_path` is set and points to a file that
/// exists on disk, includes are resolved relative to it (the in-memory
/// `source` is substituted for the master file's contents to avoid a
/// redundant disk read or TOCTOU). Otherwise the source is parsed as a
/// single file with no include resolution and an empty `SourceMap`.
///
/// The canonical form of `master_path` is computed once at construction
/// so include resolution does not re-canonicalize per file.
pub struct InMemorySource {
    pub source: String,
    master_path: Option<PathBuf>,
    master_canonical: Option<PathBuf>,
    pub base_dir: Option<PathBuf>,
}

impl InMemorySource {
    pub fn new(source: String) -> Self {
        Self {
            source,
            master_path: None,
            master_canonical: None,
            base_dir: None,
        }
    }

    pub fn with_master_path(mut self, path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        if self.base_dir.is_none() {
            self.base_dir = path.parent().map(|p| p.to_path_buf());
        }
        self.master_canonical = Some(
            path.canonicalize().unwrap_or_else(|_| path.clone()),
        );
        self.master_path = Some(path);
        self
    }

    pub fn master_path(&self) -> Option<&Path> { self.master_path.as_deref() }
}

impl HostFileSource for InMemorySource {
    fn load(&self) -> Result<LoadedSource, CompileError> {
        if let (Some(path), Some(master_canonical)) =
            (self.master_path.as_deref(), self.master_canonical.as_deref())
        {
            if path.exists() {
                let master_source = &self.source;
                let lr = pipeline::load(path, |p| {
                    let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                    if canonical == master_canonical {
                        Ok(master_source.clone())
                    } else {
                        std::fs::read_to_string(p)
                    }
                })?;
                return Ok(LoadedSource {
                    file: lr.file,
                    source_map: lr.source_map,
                    dependencies: lr.dependencies,
                });
            }
        }
        let file = pipeline::parse_source(&self.source)?;
        Ok(LoadedSource {
            file,
            source_map: SourceMap::new(),
            dependencies: Vec::new(),
        })
    }

    fn base_dir(&self) -> Option<&Path> { self.base_dir.as_deref() }
}
