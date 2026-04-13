//! Include-file loader with cycle detection and diamond-dependency deduplication.
//!
//! The loader resolves `include "path"` directives recursively, merging all
//! templates, patterns, and songs into a single [`File`] AST ready for the
//! expander. It accepts a file-reading closure so it can be tested with
//! in-memory file maps.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::{File, Span};
use crate::include_frontier::{normalize_path, EnterResult, IncludeFrontier};
use crate::parser::{parse, parse_include_file, ParseError};

/// The result of a successful load: a merged [`File`] and the full set of
/// loaded file paths (for hot-reload dependency tracking).
#[derive(Debug)]
pub struct LoadResult {
    pub file: File,
    /// All file paths that were loaded (master + all transitive includes),
    /// canonicalized. Suitable for setting up file watchers.
    pub dependencies: Vec<PathBuf>,
}

/// The specific cause of a [`LoadError`].
#[derive(Debug)]
pub enum LoadErrorKind {
    /// Failed to read a source file from disk (or the in-memory reader).
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
    /// A file failed to parse.
    Parse {
        path: PathBuf,
        error: ParseError,
    },
    /// Following an include would form a cycle.
    Cycle {
        parent: PathBuf,
        target: PathBuf,
    },
    /// Two files define the same template/pattern/song name.
    NameCollision {
        kind: &'static str,
        name: String,
        existing: PathBuf,
        new: PathBuf,
    },
}

impl std::fmt::Display for LoadErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadErrorKind::Io { path, error } => {
                write!(f, "cannot read {}: {}", path.display(), error)
            }
            LoadErrorKind::Parse { path, error } => {
                write!(f, "parse error in {}: {}", path.display(), error.message)
            }
            LoadErrorKind::Cycle { parent, target } => write!(
                f,
                "include cycle detected: {} includes {}",
                parent.display(),
                target.display()
            ),
            LoadErrorKind::NameCollision {
                kind,
                name,
                existing,
                new,
            } => write!(
                f,
                "{kind} \"{name}\" defined in both {} and {}",
                existing.display(),
                new.display()
            ),
        }
    }
}

/// An error encountered while loading an include tree.
#[derive(Debug)]
pub struct LoadError {
    pub kind: LoadErrorKind,
    /// The chain of includes that led to the error, innermost last.
    /// Each entry is (file path, span of the include directive in that file).
    pub include_chain: Vec<(PathBuf, Span)>,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.kind)?;
        for (path, span) in &self.include_chain {
            write!(
                f,
                "\n  included from {} ({}..{})",
                path.display(),
                span.start,
                span.end
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for LoadError {}

impl LoadError {
    fn new(kind: LoadErrorKind) -> Self {
        LoadError {
            kind,
            include_chain: vec![],
        }
    }

    fn from_parse_error(e: ParseError, path: &Path) -> Self {
        Self::new(LoadErrorKind::Parse {
            path: path.to_path_buf(),
            error: e,
        })
    }

    fn from_io_error(e: std::io::Error, path: &Path) -> Self {
        Self::new(LoadErrorKind::Io {
            path: path.to_path_buf(),
            error: e,
        })
    }
}

/// Load a master `.patches` file and recursively resolve all `include`
/// directives, producing a merged [`File`] AST.
///
/// `read_file` is called to read each file's contents. Using a closure keeps
/// the loader testable with in-memory file maps.
///
/// Include paths are resolved relative to the directory of the file containing
/// the directive.
pub fn load_with<F>(master_path: &Path, read_file: F) -> Result<LoadResult, LoadError>
where
    F: Fn(&Path) -> Result<String, std::io::Error>,
{
    let master_path = normalize_path(master_path);
    let src = read_file(&master_path).map_err(|e| LoadError::from_io_error(e, &master_path))?;
    let mut master = parse(&src).map_err(|e| LoadError::from_parse_error(e, &master_path))?;

    let mut ctx = ResolveContext {
        frontier: IncludeFrontier::with_root(master_path.clone()),
        all_paths: vec![master_path.clone()],
        defined_names: HashMap::new(),
    };

    // Collect names defined in the master file for collision detection.
    register_names(&master_path, &master.templates, &master.patterns, &master.songs, &mut ctx.defined_names)?;

    // Process includes from the master file.
    resolve_includes(
        &master_path,
        &master.includes,
        &read_file,
        &mut ctx,
        &mut master.templates,
        &mut master.patterns,
        &mut master.songs,
    )?;

    // Clear includes from the merged file (they've been resolved).
    master.includes.clear();

    Ok(LoadResult {
        file: master,
        dependencies: ctx.all_paths,
    })
}

/// Mutable state threaded through the recursive include resolution.
struct ResolveContext {
    frontier: IncludeFrontier<PathBuf>,
    all_paths: Vec<PathBuf>,
    defined_names: HashMap<String, PathBuf>,
}

#[allow(clippy::too_many_arguments)]
fn resolve_includes<F>(
    parent_path: &Path,
    includes: &[crate::ast::IncludeDirective],
    read_file: &F,
    ctx: &mut ResolveContext,
    templates: &mut Vec<crate::ast::Template>,
    patterns: &mut Vec<crate::ast::PatternDef>,
    songs: &mut Vec<crate::ast::SongDef>,
) -> Result<(), LoadError>
where
    F: Fn(&Path) -> Result<String, std::io::Error>,
{
    let parent_dir = parent_path.parent().unwrap_or(Path::new("."));

    for inc in includes {
        let resolved = normalize_path(&parent_dir.join(&inc.path));

        match ctx.frontier.enter(resolved.clone()) {
            EnterResult::Cycle => {
                return Err(LoadError {
                    kind: LoadErrorKind::Cycle {
                        parent: parent_path.to_path_buf(),
                        target: resolved.clone(),
                    },
                    include_chain: vec![(parent_path.to_path_buf(), inc.span)],
                });
            }
            EnterResult::AlreadyVisited => continue,
            EnterResult::Fresh => {}
        }

        let src = read_file(&resolved).map_err(|e| {
            let mut err = LoadError::from_io_error(e, &resolved);
            err.include_chain.push((parent_path.to_path_buf(), inc.span));
            err
        })?;

        let inc_file = parse_include_file(&src).map_err(|e| {
            let mut err = LoadError::from_parse_error(e, &resolved);
            err.include_chain.push((parent_path.to_path_buf(), inc.span));
            err
        })?;

        ctx.all_paths.push(resolved.clone());

        // Check for name collisions.
        register_names(&resolved, &inc_file.templates, &inc_file.patterns, &inc_file.songs, &mut ctx.defined_names)
            .map_err(|mut e| {
                e.include_chain.push((parent_path.to_path_buf(), inc.span));
                e
            })?;

        // Recurse into this file's includes before adding its definitions (depth-first).
        resolve_includes(
            &resolved,
            &inc_file.includes,
            read_file,
            ctx,
            templates,
            patterns,
            songs,
        ).map_err(|mut e| {
            e.include_chain.push((parent_path.to_path_buf(), inc.span));
            e
        })?;
        ctx.frontier.leave(&resolved);

        // Merge definitions (dependencies before dependents due to depth-first).
        templates.extend(inc_file.templates);
        patterns.extend(inc_file.patterns);
        songs.extend(inc_file.songs);
    }

    Ok(())
}

/// Register template, pattern, and song names; error on collision.
fn register_names(
    path: &Path,
    templates: &[crate::ast::Template],
    patterns: &[crate::ast::PatternDef],
    songs: &[crate::ast::SongDef],
    defined: &mut HashMap<String, PathBuf>,
) -> Result<(), LoadError> {
    for t in templates {
        check_collision(&t.name.name, "template", path, defined)?;
    }
    for p in patterns {
        check_collision(&p.name.name, "pattern", path, defined)?;
    }
    for s in songs {
        check_collision(&s.name.name, "song", path, defined)?;
    }
    Ok(())
}

fn check_collision(
    name: &str,
    kind: &'static str,
    path: &Path,
    defined: &mut HashMap<String, PathBuf>,
) -> Result<(), LoadError> {
    let key = format!("{kind}:{name}");
    if let Some(existing) = defined.get(&key) {
        return Err(LoadError::new(LoadErrorKind::NameCollision {
            kind,
            name: name.to_string(),
            existing: existing.clone(),
            new: path.to_path_buf(),
        }));
    }
    defined.insert(key, path.to_path_buf());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as StdHashMap;

    fn make_reader(files: StdHashMap<&str, &str>) -> impl Fn(&Path) -> Result<String, std::io::Error> {
        let owned: StdHashMap<PathBuf, String> = files
            .into_iter()
            .map(|(k, v)| (PathBuf::from(k), v.to_owned()))
            .collect();
        move |path: &Path| {
            let normalized = normalize_path(path);
            owned
                .get(&normalized)
                .cloned()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, format!("{} not found", normalized.display())))
        }
    }

    #[test]
    fn single_include() {
        let files = StdHashMap::from([
            ("main.patches", r#"
                include "lib.patches"
                patch {
                    module osc : Osc
                }
            "#),
            ("lib.patches", r#"
                template voice(freq: float) {
                    in: audio
                    out: audio
                    module osc : Osc
                    osc.out -> $.audio
                }
            "#),
        ]);
        let result = load_with(Path::new("main.patches"), make_reader(files)).unwrap();
        assert_eq!(result.file.templates.len(), 1);
        assert_eq!(result.file.templates[0].name.name, "voice");
        assert_eq!(result.dependencies.len(), 2);
    }

    #[test]
    fn transitive_includes() {
        let files = StdHashMap::from([
            ("a.patches", r#"
                include "b.patches"
                patch { module x : X }
            "#),
            ("b.patches", r#"
                include "c.patches"
                template tb(x: float) { in: a out: b module m : M }
            "#),
            ("c.patches", r#"
                template tc(x: float) { in: a out: b module m : M }
            "#),
        ]);
        let result = load_with(Path::new("a.patches"), make_reader(files)).unwrap();
        assert_eq!(result.file.templates.len(), 2);
        assert_eq!(result.dependencies.len(), 3);
    }

    #[test]
    fn diamond_deduplication() {
        let files = StdHashMap::from([
            ("a.patches", r#"
                include "b.patches"
                include "c.patches"
                patch { module x : X }
            "#),
            ("b.patches", r#"
                include "d.patches"
                template tb(x: float) { in: a out: b module m : M }
            "#),
            ("c.patches", r#"
                include "d.patches"
                template tc(x: float) { in: a out: b module m : M }
            "#),
            ("d.patches", r#"
                template td(x: float) { in: a out: b module m : M }
            "#),
        ]);
        let result = load_with(Path::new("a.patches"), make_reader(files)).unwrap();
        // td from d, tb from b, tc from c — d loaded only once
        assert_eq!(result.file.templates.len(), 3);
        assert_eq!(result.dependencies.len(), 4);
    }

    #[test]
    fn cycle_detection() {
        let files = StdHashMap::from([
            ("a.patches", r#"
                include "b.patches"
                patch { module x : X }
            "#),
            ("b.patches", r#"
                include "a.patches"
                template tb(x: float) { in: a out: b module m : M }
            "#),
        ]);
        let err = load_with(Path::new("a.patches"), make_reader(files)).unwrap_err();
        assert!(matches!(err.kind, LoadErrorKind::Cycle { .. }), "expected cycle error, got: {err}");
    }

    #[test]
    fn self_include() {
        let files = StdHashMap::from([
            ("a.patches", r#"
                include "a.patches"
                patch { module x : X }
            "#),
        ]);
        let err = load_with(Path::new("a.patches"), make_reader(files)).unwrap_err();
        assert!(matches!(err.kind, LoadErrorKind::Cycle { .. }), "expected cycle error, got: {err}");
    }

    #[test]
    fn missing_file() {
        let files = StdHashMap::from([
            ("a.patches", r#"
                include "missing.patches"
                patch { module x : X }
            "#),
        ]);
        let err = load_with(Path::new("a.patches"), make_reader(files)).unwrap_err();
        assert!(matches!(err.kind, LoadErrorKind::Io { .. }), "expected IO error, got: {err}");
    }

    #[test]
    fn name_collision() {
        let files = StdHashMap::from([
            ("a.patches", r#"
                include "b.patches"
                template voice(x: float) { in: a out: b module m : M }
                patch { module x : X }
            "#),
            ("b.patches", r#"
                template voice(x: float) { in: a out: b module m : M }
            "#),
        ]);
        let err = load_with(Path::new("a.patches"), make_reader(files)).unwrap_err();
        assert!(
            matches!(&err.kind, LoadErrorKind::NameCollision { name, .. } if name == "voice"),
            "expected collision error for 'voice', got: {err}"
        );
    }

    #[test]
    fn include_file_with_patch_is_error() {
        let files = StdHashMap::from([
            ("a.patches", r#"
                include "b.patches"
                patch { module x : X }
            "#),
            ("b.patches", r#"
                patch { module x : X }
            "#),
        ]);
        let err = load_with(Path::new("a.patches"), make_reader(files)).unwrap_err();
        assert!(matches!(err.kind, LoadErrorKind::Parse { .. }), "expected parse error, got: {err}");
    }

    #[test]
    fn no_includes() {
        let files = StdHashMap::from([
            ("a.patches", r#"
                patch { module x : X }
            "#),
        ]);
        let result = load_with(Path::new("a.patches"), make_reader(files)).unwrap();
        assert!(result.file.includes.is_empty());
        assert_eq!(result.dependencies.len(), 1);
    }

    #[test]
    fn relative_path_resolution() {
        let files = StdHashMap::from([
            ("project/main.patches", r#"
                include "lib/voices.patches"
                patch { module x : X }
            "#),
            ("project/lib/voices.patches", r#"
                include "utils.patches"
                template voice(x: float) { in: a out: b module m : M }
            "#),
            ("project/lib/utils.patches", r#"
                template util(x: float) { in: a out: b module m : M }
            "#),
        ]);
        let result = load_with(Path::new("project/main.patches"), make_reader(files)).unwrap();
        assert_eq!(result.file.templates.len(), 2);
        assert_eq!(result.dependencies.len(), 3);
    }
}
