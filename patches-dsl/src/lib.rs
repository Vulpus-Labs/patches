//! `patches-dsl` — parser and template expander for the Patches DSL.
//!
//! # Pipeline
//!
//! ```text
//! .patches source text
//!     │
//!     ▼  Stage 1 — PEG parser
//! AST  (spans preserved; no semantic analysis)
//!     │
//!     ▼  Stage 2 — Expander
//! FlatPatch  (templates inlined; only concrete instances and edges remain)
//! ```
//!
//! The resulting `FlatPatch` is handed to `patches-interpreter` which
//! validates it against the module registry and constructs a `ModuleGraph`.
//!
//! This crate has no knowledge of concrete module types and no audio-backend
//! dependencies.

pub mod ast;
mod expand;
pub mod flat;
mod parser;

pub mod include_frontier;
pub mod loader;

pub use ast::{
    Arrow, AtBlockIndex, Connection, Direction, File, Ident, IncludeDirective, IncludeFile,
    ModuleDecl, ParamDecl, ParamEntry, ParamIndex, ParamType, Patch, PatternChannel, PatternDef,
    PortGroupDecl, PortIndex, PortLabel, PortRef, Scalar, ShapeArg, ShapeArgValue, SongCell,
    SongDef, SongRow, Span, Statement, Step, StepOrGenerator, Template, Value,
};
pub use expand::{expand, ExpandError, ExpandResult, Warning};
pub use flat::{FlatConnection, FlatModule, FlatPatch, FlatPatternChannel, FlatPatternDef, FlatSongDef};
pub use patches_core::QName;
pub use include_frontier::{normalize_path, EnterResult, IncludeFrontier};
pub use loader::{load_with, LoadError, LoadErrorKind, LoadResult};
pub use parser::{parse, parse_include_file, ParseError};
