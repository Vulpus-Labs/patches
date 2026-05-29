//! Canonicalizing golden harness tests (ADR 0079 §4, ticket 0964).
//!
//! `voice_template` is the proof golden — the same fixture
//! `patches-dsl/tests/expand/templates.rs` exercises. The span-stability test
//! proves the harness's central claim: an edit that shifts byte offsets but not
//! graph structure produces no golden diff.

use patches_graph_json::assert_graph_golden;
use patches_graph_json::golden::doc_from_src;
use patches_graph_json::schema::{GraphDoc, Span};
use patches_graph_json::to_json_pretty;

const VOICE_TEMPLATE: &str =
    include_str!("../../patches-dsl/tests/fixtures/voice_template.patches");

#[test]
fn voice_template_golden() {
    assert_graph_golden!("voice_template", VOICE_TEMPLATE);
}

// ─── Migrated structural slice-asserts (ticket 0965, ADR 0079 §4) ────────────
//
// Each golden below replaces the hand-written FlatPatch slice-asserts in
// `patches-dsl/tests/expand/{templates,arity}.rs` for the same fixture. One
// golden per fixture captures every slice the targeted asserts checked
// (module namespacing, internal/boundary connections, composed scales,
// resolved params/defaults, shape args, group params, arity expansion counts,
// provenance structure) at once. The fixtures are shared with patches-dsl via
// `include_str!`. AST/parse-level, error-path, and a few negative-intent
// asserts stay targeted in patches-dsl (ADR 0079 §4).

/// Nested template expansion: `filtered_voice` instantiates `voice`. Captures
/// the namespaced module set (`fv/v/osc`, `fv/filt`, …; no `fv`/`fv/v`),
/// boundary rewiring through two template levels, and the inner connection.
#[test]
fn nested_templates_golden() {
    assert_graph_golden!(
        "nested_templates",
        include_str!("../../patches-dsl/tests/fixtures/nested_templates.patches"),
    );
}

/// `[*n]` arity fan-out (`size: 3`): three indexed connections into the inner
/// `Sum`, one per index.
#[test]
fn bus_size_3_golden() {
    assert_graph_golden!(
        "bus_size_3",
        include_str!("../../patches-dsl/tests/fixtures/bus_size_3.patches"),
    );
}

/// `[*n]` boundary fan-out inside a template (`size: 4`): four distinct
/// connections from the template boundary into the inner mixer.
#[test]
fn fan_size_4_golden() {
    assert_graph_golden!(
        "fan_size_4",
        include_str!("../../patches-dsl/tests/fixtures/fan_size_4.patches"),
    );
}

/// `[k]` param-index (`channel: 2`): a single connection landing at index 2.
#[test]
fn channel_indexed_golden() {
    assert_graph_golden!(
        "channel_indexed",
        include_str!("../../patches-dsl/tests/fixtures/channel_indexed.patches"),
    );
}

/// Scale composition under arity (`size: 2`, scale 0.5): each expanded
/// connection carries the composed scale independently.
#[test]
fn scaled_fan_size_2_golden() {
    assert_graph_golden!(
        "scaled_fan_size_2",
        include_str!("../../patches-dsl/tests/fixtures/scaled_fan_size_2.patches"),
    );
}

/// Group-param broadcast: `level: 0.8` over a `level[size]` group fills
/// `level/0..2` with 0.8.
#[test]
fn levelled_broadcast_golden() {
    assert_graph_golden!(
        "levelled_broadcast",
        include_str!("../../patches-dsl/tests/fixtures/levelled_broadcast.patches"),
    );
}

/// Group-param per-index: `level[0]: 0.8, level[1]: 0.3`; the unset slot falls
/// back to the declared default 1.0.
#[test]
fn levelled_per_index_golden() {
    assert_graph_golden!(
        "levelled_per_index",
        include_str!("../../patches-dsl/tests/fixtures/levelled_per_index.patches"),
    );
}

/// LimitedMixer end-to-end (ADR 0019): inner `Sum`, three in-connections, one
/// out, per-index levels.
#[test]
fn limited_mixer_golden() {
    assert_graph_golden!(
        "limited_mixer",
        include_str!("../../patches-dsl/tests/fixtures/limited_mixer.patches"),
    );
}

/// Zero arity (`[*0]`): expands to no connections into the inner module, with
/// no error. The empty connection set is the regression surface.
#[test]
fn zero_arity_golden() {
    assert_graph_golden!(
        "zero_arity",
        include_str!("../../patches-dsl/tests/fixtures/zero_arity.patches"),
    );
}

/// Shape-arg substitution: `Sum(channels: <size>)` with `size: 4` resolves the
/// shape arg to `channels: 4`.
#[test]
fn bus_shape_arg_golden() {
    assert_graph_golden!(
        "bus_shape_arg",
        include_str!("../../patches-dsl/tests/fixtures/bus_shape_arg.patches"),
    );
}

/// Provenance structure: a two-level template nesting produces a two-entry
/// expansion chain on the inner `Gain`. Span *values* are redacted, but the
/// chain *length* is preserved (each element redacted individually).
#[test]
fn provenance_two_level_golden() {
    assert_graph_golden!(
        "provenance_two_level",
        include_str!("../../patches-dsl/tests/fixtures/provenance_two_level.patches"),
    );
}

/// Spans are the one volatile part of the emitter output. `insta` redacts them
/// to a constant `"[span]"` placeholder, so any change confined to byte offsets
/// cannot alter the golden. This demonstrates that invariant directly:
/// prepending comment lines shifts every offset, yet the canonical doc with
/// spans flattened to a constant serializes identically.
#[test]
fn byte_span_shift_produces_no_golden_diff() {
    let shifted = format!("# extra comment line, shifts every byte offset\n#\n{VOICE_TEMPLATE}");

    let base = flatten_spans(doc_from_src(VOICE_TEMPLATE).expect("base builds"));
    let moved = flatten_spans(doc_from_src(&shifted).expect("shifted builds"));

    // Sanity: the shift really did move spans (otherwise the test is vacuous).
    let raw_base = doc_from_src(VOICE_TEMPLATE).expect("base builds");
    let raw_moved = doc_from_src(&shifted).expect("shifted builds");
    assert_ne!(
        to_json_pretty(&raw_base).unwrap(),
        to_json_pretty(&raw_moved).unwrap(),
        "expected raw spans to differ before flattening"
    );

    assert_eq!(
        to_json_pretty(&base).unwrap(),
        to_json_pretty(&moved).unwrap(),
        "graph differs once spans are flattened — structure changed, not just offsets"
    );
}

/// Set every span to a constant, mirroring what `insta`'s `"[span]"` redaction
/// does for equality purposes (collapses all span values to one placeholder).
fn flatten_spans(mut doc: GraphDoc) -> GraphDoc {
    const ZERO: Span = Span { source: 0, start: 0, end: 0 };
    for m in &mut doc.modules {
        m.provenance.site = ZERO;
        m.provenance.expansion.iter_mut().for_each(|s| *s = ZERO);
    }
    for c in &mut doc.connections {
        for p in [&mut c.provenance, &mut c.from_provenance, &mut c.to_provenance] {
            p.site = ZERO;
            p.expansion.iter_mut().for_each(|s| *s = ZERO);
        }
    }
    doc
}
