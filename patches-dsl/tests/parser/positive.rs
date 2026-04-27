//! Positive parser fixtures — smoke tests that known-good sources parse.

use patches_dsl::parse;

#[test]
fn positive_fixtures_parse_ok() {
    let fixtures: &[(&str, &str)] = &[
        ("simple",             include_str!("../fixtures/simple.patches")),
        ("scaled_and_indexed", include_str!("../fixtures/scaled_and_indexed.patches")),
        ("voice_template",     include_str!("../fixtures/voice_template.patches")),
        ("nested_templates",   include_str!("../fixtures/nested_templates.patches")),
    ];
    for &(name, src) in fixtures {
        assert!(parse(src).is_ok(), "expected Ok for {name}.patches");
    }
}

#[test]
fn positive_unit_literals() {
    let src = include_str!("../fixtures/unit_literals.patches");
    assert!(parse(src).is_ok(), "expected Ok for unit_literals.patches");
}

#[test]
fn fan_in_with_backward_arrow_parses() {
    use patches_dsl::ast::{Direction, Statement};
    let src = "patch {
        module mix : Mixer
        module del : Delay
        mix.return_a_right, ~meter(bar) <- del.out_right
    }";
    let file = parse(src).expect("parse ok");
    let conns: Vec<_> = file
        .patch
        .body
        .iter()
        .filter_map(|s| if let Statement::Connection(c) = s { Some(c.clone()) } else { None })
        .collect();
    assert_eq!(conns.len(), 2);
    assert!(conns.iter().all(|c| c.arrow.direction == Direction::Backward));
}

#[test]
fn fan_in_rejects_list_on_rhs_with_backward_arrow() {
    let src = "patch {
        module mix : Mixer
        module del : Delay
        mix.in <- del.a, del.b
    }";
    assert!(parse(src).is_err(), "list on RHS of `<-` must be rejected");
}

#[test]
fn fan_out_rejects_list_on_lhs_with_forward_arrow() {
    let src = "patch {
        module a : Osc
        module b : Osc
        module out : AudioOut
        a.out, b.out -> out.in_left
    }";
    assert!(parse(src).is_err(), "list on LHS of `->` must be rejected");
}
