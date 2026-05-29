//! Serde "mirror" structs for the patch-graph JSON surface (ADR 0079 §2).
//!
//! These types are a deliberate copy of the relevant shapes from
//! `patches_dsl` / `patches_core`, decoupled so the external JSON schema can
//! version independently of the internal `FlatPatch` representation and the
//! DSL crate stays serde-free. Conversion from the source types lives in
//! [`crate::emit`].

use serde::Serialize;

/// Schema version. Bump deliberately on incompatible changes; the
/// compatibility policy itself is deferred (ADR 0079 Open question 3).
pub const SCHEMA_VERSION: u32 = 1;

/// The whole document: the expanded, port-kind-resolved patch graph.
#[derive(Debug, Clone, Serialize)]
pub struct GraphDoc {
    pub version: u32,
    pub modules: Vec<Module>,
    pub connections: Vec<Connection>,
}

/// A concrete module instance (mirror of [`patches_dsl::FlatModule`]).
#[derive(Debug, Clone, Serialize)]
pub struct Module {
    /// Slash-joined display id (`QName::to_string`), e.g. `v1/osc`.
    pub id: String,
    /// Structured form of the id (namespace path + bare name).
    pub qname: QName,
    pub type_name: String,
    pub shape: Vec<ShapeArg>,
    pub params: Vec<Param>,
    pub port_aliases: Vec<PortAlias>,
    /// Descriptor ports resolved against the bundled manifest. `None` when
    /// the module type is unknown to the manifest — graceful degradation
    /// (unclassified), never an error.
    pub ports: Option<Ports>,
    pub provenance: Provenance,
}

/// Structured qualified identifier (mirror of [`patches_core::QName`]).
#[derive(Debug, Clone, Serialize)]
pub struct QName {
    pub path: Vec<String>,
    pub name: String,
}

/// A shape argument (mirror of one `(name, Scalar)` entry).
#[derive(Debug, Clone, Serialize)]
pub struct ShapeArg {
    pub name: String,
    pub value: Scalar,
}

/// An init parameter (mirror of one `(name, Value)` entry).
#[derive(Debug, Clone, Serialize)]
pub struct Param {
    pub name: String,
    pub value: Value,
}

/// A scalar literal (mirror of [`patches_dsl::Scalar`]).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Scalar {
    Int { value: i64 },
    Float { value: f64 },
    Bool { value: bool },
    Str { value: String },
    /// A `<ident>` template-parameter reference. Should not survive expansion
    /// for resolved params, but mirrored faithfully if it appears.
    ParamRef { name: String },
}

/// A param-block value (mirror of [`patches_dsl::ast::Value`]).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Value {
    Scalar { scalar: Scalar },
    /// `file("path")` — raw, unresolved path from source.
    File { path: String },
}

/// An index → alias-name mapping for an indexed port.
#[derive(Debug, Clone, Serialize)]
pub struct PortAlias {
    pub index: u32,
    pub name: String,
}

/// Resolved descriptor ports for a module.
#[derive(Debug, Clone, Serialize)]
pub struct Ports {
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
}

/// A single port with its resolved cable kind / layouts (mirror of
/// [`patches_core::PortDescriptor`]).
#[derive(Debug, Clone, Serialize)]
pub struct Port {
    pub name: String,
    pub index: u32,
    pub kind: CableKind,
    pub mono_layout: MonoLayout,
    pub poly_layout: PolyLayout,
}

/// Cable arity (mirror of [`patches_core::cables::CableKind`]).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CableKind {
    Mono,
    Poly,
    Stereo,
}

/// Mono cable layout (mirror of [`patches_core::cables::MonoLayout`]).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MonoLayout {
    Audio,
    Trigger,
}

/// Poly cable layout (mirror of [`patches_core::cables::PolyLayout`]).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolyLayout {
    Audio,
    Trigger,
    Transport,
    Midi,
}

/// A resolved connection (mirror of [`patches_dsl::FlatConnection`]).
#[derive(Debug, Clone, Serialize)]
pub struct Connection {
    pub from: PortRef,
    pub to: PortRef,
    pub map: CableMap,
    pub provenance: Provenance,
    pub from_provenance: Provenance,
    pub to_provenance: Provenance,
}

/// One side of a connection: module id + port name + index.
#[derive(Debug, Clone, Serialize)]
pub struct PortRef {
    /// Slash-joined display id of the module.
    pub module: String,
    pub port: String,
    pub index: u32,
}

/// Affine + clip map on a cable (mirror of [`patches_dsl::CableMap`]).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct CableMap {
    pub scale: f64,
    pub offset: f64,
    /// Hard clip range `[min, max]`, sorted. Absent for pure-scalar cables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip: Option<Clip>,
}

/// A hard-clip range.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Clip {
    pub min: f64,
    pub max: f64,
}

/// Full source provenance (mirror of [`patches_core::Provenance`]).
///
/// Spans carry raw byte offsets, verbatim — load-bearing for the diagram /
/// debug surface (hover, click-to-source). Volatility is a *comparison*
/// problem; the golden harness (ticket 0964) redacts at compare time, the
/// emitter does not strip (ADR 0079 §4).
#[derive(Debug, Clone, Serialize)]
pub struct Provenance {
    pub site: Span,
    pub expansion: Vec<Span>,
}

/// A byte-offset span into a source file (mirror of
/// [`patches_core::source_span::Span`]).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Span {
    /// `SourceId` raw value; `0` is the synthetic sentinel.
    pub source: u32,
    pub start: usize,
    pub end: usize,
}
