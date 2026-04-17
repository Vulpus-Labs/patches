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
