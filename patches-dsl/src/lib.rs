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
pub mod structural;
pub mod desugar;
pub mod manifest;
pub mod tap_schema;
pub mod validate;

// Provenance, SourceId, Span, and SourceMap are owned by `patches-core` so
// they can appear in core types like `BuildError`. Re-exported here for the
// historical paths.
pub use patches_core::provenance;
pub use patches_core::source_map;

pub mod include_frontier;
pub mod loader;
pub mod pipeline;

pub use ast::{
    Arrow, AtBlockIndex, Connection, Direction, File, Ident, IncludeDirective, IncludeFile,
    ModuleDecl, ParamDecl, ParamEntry, ParamIndex, ParamType, Patch, PatternChannel, PatternDef,
    PlayAtom, PlayBody, PlayExpr, PlayTerm, PortGroupDecl, PortIndex, PortLabel, PortRef,
    RowGroup, Scalar, SectionDef, ShapeArg, ShapeArgValue, SongCell, SongDef, SongItem, SongRow,
    SourceId, Span, Statement, Step, StepOrGenerator, Template, Value,
};
pub use expand::{expand, ExpandError, ExpandResult, Warning};
pub use structural::{StructuralCode, StructuralError};
pub use flat::{
    FlatConnection, FlatGraph, FlatModule, FlatPatch, FlatPatternChannel, FlatPatternDef,
    FlatPortRef, FlatSongDef, FlatSongRow, PatternIdx, PortDirection, SongData,
};
pub use patches_core::QName;
pub use include_frontier::{normalize_path, EnterResult, IncludeFrontier};
pub use loader::{load_with, LoadError, LoadErrorKind, LoadResult};
pub use parser::{
    parse, parse_include_file, parse_include_file_with_source, parse_with_source, ParseError,
};
pub use provenance::Provenance;
pub use source_map::{line_col, SourceEntry, SourceMap};
