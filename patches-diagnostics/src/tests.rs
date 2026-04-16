use super::*;
use std::path::PathBuf;

fn map_with(path: &str, text: &str) -> (SourceMap, SourceId) {
    let mut map = SourceMap::new();
    let id = map.add(PathBuf::from(path), text.to_string());
    (map, id)
}

#[test]
fn build_error_without_origin_uses_synthetic_primary() {
    let map = SourceMap::new();
    let err = BuildError::UnknownModule { name: "foo".into(), origin: None };
    let d = RenderedDiagnostic::from_build_error(&err, &map);
    assert_eq!(d.primary.source, SourceId::SYNTHETIC);
    assert_eq!(d.primary.kind, SnippetKind::Primary);
    assert!(d.related.is_empty());
    assert_eq!(d.code.as_deref(), Some("unknown-module"));
}

#[test]
fn build_error_with_root_provenance_has_no_related() {
    let (map, id) = map_with("a.patches", "module x : Y\n");
    let span = Span::new(id, 7, 8);
    let err = BuildError::UnknownModule { name: "Y".into(), origin: Some(Provenance::root(span)) };
    let d = RenderedDiagnostic::from_build_error(&err, &map);
    assert_eq!(d.primary.source, id);
    assert_eq!(d.primary.range, 7..8);
    assert!(d.related.is_empty());
}

#[test]
fn build_error_with_one_expansion_level() {
    let (mut map, inner) = map_with("inner.patches", "module x : Y\n");
    let outer = map.add(PathBuf::from("outer.patches"), "use inner\n".to_string());
    let prov = Provenance {
        site: Span::new(inner, 7, 8),
        expansion: vec![Span::new(outer, 0, 3)],
    };
    let err = BuildError::UnknownModule { name: "Y".into(), origin: Some(prov) };
    let d = RenderedDiagnostic::from_build_error(&err, &map);
    assert_eq!(d.primary.source, inner);
    assert_eq!(d.related.len(), 1);
    assert_eq!(d.related[0].source, outer);
    assert_eq!(d.related[0].kind, SnippetKind::Expansion);
    assert_eq!(d.related[0].label, "expanded from here");
}

#[test]
fn build_error_with_multi_level_expansion_cross_file() {
    let (mut map, a) = map_with("a.patches", "aaaa".to_string().as_str());
    let b = map.add(PathBuf::from("b.patches"), "bbbb".to_string());
    let c = map.add(PathBuf::from("c.patches"), "cccc".to_string());
    let prov = Provenance {
        site: Span::new(a, 0, 2),
        expansion: vec![Span::new(b, 1, 3), Span::new(c, 0, 4)],
    };
    let err = BuildError::Custom {
        module: "x",
        message: "boom".to_string(),
        origin: Some(prov),
    };
    let d = RenderedDiagnostic::from_build_error(&err, &map);
    assert_eq!(d.primary.source, a);
    assert_eq!(d.related.len(), 2);
    assert_eq!(d.related[0].source, b);
    assert_eq!(d.related[1].source, c);
}

#[test]
fn expand_error_maps_to_primary_only() {
    let (map, id) = map_with("x.patches", "module x : Y\n");
    let err = ExpandError {
        code: patches_dsl::StructuralCode::UnknownModuleRef,
        span: Span::new(id, 7, 8),
        message: "bad".into(),
    };
    let d = RenderedDiagnostic::from_expand_error(&err, &map);
    assert_eq!(d.primary.source, id);
    assert_eq!(d.primary.range, 7..8);
    assert!(d.related.is_empty());
    assert_eq!(d.message, "bad");
    assert_eq!(d.code.as_deref(), Some("ST0007"));
}

#[test]
fn structural_error_code_picks_named_variant() {
    let (map, id) = map_with("x.patches", "module x : Y\n");
    let err = ExpandError {
        code: patches_dsl::StructuralCode::RecursiveTemplate,
        span: Span::new(id, 0, 1),
        message: "recursion".into(),
    };
    let d = RenderedDiagnostic::from_structural_error(&err, &map);
    assert_eq!(d.code.as_deref(), Some("ST0010"));
    assert_eq!(d.primary.label, "recursive template");
}

#[test]
fn load_error_io_uses_synthetic_primary() {
    use patches_dsl::loader::{LoadError, LoadErrorKind};
    let err = LoadError {
        kind: LoadErrorKind::Io {
            path: PathBuf::from("missing.patches"),
            error: std::io::Error::other("no such file"),
        },
        include_chain: vec![],
    };
    let map = SourceMap::new();
    let d = RenderedDiagnostic::from_load_error(&err, &map);
    assert_eq!(d.primary.source, SourceId::SYNTHETIC);
    assert_eq!(d.code.as_deref(), Some("LD0001"));
    assert!(d.related.is_empty());
}

#[test]
fn load_error_cycle_renders_include_chain() {
    use patches_dsl::loader::{LoadError, LoadErrorKind};
    let (mut map, root) = map_with("root.patches", "include \"sub\"\n");
    let sub = map.add(PathBuf::from("sub.patches"), "include \"root\"\n".into());
    let err = LoadError {
        kind: LoadErrorKind::Cycle {
            parent: PathBuf::from("sub.patches"),
            target: PathBuf::from("root.patches"),
        },
        include_chain: vec![
            (PathBuf::from("root.patches"), Span::new(root, 8, 13)),
            (PathBuf::from("sub.patches"), Span::new(sub, 8, 14)),
        ],
    };
    let d = RenderedDiagnostic::from_load_error(&err, &map);
    assert_eq!(d.code.as_deref(), Some("LD0003"));
    assert_eq!(d.related.len(), 2);
}

#[test]
fn interpret_error_renders_expansion_chain() {
    use patches_core::Provenance;
    use patches_interpreter::{InterpretError, InterpretErrorCode};
    let (mut map, inner) = map_with("inner.patches", "x\n");
    let outer = map.add(PathBuf::from("outer.patches"), "y\n".into());
    let err = InterpretError {
        code: InterpretErrorCode::ConnectFailed,
        provenance: Provenance {
            site: Span::new(inner, 0, 1),
            expansion: vec![Span::new(outer, 0, 1)],
        },
        message: "nope".into(),
    };
    let d = RenderedDiagnostic::from_interpret_error(&err, &map);
    assert_eq!(d.code.as_deref(), Some("RT0001"));
    assert_eq!(d.primary.label, "connect failed");
    assert_eq!(d.related.len(), 1);
}

#[test]
fn source_line_col_resolves_offset() {
    let (map, id) = map_with("x.patches", "abc\ndef\nghi");
    assert_eq!(source_line_col(&map, id, 5), (2, 2));
}

#[test]
fn layering_audit_flags_unknown_module_bind_error() {
    use patches_core::Provenance;
    use patches_interpreter::{BindError, BindErrorCode};
    let (map, id) = map_with("x.patches", "patch { }\n");
    // Craft a BindError as if stage 3b caught an unknown-module
    // reference — something stage 3a expansion validates against
    // the flattened patch's module set. When descriptor_bind reports
    // BN0006, the pipeline layering audit must surface a PV0001
    // warning alongside.
    let err = BindError::new(
        BindErrorCode::UnknownModule,
        Provenance {
            site: Span::new(id, 0, 5),
            expansion: vec![],
        },
        "module 'ghost' not found",
    );
    let warnings = RenderedDiagnostic::pipeline_layering_warnings(&[err]);
    let _ = &map; // map kept alive for the span's source_id
    assert_eq!(warnings.len(), 1);
    let w = &warnings[0];
    assert_eq!(w.severity, Severity::Warning);
    assert_eq!(w.code.as_deref(), Some("PV0001"));
    assert!(
        w.message.contains("descriptor_bind") && w.message.contains("expansion"),
        "message should name the stages: {}",
        w.message
    );
    assert_eq!(w.primary.source, id);
}

#[test]
fn layering_audit_ignores_non_layering_bind_errors() {
    use patches_core::Provenance;
    use patches_interpreter::{BindError, BindErrorCode};
    // UnknownPort is a legitimate stage-3b concern (plain modules'
    // port sets are unknown to the DSL expander) — it must not
    // trigger a PV warning.
    let err = BindError::new(
        BindErrorCode::UnknownPort,
        Provenance {
            site: Span::new(SourceId::SYNTHETIC, 0, 0),
            expansion: vec![],
        },
        "no such port",
    );
    assert!(RenderedDiagnostic::pipeline_layering_warnings(&[err]).is_empty());
}

#[test]
fn render_provenance_error_none_matches_synthetic_shape() {
    // Direct-builder call with `None` provenance must match the
    // synthetic-primary shape produced by `Self::synthetic` so CLAP's
    // `Plan` variant (routed through `from_plan_error` with a
    // provenance-less BuildError) and `NotActivated` (routed through
    // `synthetic`) agree on the output shape.
    let a = render_provenance_error("plan", "boom", None, "here");
    let b = RenderedDiagnostic::synthetic("plan", "boom", "here");
    assert_eq!(a, b);
}

#[test]
fn render_provenance_error_some_matches_bind_converter() {
    // A `BindError` routed through `from_bind_error` must equal the
    // same code/message/provenance/label routed through the shared
    // builder. This is the round-trip guarantee that all three
    // consumers produce identical diagnostics.
    use patches_core::Provenance;
    use patches_interpreter::{BindError, BindErrorCode};
    let (map, id) = map_with("x.patches", "patch { }\n");
    let prov = Provenance {
        site: Span::new(id, 0, 5),
        expansion: vec![],
    };
    let err = BindError::new(BindErrorCode::UnknownPort, prov.clone(), "no port");
    let via_converter = RenderedDiagnostic::from_bind_error(&err, &map);
    let via_builder = render_provenance_error(
        BindErrorCode::UnknownPort.as_str(),
        "no port",
        Some(&prov),
        BindErrorCode::UnknownPort.label(),
    );
    assert_eq!(via_converter, via_builder);
}

#[test]
fn parse_error_renders_with_ld0002_code() {
    let (_, id) = map_with("x.patches", "bad\n");
    let err = patches_dsl::ParseError {
        span: Span::new(id, 0, 3),
        message: "syntax".to_string(),
    };
    let d = RenderedDiagnostic::from_parse_error(&err);
    assert_eq!(d.code.as_deref(), Some("LD0002"));
    assert_eq!(d.primary.source, id);
    assert_eq!(d.primary.range, 0..3);
    assert_eq!(d.message, "syntax");
}

#[test]
fn expand_warning_renders_as_warning_severity() {
    let (_, id) = map_with("x.patches", "foo\n");
    let w = patches_dsl::Warning {
        span: Span::new(id, 0, 3),
        message: "careful".to_string(),
    };
    let d = RenderedDiagnostic::from_expand_warning(&w);
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.primary.source, id);
    assert_eq!(d.message, "careful");
    assert!(d.code.is_none());
}

#[test]
fn synthetic_uses_synthetic_primary_source() {
    let d = RenderedDiagnostic::synthetic("not-activated", "not activated", "here");
    assert_eq!(d.primary.source, SourceId::SYNTHETIC);
    assert_eq!(d.code.as_deref(), Some("not-activated"));
    assert_eq!(d.message, "not activated");
}

#[test]
fn source_line_col_synthetic_returns_zeroes() {
    let map = SourceMap::new();
    assert_eq!(source_line_col(&map, SourceId::SYNTHETIC, 5), (1, 1));
}
