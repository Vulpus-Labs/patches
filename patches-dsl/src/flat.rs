use patches_core::QName;

use crate::ast::{Ident, Scalar, Step, Value};
use crate::provenance::Provenance;

/// Index of a pattern within [`SongData::patterns`].
///
/// Emitted by the expansion stage after all pattern names (template-qualified
/// or file-level) have been resolved. Interpreter consumers index directly
/// into the pattern list without rewalking song cells.
pub type PatternIdx = usize;

/// A concrete module instance with all template parameters resolved.
#[derive(Debug, Clone)]
pub struct FlatModule {
    /// Fully-qualified instance identifier (e.g. `QName::bare("osc")` or
    /// `QName { path: ["v1"], name: "osc" }`).
    pub id: QName,
    /// The module type name as it appears in the registry.
    pub type_name: String,
    /// Shape arguments (name, scalar value).
    pub shape: Vec<(String, Scalar)>,
    /// Initialisation parameters (name, value).
    pub params: Vec<(String, Value)>,
    /// Index → alias name for indexed ports declared via shape alias lists
    /// (e.g. `(channels: [drums, bass])`). Used by downstream diagnostics so
    /// "available ports" lists can show user-visible alias labels rather than
    /// raw numeric indices.
    pub port_aliases: Vec<(u32, String)>,
    /// Source provenance: innermost site (`provenance.site`) plus the chain of
    /// template call sites that led here.
    pub provenance: Provenance,
}

/// A concrete, fully resolved connection between two module ports.
#[derive(Debug, Clone)]
pub struct FlatConnection {
    pub from_module: QName,
    pub from_port: String,
    /// Port index; `0` for unindexed references.
    pub from_index: u32,
    pub to_module: QName,
    pub to_port: String,
    /// Port index; `0` for unindexed references.
    pub to_index: u32,
    /// Cable scale, composed from all template-boundary scales along the path.
    pub scale: f64,
    /// Source provenance for the whole connection (covers `lhs arrow rhs`).
    pub provenance: Provenance,
    /// Source provenance for the source side's authored port reference, used
    /// to tighten port-level diagnostics (e.g. `UnknownPort`) to just the
    /// offending `module.port` token instead of the whole connection line.
    pub from_provenance: Provenance,
    /// Source provenance for the destination side's authored port reference.
    pub to_provenance: Provenance,
}

/// Direction of a port reference recorded at a template boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortDirection {
    Input,
    Output,
}

/// A port reference made at a template boundary (`inner -> $.out` or
/// `$.in -> inner`) that may not survive into any [`FlatConnection`] — e.g.
/// if the enclosing scope never drives or consumes the template port. The
/// interpreter validates each ref against the target module's descriptor so
/// bogus port names are rejected even when the mapping is orphaned.
#[derive(Debug, Clone)]
pub struct FlatPortRef {
    pub module: QName,
    pub port: String,
    pub index: u32,
    pub direction: PortDirection,
    pub provenance: Provenance,
}

/// A pattern channel with slide generators expanded into concrete steps.
#[derive(Debug, Clone)]
pub struct FlatPatternChannel {
    pub name: String,
    pub steps: Vec<Step>,
}

/// A pattern definition with all generators expanded.
#[derive(Debug, Clone)]
pub struct FlatPatternDef {
    pub name: QName,
    pub channels: Vec<FlatPatternChannel>,
    pub provenance: Provenance,
}

/// One row of a resolved song: `None` cells are silences, `Some(idx)` cells
/// reference [`SongData::patterns`] by position.
#[derive(Debug, Clone)]
pub struct FlatSongRow {
    pub cells: Vec<Option<PatternIdx>>,
    pub provenance: Provenance,
}

/// A song definition with its name qualified by any enclosing scope and all
/// pattern references resolved to indices into [`SongData::patterns`].
///
/// Resolution happens in the expansion stage, so downstream consumers never
/// see raw [`SongCell`](crate::ast::SongCell) variants — in particular,
/// `ParamRef` cells cannot appear after expansion.
#[derive(Debug, Clone)]
pub struct FlatSongDef {
    pub name: QName,
    pub channels: Vec<Ident>,
    pub rows: Vec<FlatSongRow>,
    pub loop_point: Option<usize>,
    pub provenance: Provenance,
}

/// Graph-relevant half of a [`FlatPatch`]: concrete module instances,
/// connections between their ports, and template-boundary port refs.
///
/// `bind` operates on this type; [`SongData`] threads through the pipeline
/// unchanged.
#[derive(Debug, Clone, Default)]
pub struct FlatGraph {
    pub modules: Vec<FlatModule>,
    pub connections: Vec<FlatConnection>,
    /// Port references made at template boundaries that may have been dropped
    /// during flattening. Interpreter validates these against module
    /// descriptors so bogus port names are rejected even when orphaned.
    pub port_refs: Vec<FlatPortRef>,
}

/// Tracker half of a [`FlatPatch`]: pattern and song definitions, used by
/// the interpreter to build [`patches_core::TrackerData`] for sequencer
/// modules. Threaded through bind unchanged.
#[derive(Debug, Clone, Default)]
pub struct SongData {
    /// Pattern definitions with slide generators expanded.
    pub patterns: Vec<FlatPatternDef>,
    /// Song definitions (names qualified under any enclosing template scope).
    pub songs: Vec<FlatSongDef>,
}

/// A flat, template-free description of a patch.
///
/// This is the output of the template expander (Stage 2) and the input to the
/// graph builder (Stage 3). It contains only concrete module instances and
/// port-to-port connections — no template declarations, no `$`-prefixed
/// references. The patch decomposes into a [`FlatGraph`] (graph topology)
/// and a [`SongData`] (tracker data); `bind` operates on the former and
/// threads the latter through unchanged.
#[derive(Debug, Clone, Default)]
pub struct FlatPatch {
    pub graph: FlatGraph,
    pub song_data: SongData,
}

impl std::ops::Deref for FlatPatch {
    type Target = FlatGraph;
    fn deref(&self) -> &FlatGraph {
        &self.graph
    }
}

impl std::ops::DerefMut for FlatPatch {
    fn deref_mut(&mut self) -> &mut FlatGraph {
        &mut self.graph
    }
}
