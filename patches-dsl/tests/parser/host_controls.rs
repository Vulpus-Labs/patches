//! Parser + validate tests for host control blocks (ADR 0057, ticket 0807).

use patches_dsl::ast::{
    CableEndpoint, HostControlKind, Scalar, Statement, Value,
};
use patches_dsl::parse;
use patches_dsl::StructuralCode;

fn host_controls(src: &str) -> Vec<patches_dsl::ast::HostControlBlock> {
    let file = parse(src).expect("parse ok");
    file.patch
        .body
        .into_iter()
        .filter_map(|s| match s {
            Statement::HostControl(hc) => Some(hc),
            _ => None,
        })
        .collect()
}

#[test]
fn parses_knob_block() {
    let src = r#"patch {
        knob frequency_knob {
            displayName: "Frequency",
            unit: "Hz",
            low: 21Hz,
            high: 2001Hz,
            default: 441Hz,
            taper: exp,
        }
    }"#;
    let hcs = host_controls(src);
    assert_eq!(hcs.len(), 1);
    let hc = &hcs[0];
    assert_eq!(hc.kind, HostControlKind::Knob);
    assert_eq!(hc.name.name, "frequency_knob");
    assert_eq!(hc.fields.len(), 6);
    let display = hc
        .fields
        .iter()
        .find(|f| f.name.name == "displayName")
        .unwrap();
    assert!(matches!(&display.value, Value::Scalar(Scalar::Str(s)) if s == "Frequency"));
    let taper = hc.fields.iter().find(|f| f.name.name == "taper").unwrap();
    assert!(matches!(&taper.value, Value::Scalar(Scalar::Str(s)) if s == "exp"));
}

#[test]
fn parses_slider_and_toggle() {
    let src = r#"patch {
        slider vca_attack { low: 0.001s, high: 2.0s, default: 0.01s }
        toggle reverb_bypass { default: false }
    }"#;
    let hcs = host_controls(src);
    assert_eq!(hcs.len(), 2);
    assert_eq!(hcs[0].kind, HostControlKind::Slider);
    assert_eq!(hcs[1].kind, HostControlKind::Toggle);
    assert_eq!(hcs[1].name.name, "reverb_bypass");
}

#[test]
fn parses_trigger_no_required_fields() {
    // `trigger` is a one-shot button with no required fields.
    let src = r#"patch {
        trigger fire { }
        trigger labelled { displayName: "Fire" }
    }"#;
    let hcs = host_controls(src);
    assert_eq!(hcs.len(), 2);
    assert_eq!(hcs[0].kind, HostControlKind::Trigger);
    assert_eq!(hcs[0].name.name, "fire");
    assert!(hcs[0].fields.is_empty());
    assert_eq!(hcs[1].fields.len(), 1);
    // Must also pass validate.
    let file = parse(src).unwrap();
    patches_dsl::validate::validate(&file).expect("trigger validates");
}

#[test]
fn bare_name_reference_in_cable() {
    let src = r#"patch {
        knob cutoff { low: 21Hz, high: 2001Hz }
        module filter : Filter()
        cutoff -> filter.voct
    }"#;
    let file = parse(src).expect("parse ok");
    let conn = file
        .patch
        .body
        .iter()
        .find_map(|s| if let Statement::Connection(c) = s { Some(c) } else { None })
        .unwrap();
    let CableEndpoint::HostControlRef(id) = &conn.lhs else {
        panic!("expected host control ref, got {:?}", conn.lhs);
    };
    assert_eq!(id.name, "cutoff");
    assert!(matches!(conn.rhs, CableEndpoint::Port(_)));
}

#[test]
fn knob_keyword_word_boundary() {
    // `knob_count` is an ordinary identifier; the `knob` keyword has a
    // word-boundary lookahead so this still parses as a module decl.
    let src = r#"patch {
        module knob_count : Const()
        knob_count.out -> knob_count.in
    }"#;
    parse(src).expect("knob_count parses as a plain ident");
}

// ─── Negative parses ─────────────────────────────────────────────────────────

#[test]
fn block_in_template_rejected_by_validate() {
    let src = r#"
    template bad {
        out: x
        knob k1 { low: 1Hz, high: 1Hz }
    }
    patch { module b : bad() }
    "#;
    let file = parse(src).expect("parse ok");
    let err = patches_dsl::validate::validate(&file).expect_err("validate rejects");
    assert_eq!(err.code, StructuralCode::HostControlInTemplate);
}

#[test]
fn knob_missing_high_rejected() {
    let src = "patch { knob k { low: 21Hz } }";
    let file = parse(src).expect("parse ok");
    let err = patches_dsl::validate::validate(&file).expect_err("validate rejects");
    assert_eq!(err.code, StructuralCode::HostControlMissingField);
}

#[test]
fn slider_missing_low_rejected() {
    let src = "patch { slider s { high: 1.0 } }";
    let file = parse(src).expect("parse ok");
    let err = patches_dsl::validate::validate(&file).expect_err("validate rejects");
    assert_eq!(err.code, StructuralCode::HostControlMissingField);
}

#[test]
fn toggle_missing_default_rejected() {
    let src = "patch { toggle t { } }";
    let file = parse(src).expect("parse ok");
    let err = patches_dsl::validate::validate(&file).expect_err("validate rejects");
    assert_eq!(err.code, StructuralCode::HostControlMissingField);
}

#[test]
fn name_collision_with_module_instance() {
    let src = r#"patch {
        knob shared { low: 1Hz, high: 1Hz }
        module shared : Const()
    }"#;
    let file = parse(src).expect("parse ok");
    let err = patches_dsl::validate::validate(&file).expect_err("validate rejects");
    assert_eq!(err.code, StructuralCode::HostControlNameCollision);
}

#[test]
fn duplicate_host_control_name_rejected() {
    let src = r#"patch {
        knob k { low: 1Hz, high: 1Hz }
        knob k { low: 1Hz, high: 2Hz }
    }"#;
    let file = parse(src).expect("parse ok");
    let err = patches_dsl::validate::validate(&file).expect_err("validate rejects");
    assert_eq!(err.code, StructuralCode::HostControlDuplicateName);
}

#[test]
fn duplicate_field_rejected() {
    let src = "patch { knob k { low: 1Hz, low: 1Hz, high: 2Hz } }";
    let file = parse(src).expect("parse ok");
    let err = patches_dsl::validate::validate(&file).expect_err("validate rejects");
    assert_eq!(err.code, StructuralCode::HostControlDuplicateField);
}
