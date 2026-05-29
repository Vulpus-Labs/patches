//! Canonicalizing golden harness for the patch-graph JSON surface (ADR 0079
//! §4, ticket 0964).
//!
//! The emitter ([`crate::emit`]) carries **full** provenance — raw byte spans
//! are load-bearing for the diagram/debug surface. Goldens, however, must be
//! stable under fixture edits that shift byte offsets without changing graph
//! structure. The split (ADR 0079 §4) is:
//!
//! - **Ordering + float normalization happen here, in the serializer** that
//!   feeds the snapshot ([`canonical_doc`]): modules sorted by id, connections
//!   sorted by their port-ref tuple, and every float rounded to `1e-12` so
//!   composed-scale representation noise (e.g. `0.5 * 0.8`) does not churn.
//! - **Span redaction happens in `insta`**, via redaction *paths*, not here —
//!   see [`assert_graph_golden!`]. The field is retained, the value becomes
//!   `"[span]"`, so the golden still proves provenance exists where expected;
//!   the expansion-chain *structure* (array length) is preserved because each
//!   element is redacted individually rather than the array being collapsed.
//!
//! Float policy: **round-before-serialize** to a `1e-12` quantum (chosen over a
//! numeric epsilon compare so the stored golden is itself stable and diffable).

use patches_manifest::bundled_manifest;
use patches_manifest::static_registry::registry_from_manifest;

use crate::emit::graph_doc;
use crate::schema::{CableMap, GraphDoc, Module, Scalar, Value};

/// Reciprocal of the `1e-12` float tolerance: rounding to this quantum makes
/// scale/param floats that agree within `1e-12` serialize identically.
const FLOAT_QUANTUM: f64 = 1e12;

fn round12(x: f64) -> f64 {
    (x * FLOAT_QUANTUM).round() / FLOAT_QUANTUM
}

/// Parse + expand `src`, emit the graph doc against the bundled manifest, and
/// canonicalize it ([`canonical_doc`]). Spans are left intact — `insta`
/// redacts them at compare time. Parse/expand failures return the diagnostic
/// string rather than panicking.
pub fn doc_from_src(src: &str) -> Result<GraphDoc, String> {
    let file = patches_dsl::parse(src).map_err(|e| e.to_string())?;
    let expanded = patches_dsl::expand(&file).map_err(|e| e.to_string())?;
    let registry = registry_from_manifest(bundled_manifest());
    Ok(canonical_doc(&graph_doc(&expanded.patch, &registry)))
}

/// Return a canonicalized clone: modules sorted by id, connections sorted by
/// `(from, from_port, from_index, to, to_port, to_index)`, all floats rounded
/// to the `1e-12` quantum. Spans are untouched (redacted by `insta` at compare
/// time, ADR 0079 §4).
pub fn canonical_doc(doc: &GraphDoc) -> GraphDoc {
    let mut doc = doc.clone();

    for m in &mut doc.modules {
        round_module_floats(m);
    }
    doc.modules.sort_by(|a, b| a.id.cmp(&b.id));

    for c in &mut doc.connections {
        round_map(&mut c.map);
    }
    doc.connections.sort_by_key(connection_key);

    doc
}

type ConnectionKey = (String, String, u32, String, String, u32);

fn connection_key(c: &crate::schema::Connection) -> ConnectionKey {
    (
        c.from.module.clone(),
        c.from.port.clone(),
        c.from.index,
        c.to.module.clone(),
        c.to.port.clone(),
        c.to.index,
    )
}

fn round_module_floats(m: &mut Module) {
    for arg in &mut m.shape {
        round_scalar(&mut arg.value);
    }
    for param in &mut m.params {
        round_value(&mut param.value);
    }
}

fn round_value(v: &mut Value) {
    if let Value::Scalar { scalar } = v {
        round_scalar(scalar);
    }
}

fn round_scalar(s: &mut Scalar) {
    if let Scalar::Float { value } = s {
        *value = round12(*value);
    }
}

fn round_map(map: &mut CableMap) {
    map.scale = round12(map.scale);
    map.offset = round12(map.offset);
    if let Some(clip) = &mut map.clip {
        clip.min = round12(clip.min);
        clip.max = round12(clip.max);
    }
}

/// Assert a canonicalized graph golden for a `.patches` source string.
///
/// Adding a fixture golden is: drop a `.patches` file, write one
/// `assert_graph_golden!("name", include_str!("fixtures/name.patches"))`, run
/// with `INSTA_UPDATE=always` (or `cargo insta review`), eyeball, commit.
///
/// Spans are redacted to `"[span]"` via `insta` redaction *paths* — the field
/// stays, the volatile byte value goes, and each `expansion` element is
/// redacted individually so the chain length is preserved (ADR 0079 §4).
///
/// Requires the caller to have `insta` (features `json`, `redactions`) in
/// scope.
#[macro_export]
macro_rules! assert_graph_golden {
    ($name:expr, $src:expr $(,)?) => {{
        let doc = $crate::golden::doc_from_src($src).expect("build graph doc");
        insta::assert_json_snapshot!($name, &doc, {
            ".**.site" => "[span]",
            ".**.expansion[]" => "[span]",
        });
    }};
}
