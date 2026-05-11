//! Host-control desugaring (ADR 0057 §2, ticket 0808).
//!
//! Drives `desugar_host_controls` directly on parsed files and inspects
//! the rewritten AST: synthesised `~host_control` /
//! `~host_control_trigger` instances, alphabetical slot ordering,
//! bare-name reference rewriting, and the empty / unresolved cases.

use patches_dsl::ast::{
    AtBlockIndex, CableEndpoint, ParamEntry, PortIndex, PortLabel, Scalar, ShapeValue, Statement,
    Value,
};
use patches_dsl::host_control_desugar::{
    desugar_host_controls, AUDIO_OUT, SYNTH_HOST_CONTROL, TRIGGER_OUT,
};
use patches_dsl::parse;

fn rewritten(src: &str) -> patches_dsl::ast::File {
    let file = parse(src).expect("parse ok");
    desugar_host_controls(file.clone()).expect("desugar ok").0
}

fn find_module<'a>(
    file: &'a patches_dsl::ast::File,
    name: &str,
) -> Option<&'a patches_dsl::ast::ModuleDecl> {
    file.patch.body.iter().find_map(|s| match s {
        Statement::Module(m) if m.name.name == name => Some(m),
        _ => None,
    })
}

fn aliases(m: &patches_dsl::ast::ModuleDecl) -> Vec<String> {
    let cb = m.call_block.as_ref().expect("synth module has call block");
    let arg = &cb.args[0];
    let val = match arg {
        patches_dsl::ast::CallArg::Bare { value, .. } => value,
        _ => panic!("expected bare arg"),
    };
    match val {
        ShapeValue::AliasList(list) => list.iter().map(|i| i.name.clone()).collect(),
        _ => panic!("expected alias list"),
    }
}

fn at_block_pair<'a>(
    m: &'a patches_dsl::ast::ModuleDecl,
    alias: &str,
) -> &'a Vec<(patches_dsl::ast::Ident, Value)> {
    for entry in &m.params {
        if let ParamEntry::AtBlock { index: AtBlockIndex::Alias(a), entries, .. } = entry {
            if a == alias {
                return entries;
            }
        }
    }
    panic!("no @{alias} block in {:?}", m.name.name);
}

#[test]
fn empty_case_no_synth_module() {
    let src = "patch { module osc : Osc() osc.out -> osc.in }";
    let r = rewritten(src);
    assert!(find_module(&r, SYNTH_HOST_CONTROL).is_none());
}

#[test]
fn alphabetical_slot_ordering() {
    let src = r#"patch {
        knob zeta { low: 1Hz, high: 2Hz }
        knob alpha { low: 1Hz, high: 2Hz }
        knob mu { low: 1Hz, high: 2Hz }
    }"#;
    let r = rewritten(src);
    let m = find_module(&r, SYNTH_HOST_CONTROL).expect("synth module");
    assert_eq!(aliases(m), vec!["alpha", "mu", "zeta"]);
    // slot_offset matches alphabetical position.
    for (i, a) in ["alpha", "mu", "zeta"].iter().enumerate() {
        let entries = at_block_pair(m, a);
        let slot = entries
            .iter()
            .find(|(k, _)| k.name == "slot_offset")
            .map(|(_, v)| v)
            .expect("slot_offset");
        assert!(matches!(slot, Value::Scalar(Scalar::Int(n)) if *n == i as i64));
    }
}

#[test]
fn tilde_name_reference_rewrites_to_synth_port() {
    let src = r#"patch {
        knob cutoff { low: 20Hz, high: 2000Hz }
        module filter : Filter()
        ~cutoff -> filter.voct
    }"#;
    let r = rewritten(src);
    let conn = r
        .patch
        .body
        .iter()
        .find_map(|s| if let Statement::Connection(c) = s { Some(c) } else { None })
        .unwrap();
    let CableEndpoint::Port(p) = &conn.lhs else {
        panic!("expected port lhs after rewrite, got {:?}", conn.lhs);
    };
    assert_eq!(p.module, SYNTH_HOST_CONTROL);
    assert!(matches!(&p.port, PortLabel::Literal(s) if s == AUDIO_OUT));
    assert!(matches!(
        &p.index,
        Some(PortIndex::Name { name, arity_marker: false }) if name == "cutoff"
    ));
}

#[test]
fn audio_and_trigger_share_one_synth_module_with_split_ports() {
    let src = r#"patch {
        knob k { low: 1Hz, high: 2Hz }
        trigger fire { }
        module flt : Filter()
        module env : Env()
        ~k -> flt.voct
        ~fire -> env.gate
    }"#;
    let r = rewritten(src);
    let m = find_module(&r, SYNTH_HOST_CONTROL).expect("synth module");
    // Single instance, one alias list across both kinds (alphabetical:
    // `fire` then `k`).
    assert_eq!(aliases(m), vec!["fire", "k"]);

    // Locate each rewritten cable by its index name and check the port.
    let connections: Vec<_> = r
        .patch
        .body
        .iter()
        .filter_map(|s| match s {
            Statement::Connection(c) => Some(c),
            _ => None,
        })
        .collect();
    let fire_lhs = connections
        .iter()
        .find_map(|c| match &c.lhs {
            CableEndpoint::Port(p) if p.index_name() == Some("fire") => Some(p),
            _ => None,
        })
        .expect("fire connection");
    let k_lhs = connections
        .iter()
        .find_map(|c| match &c.lhs {
            CableEndpoint::Port(p) if p.index_name() == Some("k") => Some(p),
            _ => None,
        })
        .expect("k connection");
    assert_eq!(fire_lhs.module, SYNTH_HOST_CONTROL);
    assert!(matches!(&fire_lhs.port, PortLabel::Literal(s) if s == TRIGGER_OUT));
    assert_eq!(k_lhs.module, SYNTH_HOST_CONTROL);
    assert!(matches!(&k_lhs.port, PortLabel::Literal(s) if s == AUDIO_OUT));
}

#[test]
fn undeclared_tilde_name_synthesises_implicit_knob() {
    // Ticket 0868: a `~name` reference without a preceding declaration
    // synthesises an empty `knob name {}` block transparently.
    let src = r#"patch {
        module flt : Filter()
        ~not_declared -> flt.voct
    }"#;
    let r = rewritten(src);
    let m = find_module(&r, SYNTH_HOST_CONTROL).expect("synth module");
    assert_eq!(aliases(m), vec!["not_declared"]);
    let conn = r
        .patch
        .body
        .iter()
        .find_map(|s| if let Statement::Connection(c) = s { Some(c) } else { None })
        .expect("connection rewritten");
    let CableEndpoint::Port(p) = &conn.lhs else {
        panic!("expected port lhs after rewrite, got {:?}", conn.lhs);
    };
    assert_eq!(p.module, SYNTH_HOST_CONTROL);
    assert!(matches!(&p.port, PortLabel::Literal(s) if s == AUDIO_OUT));
}

#[test]
fn implicit_and_explicit_empty_knob_identical() {
    // Ticket 0868: implicit `~name` (no decl) and explicit empty
    // `knob name {}` must lower to the same body and manifest.
    let implicit_src = r#"patch {
        module flt : Filter()
        ~cutoff -> flt.voct
    }"#;
    let explicit_src = r#"patch {
        knob cutoff { }
        module flt : Filter()
        ~cutoff -> flt.voct
    }"#;
    let (implicit_file, implicit_manifest) =
        desugar_host_controls(parse(implicit_src).expect("parse")).expect("desugar");
    let (explicit_file, explicit_manifest) =
        desugar_host_controls(parse(explicit_src).expect("parse")).expect("desugar");

    let im = find_module(&implicit_file, SYNTH_HOST_CONTROL).expect("synth (implicit)");
    let em = find_module(&explicit_file, SYNTH_HOST_CONTROL).expect("synth (explicit)");
    assert_eq!(aliases(im), aliases(em));
    assert_eq!(
        at_block_pair(im, "cutoff")
            .iter()
            .map(|(k, _)| k.name.as_str())
            .collect::<Vec<_>>(),
        at_block_pair(em, "cutoff")
            .iter()
            .map(|(k, _)| k.name.as_str())
            .collect::<Vec<_>>(),
    );

    // Manifest: names, slots, kinds, params must match (the explicit
    // form's empty block carries no fields either, so `params` is empty
    // both sides).
    assert_eq!(implicit_manifest.len(), explicit_manifest.len());
    for (a, b) in implicit_manifest.iter().zip(explicit_manifest.iter()) {
        assert_eq!(a.name, b.name);
        assert_eq!(a.slot, b.slot);
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.params, b.params);
    }
}

#[test]
fn declarations_consumed_from_body() {
    let src = r#"patch {
        knob k { low: 1Hz, high: 2Hz }
    }"#;
    let r = rewritten(src);
    let any_decl = r
        .patch
        .body
        .iter()
        .any(|s| matches!(s, Statement::HostControl(_)));
    assert!(!any_decl, "host control declarations must be stripped");
}

#[test]
fn rename_only_preserves_alias_count() {
    let before = rewritten(
        "patch { knob a { low: 1Hz, high: 2Hz } knob b { low: 1Hz, high: 2Hz } }",
    );
    let after = rewritten(
        "patch { knob a { low: 1Hz, high: 2Hz } knob c { low: 1Hz, high: 2Hz } }",
    );
    let bm = find_module(&before, SYNTH_HOST_CONTROL).unwrap();
    let am = find_module(&after, SYNTH_HOST_CONTROL).unwrap();
    assert_eq!(aliases(bm).len(), aliases(am).len());
}

// ─── Manifest (ticket 0810) ─────────────────────────────────────────────────

fn manifest(src: &str) -> patches_dsl::host_control_manifest::HostControlManifest {
    let file = parse(src).expect("parse ok");
    desugar_host_controls(file.clone()).expect("desugar ok").1
}

#[test]
fn manifest_is_empty_when_no_declarations() {
    let m = manifest("patch { module osc : Osc() osc.out -> osc.in }");
    assert!(m.is_empty());
}

#[test]
fn manifest_alphabetical_slot_ordering_matches_expander() {
    use patches_dsl::host_control_manifest::HostControlKind as K;
    let src = r#"patch {
        knob zeta { low: 1, high: 2 }
        trigger fire { }
        toggle alpha { default: true }
        slider mu { low: 0, high: 1 }
    }"#;
    let m = manifest(src);
    let names: Vec<&str> = m.iter().map(|d| d.name.as_str()).collect();
    let slots: Vec<usize> = m.iter().map(|d| d.slot).collect();
    let kinds: Vec<K> = m.iter().map(|d| d.kind).collect();
    assert_eq!(names, vec!["alpha", "fire", "mu", "zeta"]);
    assert_eq!(slots, vec![0, 1, 2, 3]);
    assert_eq!(kinds, vec![K::Toggle, K::Trigger, K::Slider, K::Knob]);
}

#[test]
fn manifest_carries_fields_verbatim() {
    use patches_dsl::host_control_manifest::HostControlParamValue as V;
    let src = r#"patch {
        knob cutoff { low: 20, high: 2000, default: 440 }
    }"#;
    let m = manifest(src);
    assert_eq!(m.len(), 1);
    let d = &m[0];
    assert_eq!(d.name, "cutoff");
    assert_eq!(d.params.get("low"), Some(&V::Int(20)));
    assert_eq!(d.params.get("high"), Some(&V::Int(2000)));
    assert_eq!(d.params.get("default"), Some(&V::Int(440)));
}

#[test]
fn manifest_kind_smoothing_flag_matches_kind() {
    use patches_dsl::host_control_manifest::HostControlKind as K;
    assert!(K::Knob.smoothed());
    assert!(K::Slider.smoothed());
    assert!(!K::Toggle.smoothed());
    assert!(!K::Trigger.smoothed());
}

#[test]
fn manifest_provenance_points_at_declaration_site() {
    let src = "patch { knob k { low: 1, high: 2 } }";
    let m = manifest(src);
    let d = &m[0];
    let decl_start = src.find("knob").unwrap();
    assert_eq!(d.source.site.start, decl_start);
}

// ─── Helper trait: surface the index name from a PortRef ────────────────────

trait PortRefExt {
    fn index_name(&self) -> Option<&str>;
}

impl PortRefExt for patches_dsl::ast::PortRef {
    fn index_name(&self) -> Option<&str> {
        match &self.index {
            Some(PortIndex::Name { name, .. }) => Some(name.as_str()),
            _ => None,
        }
    }
}
