//! Completion engine for the patches DSL.
//!
//! Provides context-sensitive completions for module types, parameters,
//! ports, shape arguments, and template ports.
//!
//! # Two dispatch paths
//!
//! Completions flow through two distinct paths:
//!
//! 1. **Parsed-input dispatch** — `classify_cursor` returns a
//!    [`crate::tree_nav::CursorContext`] built from the tree-sitter parse
//!    tree. [`compute_completions`] matches on the variant and hands off to
//!    the appropriate completer. This covers every case where tree-sitter
//!    produced a usable node (including error-recovered nodes that still
//!    carry structure, like an empty type slot).
//!
//! 2. **Incomplete-input fallback** — [`scan_backward_for_context`] inspects
//!    the source text behind the cursor when tree-sitter has no node (e.g.
//!    `osc.` with the cursor just past the dot, or `$.` inside a template
//!    before a port identifier exists). Folding this into the classifier
//!    would require either ERROR-node text inspection or richer tree-sitter
//!    error recovery; both are out of scope for E084.

mod backward_scan;
mod module_types;
mod params;
mod ports;
mod shape;
mod tap;

use patches_registry::Registry;
use tower_lsp::lsp_types::*;
use tree_sitter::Tree;

use crate::analysis::SemanticModel;
use crate::tree_nav::{
    classify_cursor, is_master_sequencer, module_instance_name, CursorContext,
};

use backward_scan::{scan_backward_for_context, BackwardContext};
use module_types::{complete_module_types, complete_statement_keywords};
use params::{
    complete_enum_values, complete_parameters, complete_pattern_names, complete_song_names,
    is_after_param_colon, preceding_param_name,
};
use ports::{complete_port_ref, complete_ports, complete_template_ports};
use shape::{
    complete_at_block_aliases, complete_port_index_aliases, complete_shape_args, is_after_at_sign,
};

/// Determine the completion context from the cursor position and return items.
pub(crate) fn compute_completions(
    tree: &Tree,
    source: &str,
    byte_offset: usize,
    model: &SemanticModel,
    registry: &Registry,
) -> Vec<CompletionItem> {
    // Tap-context fast path: `~|`, `~comp+|`, and `~...(|` aren't reliably
    // classifiable by tree-sitter (they live in ERROR-recovered nodes), so
    // dispatch via a textual scan first.
    if let Some(ctx) = tap::scan_tap_context(source, byte_offset) {
        return match ctx {
            tap::TapCursor::AfterTilde => tap::complete_components_initial(),
            tap::TapCursor::AfterPlus { listed } => tap::complete_components_continuing(&listed),
        };
    }

    match classify_cursor(tree, byte_offset) {
        CursorContext::ModuleType { .. } | CursorContext::ModuleTypeSlot { .. } => {
            return complete_module_types(model, registry);
        }
        CursorContext::ParamBlock { module_decl, .. } => {
            return complete_param_block(module_decl, source, byte_offset, model);
        }
        CursorContext::ShapeBlock { .. } => {
            return complete_shape_args();
        }
        CursorContext::PortRef { port_ref_node, .. } => {
            return complete_port_ref(source, byte_offset, port_ref_node, tree, model);
        }
        CursorContext::SongRow { .. } => {
            return complete_pattern_names(model);
        }
        CursorContext::ModuleName { .. }
        | CursorContext::PortIndex { .. }
        | CursorContext::TapType { .. }
        | CursorContext::TapName { .. }
        | CursorContext::HostControlDecl { .. }
        | CursorContext::HostControlRef { .. }
        | CursorContext::Unknown => {}
    }

    // Incomplete-input fallback: tree-sitter produced no classifiable node
    // for this offset. Use the textual backward scanner.
    if let Some(ctx) = scan_backward_for_context(source, byte_offset) {
        return match ctx {
            BackwardContext::ModuleColon | BackwardContext::ModuleTypeName => {
                complete_module_types(model, registry)
            }
            BackwardContext::Dot(module_name) => {
                complete_ports(&module_name, model, byte_offset, source, tree)
            }
            BackwardContext::DollarDot => complete_template_ports(source, byte_offset, tree, model),
            BackwardContext::PortIndex(module_name) => {
                complete_port_index_aliases(&module_name, model)
            }
            BackwardContext::SongRow => complete_pattern_names(model),
            BackwardContext::StatementStart => complete_statement_keywords(),
        };
    }

    vec![]
}

/// Sub-classification within a [`CursorContext::ParamBlock`]: drives the
/// dispatch in [`complete_param_block`] without re-running the probes
/// at every call site or in every reader's head.
enum ParamBlockTarget<'tree, 'src> {
    /// Cursor is just past a `@` — completing shape-alias names.
    AtBlockAlias { module_decl: tree_sitter::Node<'tree> },
    /// Cursor is in a MasterSequencer's `song:` slot — song names only.
    MasterSequencerSong,
    /// Cursor is just past a `param_name:` — completing values for the
    /// named parameter (enum variants, etc.).
    ParamValue { param_name: String, module_name: Option<&'src str> },
    /// Plain parameter-name completion inside the block.
    ParamName { module_name: Option<&'src str> },
}

fn classify_param_block<'tree, 'src>(
    module_decl: tree_sitter::Node<'tree>,
    source: &'src str,
    byte_offset: usize,
) -> ParamBlockTarget<'tree, 'src> {
    if is_after_at_sign(source, byte_offset) {
        return ParamBlockTarget::AtBlockAlias { module_decl };
    }
    if is_master_sequencer(module_decl, source)
        && is_after_param_colon(source, byte_offset, "song")
    {
        return ParamBlockTarget::MasterSequencerSong;
    }
    let module_name = module_instance_name(module_decl, source);
    if let Some(param_name) = preceding_param_name(source, byte_offset) {
        return ParamBlockTarget::ParamValue { param_name, module_name };
    }
    ParamBlockTarget::ParamName { module_name }
}

/// Dispatch `ParamBlock`-kind completions: shape-alias list (`@`),
/// MasterSequencer `song:` slot, enum-value lookup, or parameter names.
fn complete_param_block(
    module_decl: tree_sitter::Node,
    source: &str,
    byte_offset: usize,
    model: &SemanticModel,
) -> Vec<CompletionItem> {
    match classify_param_block(module_decl, source, byte_offset) {
        ParamBlockTarget::AtBlockAlias { module_decl } => {
            complete_at_block_aliases(module_decl, source)
        }
        ParamBlockTarget::MasterSequencerSong => complete_song_names(model),
        ParamBlockTarget::ParamValue { param_name, module_name } => {
            let values = complete_enum_values(module_name, &param_name, model);
            if values.is_empty() { vec![] } else { values }
        }
        ParamBlockTarget::ParamName { module_name } => complete_parameters(module_name, model),
    }
}

// ─── Formatting helpers (shared with hover) ──────────────────────────────

pub(crate) fn format_parameter_kind(kind: &patches_core::ParameterKind) -> String {
    match kind {
        patches_core::ParameterKind::Float { min, max, default } => {
            format!("float ({min}..{max}, default {default})")
        }
        patches_core::ParameterKind::Int { min, max, default } => {
            format!("int ({min}..{max}, default {default})")
        }
        patches_core::ParameterKind::Bool { default } => {
            format!("bool (default {default})")
        }
        patches_core::ParameterKind::Enum { variants, default } => {
            let vs = variants.join(" | ");
            format!("enum ({vs}, default {default})")
        }
        patches_core::ParameterKind::File { extensions } => {
            let exts = extensions.join(", ");
            format!("file ({exts})")
        }
        patches_core::ParameterKind::SongName => "song name".to_string(),
    }
}

pub(crate) fn cable_kind_str(kind: &patches_core::CableKind) -> &'static str {
    match kind {
        patches_core::CableKind::Mono => "mono",
        patches_core::CableKind::Poly => "poly",
        patches_core::CableKind::Stereo => "stereo",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis;
    use crate::ast_builder;
    use crate::parser::language;
    use crate::manifest_source::manifest_registry as default_registry;
    use tree_sitter::Parser;

    fn setup(source: &str) -> (Tree, SemanticModel, Registry) {
        let mut parser = Parser::new();
        parser.set_language(&language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let (file, _) = ast_builder::build_ast(&tree, source);
        let registry = default_registry();
        let model = analysis::analyse(&file, &registry);
        (tree, model, registry)
    }

    #[test]
    fn completions_for_module_type() {
        let source = "patch {\n    module osc : \n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find(": \n").unwrap() + 2;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        assert!(
            items.iter().any(|i| i.label == "Osc"),
            "expected Osc in completions, got: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn completions_while_typing_module_type() {
        let source = "patch {\n    module x : Pitch\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("Pitch").unwrap() + 5;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"PitchShift"),
            "expected PitchShift in completions, got: {labels:?}"
        );
        assert!(
            labels.contains(&"ConvReverb"),
            "expected ConvReverb in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_for_at_block_aliases() {
        let source =
            "patch {\n    module mx : Mixer([drum, bass]) {\n        @\n    }\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find('@').unwrap() + 1;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"drum"),
            "expected drum in completions, got: {labels:?}"
        );
        assert!(
            labels.contains(&"bass"),
            "expected bass in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_for_parameters() {
        let source = "patch {\n    module osc : Osc { }\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("{ }").unwrap() + 2;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        assert!(
            items.iter().any(|i| i.label == "frequency"),
            "expected frequency in completions, got: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn completions_for_port_after_dot() {
        let source = "patch {\n    module osc : Osc\n    module out : AudioOut\n    osc.\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("osc.\n").unwrap() + 4;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        assert!(
            items.iter().any(|i| i.label == "sine"),
            "expected sine in completions, got: {:?}",
            items.iter().map(|i| &i.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn completions_for_port_index_aliases() {
        let source =
            "patch {\n    module mix : Mixer([drum, bass])\n    mix.out[\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("out[").unwrap() + 4;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"drum"),
            "expected drum in completions, got: {labels:?}"
        );
        assert!(
            labels.contains(&"bass"),
            "expected bass in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_at_statement_start_offer_stereo_keyword() {
        // ADR 0070: cursor on a fresh line inside the patch body should
        // discover the `stereo` keyword alongside the existing `module`
        // statement-introducer.
        let source = "patch {\n    \n}";
        let (tree, model, registry) = setup(source);
        // Position cursor on the empty indented line.
        let byte_offset = source.find("    \n").unwrap() + 4;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"stereo") && labels.contains(&"module"),
            "expected `stereo` and `module` keywords: {labels:?}"
        );
    }

    #[test]
    fn completions_for_stereo_module_port_after_dot() {
        // ADR 0070 — bus form: typing `bus.` on a stereo module offers
        // the underlying mono module's port labels exactly like a plain
        // mono module does. Side selectors come *after* the port label.
        let source = "patch {\n    stereo module bus : Vca\n    bus.\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("bus.\n").unwrap() + 4;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"in") && labels.contains(&"out"),
            "expected Vca's in/out ports for stereo bus form: {labels:?}"
        );
    }

    #[test]
    fn completions_for_stereo_port_index_offers_l_and_r() {
        let source = "patch {\n    stereo module bus : Vca\n    bus.out[\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("out[").unwrap() + 4;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["l", "r"], "got: {labels:?}");
    }

    #[test]
    fn completions_for_stereo_at_block_offers_l_and_r() {
        let source = "patch {\n    stereo module bus : Vca {\n        @\n    }\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("@\n").unwrap() + 1;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, vec!["l", "r"], "got: {labels:?}");
    }

    #[test]
    fn completions_for_dollar_dot_in_template() {
        let source = "template voice {\n    in: voct, gate\n    out: audio\n    module osc : Osc\n    $.\n}\npatch {}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("$.").unwrap() + 2;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"voct"),
            "expected voct in completions, got: {labels:?}"
        );
        assert!(
            labels.contains(&"gate"),
            "expected gate in completions, got: {labels:?}"
        );
        assert!(
            labels.contains(&"audio"),
            "expected audio in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_for_pattern_names_in_song_row() {
        let source = "pattern drums {\n    kick: x . x .\n}\n\nsong my_song(ch) {\n    play {\n        \n    }\n}\n\npatch {}";
        let (tree, model, registry) = setup(source);
        // Position cursor inside the play block before the closing brace.
        let byte_offset = source.find("        \n    }").unwrap() + 8;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"drums"),
            "expected drums in completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_for_enum_values_after_param_colon() {
        let source = "patch {\n    module o : Osc {\n        fm_type: \n    }\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("fm_type: \n").unwrap() + "fm_type: ".len();
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            !labels.is_empty() && items.iter().all(|i| i.kind == Some(CompletionItemKind::ENUM_MEMBER)),
            "expected enum-member completions, got: {labels:?}"
        );
    }

    #[test]
    fn completions_after_tilde_returns_all_components() {
        let source = "patch {\n    module o : Osc\n    o.out -> ~\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find('~').unwrap() + 1;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels.len(), 6, "got: {labels:?}");
        for c in ["meter", "stereo_meter", "osc", "spectrum", "gate_led", "trigger_led"] {
            assert!(labels.contains(&c), "missing {c}: {labels:?}");
        }
    }

    #[test]
    fn completions_after_plus_filters_listed_and_incompatible_kinds() {
        let source = "patch {\n    module o : Osc\n    o.out -> ~meter+\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find('+').unwrap() + 1;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        // Audio components remaining after `meter`: osc, spectrum, gate_led.
        assert_eq!(labels.len(), 3, "got: {labels:?}");
        for c in ["osc", "spectrum", "gate_led"] {
            assert!(labels.contains(&c), "missing {c}: {labels:?}");
        }
        assert!(!labels.contains(&"trigger_led"), "trigger_led excluded (cable kind)");
        assert!(!labels.contains(&"stereo_meter"), "stereo_meter excluded (compound mono-only)");
        assert!(!labels.contains(&"meter"), "meter already listed");
    }

    #[test]
    fn completions_after_stereo_meter_plus_returns_empty() {
        // ADR 0059 §8: stereo_meter cannot share a compound tap.
        let source = "patch {\n    module m : Mix\n    m.out -> ~stereo_meter+\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find('+').unwrap() + 1;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        assert!(items.is_empty(), "stereo_meter+ must offer no continuations");
    }

    #[test]
    fn completions_for_song_names_in_master_sequencer() {
        let source = "song my_song(ch) {\n    play {}\n}\n\npatch {\n    module seq : MasterSequencer([ch]) {\n        song: \n    }\n}";
        let (tree, model, registry) = setup(source);
        let byte_offset = source.find("song: \n").unwrap() + 6;
        let items = compute_completions(&tree, source, byte_offset, &model, &registry);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"my_song"),
            "expected my_song in completions, got: {labels:?}"
        );
    }
}
