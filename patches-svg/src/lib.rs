//! SVG renderer for Patches DSL patch graphs.
//!
//! Consumes a [`patches_dsl::FlatPatch`] directly: no `ModuleGraph`, no
//! interpreter pass. Partial or invalid patches still render, which is
//! useful for live editing where the user's current source may not be a
//! fully valid graph.
//!
//! A [`SourceMap`] and a [`Registry`] are required so the renderer can
//! resolve provenance spans to source-file snippets and look up each
//! port's [`CableKind`] / [`PolyLayout`]. When either lookup fails (e.g.
//! a synthetic span or a module type the registry does not know), the
//! renderer falls back to unclassified output — the SVG still renders.
//!
//! Sugiyama layout lives in the [`layout`] submodule; rendering emits
//! a standalone SVG `String` with inline styling.
//!
//! # Snapshot tests
//!
//! Structural regressions on the rendered output are pinned with `insta`
//! snapshots under `src/snapshots/`. If you intentionally change the SVG
//! shape (theme, layout config, class names), run `cargo insta review -p
//! patches-svg` to accept new snapshots.

pub mod layout;
mod flat_to_layout;
mod hints;
mod render;

use patches_core::source_map::SourceMap;
use patches_registry::Registry;
use patches_dsl::FlatPatch;

use crate::layout::LayoutConfig;

pub use flat_to_layout::{flat_to_layout_input, port_label};

// ── Options ────────────────────────────────────────────────────────────────

/// Visual theme for the rendered SVG.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    #[default]
    Dark,
}

/// Renderer options.
#[derive(Debug, Clone)]
pub struct SvgOptions {
    pub theme: Theme,
    /// If false, omit per-port text labels (dots + cables remain).
    pub include_port_labels: bool,
    /// If true, emit a `<style>` block with CSS classes; else inline
    /// `style="..."` on each element.
    pub embed_css: bool,
    /// Override the default node width. `None` uses [`NODE_WIDTH`].
    pub node_width: Option<f32>,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            include_port_labels: true,
            embed_css: true,
            node_width: None,
        }
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Default node width; matches the value used by the clap GUI so outputs
/// are visually consistent. Override via [`SvgOptions::node_width`].
pub const NODE_WIDTH: f32 = 160.0;

/// Render `patch` as a standalone SVG document.
///
/// `source_map` resolves provenance spans to source-file snippets used in
/// hover tooltips. `registry` resolves each module's port kinds so cables
/// can be styled per [`CableKind`] / [`PolyLayout`]. Both lookups degrade
/// gracefully: missing sources or unknown module types yield unstyled,
/// tooltip-free output rather than an error.
pub fn render_svg(
    patch: &FlatPatch,
    source_map: &SourceMap,
    registry: &Registry,
    opts: &SvgOptions,
) -> String {
    let config = LayoutConfig::default();
    let width = opts.node_width.unwrap_or(NODE_WIDTH);
    let (mut nodes, mut edges) = flat_to_layout::flat_to_layout_input(patch, &config);
    for n in &mut nodes {
        n.width = width;
    }
    hints::enrich_node_hints(patch, source_map, &mut nodes);
    hints::enrich_edge_hints(patch, source_map, registry, &mut edges);
    let layout = layout::layout_graph(&nodes, &edges, &config);
    render::emit_svg(&layout, &config, opts)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::cables::{CableKind, PolyLayout};
    use patches_core::ModuleShape;
    use patches_dsl::{FlatConnection, FlatModule, FlatPatch, Provenance};
    use patches_modules::default_registry;

    fn synthetic_span() -> patches_dsl::ast::Span {
        patches_dsl::ast::Span::synthetic()
    }

    fn empty_source_map() -> SourceMap {
        SourceMap::new()
    }

    fn sample_patch() -> FlatPatch {
        let mut patch = FlatPatch::default();
        patch.graph.modules = vec![
            FlatModule {
                id: "osc".into(),
                type_name: "Osc".into(),
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: Provenance::root(synthetic_span()),
            },
            FlatModule {
                id: "vca".into(),
                type_name: "Vca".into(),
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: Provenance::root(synthetic_span()),
            },
        ];
        patch.graph.connections = vec![FlatConnection {
            from_module: "osc".into(),
            from_port: "sine".into(),
            from_index: 0,
            to_module: "vca".into(),
            to_port: "in".into(),
            to_index: 0,
            scale: 1.0,
            provenance: Provenance::root(synthetic_span()),
            from_provenance: Provenance::root(synthetic_span()),
            to_provenance: Provenance::root(synthetic_span()),
        }];
        patch
    }

    fn render(patch: &FlatPatch, opts: &SvgOptions) -> String {
        render_svg(patch, &empty_source_map(), &default_registry(), opts)
    }

    #[test]
    fn empty_patch_renders_minimal_svg() {
        let flat = FlatPatch::default();
        let svg = render(&flat, &SvgOptions::default());
        insta::assert_snapshot!("empty_patch", svg);
    }

    #[test]
    fn sample_patch_snapshot() {
        let flat = sample_patch();
        let svg = render(&flat, &SvgOptions::default());
        insta::assert_snapshot!("sample_patch_mono", svg);
    }

    #[test]
    fn inline_mode_omits_style_block() {
        let flat = sample_patch();
        let opts = SvgOptions {
            embed_css: false,
            ..SvgOptions::default()
        };
        let svg = render(&flat, &opts);
        assert!(!svg.contains("<style>"));
        assert!(svg.contains("fill=\""));
    }

    #[test]
    fn include_port_labels_false_omits_port_text() {
        let flat = sample_patch();
        let opts = SvgOptions {
            include_port_labels: false,
            ..SvgOptions::default()
        };
        let svg = render(&flat, &opts);
        assert!(!svg.contains(">sine<"));
        assert!(!svg.contains(">in<"));
        assert!(svg.contains("class=\"input-dot\"") || svg.contains("class=\"output-dot\""));
    }

    #[test]
    fn xml_escapes_special_characters_in_labels() {
        let mut patch = FlatPatch::default();
        patch.graph.modules = vec![FlatModule {
            id: "a&b".into(),
            type_name: "<Odd>".into(),
            shape: vec![],
            params: vec![],
            port_aliases: vec![],
            provenance: Provenance::root(synthetic_span()),
        }];
        let svg = render(&patch, &SvgOptions::default());
        assert!(svg.contains("a&amp;b : &lt;Odd&gt;"));
        assert!(!svg.contains("<Odd>"));
    }

    #[test]
    fn output_is_well_formed_xml() {
        let flat = sample_patch();
        let svg = render(&flat, &SvgOptions::default());
        let mut reader = quick_xml::Reader::from_str(&svg);
        reader.config_mut().trim_text(true);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("invalid XML at position {}: {e:?}", reader.buffer_position()),
            }
        }
    }

    #[test]
    fn synthetic_provenance_omits_title_and_data_attrs() {
        let svg = render(&sample_patch(), &SvgOptions::default());
        assert!(!svg.contains("<title>"));
        assert!(!svg.contains("data-span-start"));
        assert!(!svg.contains("data-source-id"));
    }

    #[test]
    fn real_source_provenance_emits_title_and_data_attrs() {
        let source = "patch { module osc : Osc\nmodule vca : Vca\nosc.out -> vca.in }\n";
        let load = patches_dsl::load_with(
            std::path::Path::new("master.patches"),
            |_p: &std::path::Path| -> std::io::Result<String> { Ok(source.to_string()) },
        )
        .expect("load");
        let expanded = patches_dsl::expand(&load.file).expect("expand");
        let svg = render_svg(
            &expanded.patch,
            &load.source_map,
            &default_registry(),
            &SvgOptions::default(),
        );
        insta::assert_snapshot!("real_source_provenance", svg);
    }

    #[test]
    fn mono_cable_gets_cable_mono_class() {
        let source = "patch { module osc : Osc\nmodule vca : Vca\nosc.sine -> vca.in }\n";
        let load = patches_dsl::load_with(
            std::path::Path::new("master.patches"),
            |_p: &std::path::Path| -> std::io::Result<String> { Ok(source.to_string()) },
        )
        .expect("load");
        let expanded = patches_dsl::expand(&load.file).expect("expand");
        let svg = render_svg(
            &expanded.patch,
            &load.source_map,
            &default_registry(),
            &SvgOptions::default(),
        );
        insta::assert_snapshot!("mono_cable", svg);
    }

    #[test]
    fn unknown_module_type_falls_back_to_base_cable() {
        let mut patch = FlatPatch::default();
        patch.graph.modules = vec![
            FlatModule {
                id: "a".into(),
                type_name: "NoSuchModule".into(),
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: Provenance::root(synthetic_span()),
            },
            FlatModule {
                id: "b".into(),
                type_name: "NoSuchModule".into(),
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: Provenance::root(synthetic_span()),
            },
        ];
        patch.graph.connections = vec![FlatConnection {
            from_module: "a".into(),
            from_port: "out".into(),
            from_index: 0,
            to_module: "b".into(),
            to_port: "in".into(),
            to_index: 0,
            scale: 1.0,
            provenance: Provenance::root(synthetic_span()),
            from_provenance: Provenance::root(synthetic_span()),
            to_provenance: Provenance::root(synthetic_span()),
        }];
        let svg = render(&patch, &SvgOptions::default());
        assert!(svg.contains(r#"<path class="cable""#));
        assert!(!svg.contains(r#"<path class="cable cable-"#));
    }

    #[test]
    fn poly_cable_gets_poly_audio_class() {
        // AudioOut has a poly input and MasterSequencer has a poly output
        // layout — check via a module with a poly audio output. Osc has a
        // poly_out "poly" per the module descriptor tests; we check that a
        // poly-output-producing module yields the right class via registry.
        //
        // We build a minimal synthetic patch using the real registry; if any
        // registered module exposes a poly Audio output it must pick up the
        // poly-audio class. This test stays robust by asking the registry for
        // each module and scanning for one.
        let registry = default_registry();
        let names: Vec<String> = registry.module_names().map(|s| s.to_string()).collect();
        let shape = ModuleShape::default();
        let mut from_name = None;
        let mut from_port = None;
        for name in &names {
            if let Ok(desc) = registry.describe(name, &shape) {
                if let Some(p) = desc.outputs.iter().find(|p| {
                    p.kind == CableKind::Poly && p.poly_layout == PolyLayout::Audio
                }) {
                    from_name = Some(name.clone());
                    from_port = Some(p.name.to_string());
                    break;
                }
            }
        }
        let (from_name, from_port) = match (from_name, from_port) {
            (Some(n), Some(p)) => (n, p),
            _ => return, // no poly-audio output modules in registry; skip
        };

        let mut patch = FlatPatch::default();
        patch.graph.modules = vec![
            FlatModule {
                id: "src".into(),
                type_name: from_name,
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: Provenance::root(synthetic_span()),
            },
            FlatModule {
                id: "sink".into(),
                type_name: "AudioOut".into(),
                shape: vec![],
                params: vec![],
                port_aliases: vec![],
                provenance: Provenance::root(synthetic_span()),
            },
        ];
        patch.graph.connections = vec![FlatConnection {
            from_module: "src".into(),
            from_port,
            from_index: 0,
            to_module: "sink".into(),
            to_port: "in_left".into(),
            to_index: 0,
            scale: 1.0,
            provenance: Provenance::root(synthetic_span()),
            from_provenance: Provenance::root(synthetic_span()),
            to_provenance: Provenance::root(synthetic_span()),
        }];
        let svg = render(&patch, &SvgOptions::default());
        assert!(
            svg.contains("cable-poly-audio"),
            "expected cable-poly-audio class: {svg}"
        );
    }
}
