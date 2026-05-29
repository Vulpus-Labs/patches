//! `patches-graph-json` — serialize the expanded, port-kind-resolved patch
//! graph to JSON (ADR 0079, Phase 1 / Epic E157).
//!
//! The schema is defined as serde "mirror" structs ([`schema`]) decoupled from
//! `patches_dsl::FlatPatch` so the external contract versions independently and
//! the leaf DSL crate stays serde-free. The emitter ([`emit`]) maps a
//! `FlatPatch` plus manifest-resolved port kinds (full provenance included)
//! into that schema.
//!
//! Pipeline (mirrors `patches-svg`): `patches_dsl::load_with` →
//! `patches_dsl::expand` → [`graph_doc`]. No interpreter pass, so partial /
//! invalid patches still emit; unknown module types degrade to unclassified
//! ports rather than erroring.

pub mod emit;
pub mod golden;
pub mod schema;

pub use emit::graph_doc;
pub use schema::{GraphDoc, SCHEMA_VERSION};

/// Serialize a document as pretty-printed JSON with a trailing newline.
pub fn to_json_pretty(doc: &GraphDoc) -> serde_json::Result<String> {
    let mut s = serde_json::to_string_pretty(doc)?;
    s.push('\n');
    Ok(s)
}

/// Serialize a document as compact JSON.
pub fn to_json(doc: &GraphDoc) -> serde_json::Result<String> {
    serde_json::to_string(doc)
}
