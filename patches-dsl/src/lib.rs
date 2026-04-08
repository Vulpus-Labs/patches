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

pub use ast::{
    Arrow, AtBlockIndex, Connection, Direction, File, Ident, ModuleDecl, ParamDecl,
    ParamEntry, ParamIndex, ParamType, Patch, PortGroupDecl, PortIndex, PortLabel, PortRef, Scalar,
    ShapeArg, ShapeArgValue, Span, Statement, Template, Value,
};
pub use expand::{expand, ExpandError, ExpandResult, Warning};
pub use flat::{FlatConnection, FlatModule, FlatPatch};
pub use parser::{parse, ParseError};
