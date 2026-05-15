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
    /// Source span covering the authored `{ ... }` parameter block, when
    /// present. Diagnostics like `UnknownParameter` use this to widen
    /// the search scope for the offending key beyond `provenance.site`
    /// (which is intentionally narrow — `name : type_name`).
    pub param_block_span: Option<patches_core::source_span::Span>,
    /// Source span covering just the `type_name` identifier on the RHS of
    /// `name : type_name`. Used to narrow the `UnknownModuleType` squiggle
    /// to the offending type token, rather than `provenance.site` which
    /// covers the wider `name : type_name` range.
    pub type_name_span: Option<patches_core::source_span::Span>,
    /// Index → alias name for indexed ports declared via shape alias lists
    /// (e.g. `(channels: [drums, bass])`). Used by downstream diagnostics so
    /// "available ports" lists can show user-visible alias labels rather than
    /// raw numeric indices.
    pub port_aliases: Vec<(u32, String)>,
    /// Source provenance: innermost site (`provenance.site`) plus the chain of
    /// template call sites that led here.
    pub provenance: Provenance,
}

/// Affine map plus optional hard clip applied to a cable signal.
///
/// Pure-scalar cables produce `CableMap::scalar(k)`
/// (`offset = 0`, `clip = None`) so downstream code can pattern-match
/// the fast path. Range cables (`uni` / `bi`) lower to a non-zero
/// `offset` and a `Some(clip)` per ADR 0062.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CableMap {
    pub scale: f64,
    pub offset: f64,
    /// Hard clip range, sorted as `(min, max)`. Inverted (min > max)
    /// representations are produced by the composition algebra when
    /// two clip ranges have empty intersection; downstream code
    /// treats that as "always clipped".
    pub clip: Option<(f64, f64)>,
}

impl CableMap {
    /// Pure scalar (no offset, no clip) — the fast path.
    pub const fn scalar(k: f64) -> Self {
        Self { scale: k, offset: 0.0, clip: None }
    }

    /// `scale = 1`, no offset, no clip.
    pub const fn identity() -> Self {
        Self::scalar(1.0)
    }

    /// Whether this map is the pure-scalar fast path (`offset == 0`,
    /// `clip == None`). Used by downstream consumers to skip the
    /// affine + clip work.
    pub fn is_scalar(&self) -> bool {
        self.offset == 0.0 && self.clip.is_none()
    }

    /// Compose two segments where `s1` is applied first (closer to
    /// source) and `s2` second (closer to destination). Per ADR 0062:
    ///
    /// ```text
    /// gain   = s2.gain * s1.gain
    /// offset = s2.gain * s1.offset + s2.offset
    /// clip   = intersect(map_forward(s1.clip, s2), s2.clip)
    /// ```
    pub fn compose(s1: &Self, s2: &Self) -> Self {
        let scale = s2.scale * s1.scale;
        let offset = s2.scale * s1.offset + s2.offset;
        let s1_mapped = s1.clip.map(|(a, b)| {
            let a2 = s2.scale * a + s2.offset;
            let b2 = s2.scale * b + s2.offset;
            if a2 <= b2 { (a2, b2) } else { (b2, a2) }
        });
        let clip = match (s1_mapped, s2.clip) {
            (None, None) => None,
            (Some(c), None) | (None, Some(c)) => Some(c),
            (Some((a1, b1)), Some((a2, b2))) => Some((a1.max(a2), b1.min(b2))),
        };
        Self { scale, offset, clip }
    }
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
    /// Affine + clip composed from all template-boundary scales along
    /// the path. Pure-scalar cables retain `CableMap::scalar(k)`.
    pub map: CableMap,
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

#[cfg(test)]
mod cable_map_tests {
    use super::CableMap;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-12, "{} vs {}", a, b);
    }

    #[test]
    fn scalar_compose_scalar_is_multiplication() {
        let c = CableMap::compose(&CableMap::scalar(0.5), &CableMap::scalar(2.0));
        approx(c.scale, 1.0);
        approx(c.offset, 0.0);
        assert_eq!(c.clip, None);
        assert!(c.is_scalar());
    }

    #[test]
    fn scalar_compose_range_carries_clip_and_offset() {
        // Inner scalar k=2.0 followed by outer uni(0,1)-style affine.
        let inner = CableMap::scalar(2.0);
        let outer = CableMap { scale: 1.0, offset: 0.5, clip: Some((0.5, 1.5)) };
        let c = CableMap::compose(&inner, &outer);
        approx(c.scale, 2.0);
        approx(c.offset, 0.5);
        let (lo, hi) = c.clip.unwrap();
        approx(lo, 0.5);
        approx(hi, 1.5);
    }

    #[test]
    fn range_compose_scalar_forward_maps_inner_clip() {
        // Inner range [-1, 1] → outer ×0.5 should clip to [-0.5, 0.5].
        let inner = CableMap { scale: 1.0, offset: 0.0, clip: Some((-1.0, 1.0)) };
        let outer = CableMap::scalar(0.5);
        let c = CableMap::compose(&inner, &outer);
        approx(c.scale, 0.5);
        approx(c.offset, 0.0);
        let (lo, hi) = c.clip.unwrap();
        approx(lo, -0.5);
        approx(hi, 0.5);
    }

    #[test]
    fn range_compose_range_intersects_after_forward_map() {
        // Inner clip (0, 4) mapped through outer (×0.5, +0) → (0, 2);
        // intersect with outer clip (1, 3) → (1, 2).
        let inner = CableMap { scale: 1.0, offset: 0.0, clip: Some((0.0, 4.0)) };
        let outer = CableMap { scale: 0.5, offset: 0.0, clip: Some((1.0, 3.0)) };
        let c = CableMap::compose(&inner, &outer);
        let (lo, hi) = c.clip.unwrap();
        approx(lo, 1.0);
        approx(hi, 2.0);
    }

    #[test]
    fn inverted_range_endpoints_sort_clip() {
        // bi(1, -1) — lo > hi at the source. After lowering the clip
        // is sorted; the affine still encodes the user's polarity.
        let inner = CableMap {
            scale: -1.0, // = (hi - lo) / 2 with lo=1, hi=-1
            offset: 0.0,
            clip: Some((-1.0, 1.0)),
        };
        // Forward through ×2 — clip remains sorted post-map.
        let c = CableMap::compose(&inner, &CableMap::scalar(2.0));
        let (lo, hi) = c.clip.unwrap();
        assert!(lo <= hi, "post-compose clip must stay sorted: {lo} {hi}");
    }

    #[test]
    fn degenerate_uni_collapses_to_point() {
        // uni(k, k) → scale = 0, offset = k, clip = (k, k).
        let m = CableMap { scale: 0.0, offset: 0.7, clip: Some((0.7, 0.7)) };
        let c = CableMap::compose(&CableMap::scalar(1.0), &m);
        approx(c.scale, 0.0);
        approx(c.offset, 0.7);
        let (lo, hi) = c.clip.unwrap();
        approx(lo, 0.7);
        approx(hi, 0.7);
    }
}
