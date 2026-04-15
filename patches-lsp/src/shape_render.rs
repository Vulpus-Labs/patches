//! Shape-evaluation and rendering helpers shared by expansion-aware
//! features (hover, inlay hints, peek expansion).
//!
//! Flat modules carry their shape as `Vec<(String, Scalar)>`. This module
//! turns that into a `ModuleShape` for registry lookups and renders compact
//! human-readable summaries for UI surfaces.

use patches_core::{ModuleShape, PortDescriptor};
use patches_dsl::ast::{self as dsl_ast, PortLabel as DslPortLabel, Scalar};

/// Render a [`patches_dsl::ast::PortRef`] to the compact
/// `module.port[index]` notation used by hover and peek. `Literal` indices
/// render as `/{n}` for backwards compatibility with authored hover output;
/// alias / arity forms render as `[k]` / `[*n]`.
pub(crate) fn format_port_ref(pr: &dsl_ast::PortRef) -> String {
    let port_str = match &pr.port {
        DslPortLabel::Literal(name) => name.clone(),
        DslPortLabel::Param(name) => format!("<{name}>"),
    };
    match &pr.index {
        None => format!("{}.{}", pr.module, port_str),
        Some(dsl_ast::PortIndex::Literal(n)) => format!("{}.{}/{}", pr.module, port_str, n),
        Some(dsl_ast::PortIndex::Alias(a)) => format!("{}.{}[{}]", pr.module, port_str, a),
        Some(dsl_ast::PortIndex::Arity(a)) => format!("{}.{}[*{}]", pr.module, port_str, a),
    }
}

/// Render a post-expansion `FlatConnection`-style port reference: returns
/// `name` for index 0 and `name[index]` otherwise. Shared by peek's
/// connection rendering so the formatting lives in one place.
pub(crate) fn format_flat_port(name: &str, index: u32) -> String {
    if index == 0 {
        name.to_string()
    } else {
        format!("{name}[{index}]")
    }
}

/// Build a [`ModuleShape`] from a [`patches_dsl::flat::FlatModule::shape`]
/// argument list. Unknown keys are ignored; known keys follow the same
/// coercion rules as `patches-modules` (`channels`, `length` as ints;
/// `high_quality` as bool).
pub(crate) fn module_shape_from_args(args: &[(String, Scalar)]) -> ModuleShape {
    let mut shape = ModuleShape::default();
    for (name, scalar) in args {
        match (name.as_str(), scalar) {
            ("channels", Scalar::Int(n)) => shape.channels = *n as usize,
            ("length", Scalar::Int(n)) => shape.length = *n as usize,
            ("high_quality", Scalar::Bool(b)) => shape.high_quality = *b,
            _ => {}
        }
    }
    shape
}

/// Compact, comma-separated rendering of a module shape, suitable for an
/// inlay-hint label. Empty string when no shape fields are set.
pub(crate) fn render_shape_inline(shape: &ModuleShape) -> String {
    let mut parts = Vec::new();
    if shape.channels > 0 {
        parts.push(format!("channels={}", shape.channels));
    }
    if shape.length > 0 {
        parts.push(format!("length={}", shape.length));
    }
    if shape.high_quality {
        parts.push("hq".to_string());
    }
    parts.join(", ")
}

/// Collapse a descriptor's indexed ports into "name[0..N]" summaries.
/// Returns one string per indexed port group; scalar ports are skipped.
pub(crate) fn render_indexed_ports(ports: &[PortDescriptor]) -> Vec<String> {
    let mut groups: Vec<(&str, Vec<usize>)> = Vec::new();
    for p in ports {
        if let Some(g) = groups.iter_mut().find(|g| g.0 == p.name) {
            g.1.push(p.index);
        } else {
            groups.push((p.name, vec![p.index]));
        }
    }
    groups
        .into_iter()
        .filter(|(_, ixs)| ixs.len() > 1)
        .map(|(name, ixs)| {
            let max = ixs.iter().copied().max().unwrap_or(0);
            let min = ixs.iter().copied().min().unwrap_or(0);
            format!("{name}[{min}..{max}]")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::CableKind;

    #[test]
    fn shape_from_args_round_trips() {
        let args = vec![
            ("channels".to_string(), Scalar::Int(4)),
            ("length".to_string(), Scalar::Int(2048)),
            ("high_quality".to_string(), Scalar::Bool(true)),
        ];
        let s = module_shape_from_args(&args);
        assert_eq!(s.channels, 4);
        assert_eq!(s.length, 2048);
        assert!(s.high_quality);
    }

    #[test]
    fn inline_render_skips_unset_fields() {
        let s = ModuleShape { channels: 2, length: 0, high_quality: false };
        assert_eq!(render_shape_inline(&s), "channels=2");
    }

    #[test]
    fn indexed_ports_collapse_groups() {
        let p = |name: &'static str, index: usize| PortDescriptor {
            name,
            index,
            kind: CableKind::Mono,
            poly_layout: patches_core::PolyLayout::Audio,
        };
        let ports = vec![p("out", 0), p("out", 1), p("out", 2), p("solo", 0)];
        let rendered = render_indexed_ports(&ports);
        assert_eq!(rendered, vec!["out[0..2]".to_string()]);
    }

    #[test]
    fn indexed_ports_empty_for_scalar_descriptor() {
        let ports = vec![PortDescriptor {
            name: "out",
            index: 0,
            kind: CableKind::Mono,
            poly_layout: patches_core::PolyLayout::Audio,
        }];
        let rendered = render_indexed_ports(&ports);
        assert!(rendered.is_empty());
    }

}
