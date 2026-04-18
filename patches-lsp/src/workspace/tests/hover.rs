use super::*;

#[test]
fn hover_on_template_use_shows_expansion() {
    let src = r#"
template voice(n: int) {
in: gate
out: audio
module osc : Osc
module mix : Sum(channels: <n>)
}
patch {
module v : voice(n: 2)
}
"#;
    let tmp = TempDir::new("hover_exp_use");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "v : voice", 0);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("expansion"), "{value}");
    assert!(value.contains("Osc"), "{value}");
    assert!(value.contains("Sum"), "{value}");
}

#[test]
fn hover_on_template_use_shows_fanout_wiring() {
    let src = r#"
template voice() {
in: gate
out: audio
module env1 : Env
module env2 : Env
module mix : Sum(channels: 2)
$.gate -> env1.gate, env2.gate
mix.out -> $.audio
}
patch {
module v : voice
}
"#;
    let tmp = TempDir::new("hover_exp_fanout");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "v : voice", 0);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("env1.gate"), "{value}");
    assert!(value.contains("env2.gate"), "missing fan-out target: {value}");
}

#[test]
fn hover_on_template_use_shows_port_wiring() {
    let src = r#"
template voice() {
in: voct, gate
out: audio
module osc : Osc
module env : Env
$.voct -> osc.voct
$.gate -> env.gate
osc.sine -> $.audio
}
patch {
module v : voice
}
"#;
    let tmp = TempDir::new("hover_exp_wire");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "v : voice", 0);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("**In:**"), "{value}");
    assert!(value.contains("**Out:**"), "{value}");
    assert!(value.contains("voct"), "{value}");
    assert!(value.contains("osc.voct"), "{value}");
    assert!(value.contains("gate"), "{value}");
    assert!(value.contains("env.gate"), "{value}");
    assert!(value.contains("audio"), "{value}");
    assert!(value.contains("osc.sine"), "{value}");
}

#[test]
fn hover_inside_template_body_resolves_channels() {
    let src = r#"
template voice(n: int) {
in: gate
out: audio
module mix : Sum(channels: <n>)
}
patch {
module v : voice(n: 3)
}
"#;
    let tmp = TempDir::new("hover_exp_body");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    // Hover on `mix` inside the template body.
    let pos = position_at(src, "module mix", 7);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("Sum"), "{value}");
    assert!(
        value.contains("channels = 3"),
        "expected resolved channels in hover: {value}"
    );
}

#[test]
fn hover_top_level_fanout_lists_all_targets() {
    let src = r#"
patch {
module osc : Osc
module out : AudioOut
osc.sine -> out.in_left, out.in_right
}
"#;
    let tmp = TempDir::new("hover_exp_fanout_top");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, "osc.sine", 4);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("in_left"), "{value}");
    assert!(value.contains("in_right"), "missing second target: {value}");
    assert!(value.contains("fan-out"), "{value}");
}

#[test]
fn hover_port_shows_expanded_index() {
    let src = r#"
patch {
module mix : Sum(channels: 2)
module out : AudioOut
mix.out -> out.in_left
}
"#;
    let tmp = TempDir::new("hover_exp_port");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    // Hover over `mix.out` — the connection's from side.
    let pos = position_at(src, "mix.out", 4);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(
        value.contains("connection") || value.contains("port"),
        "{value}"
    );
}

#[test]
fn hover_falls_back_on_broken_syntax() {
    let src = "patch {\n    module osc : Osc\n"; // missing `}`
    let tmp = TempDir::new("hover_exp_broken");
    tmp.write("a.patches", src);
    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("a.patches");
    let _ = ws.analyse_flat(&uri, src.to_string());

    let pos = position_at(src, ": Osc", 2);
    // Must not panic; tolerant hover still produces info.
    let h = ws.hover(&uri, pos).expect("fallback hover");
    let value = hover_value(&h);
    assert!(value.contains("Osc"), "{value}");
}

#[test]
fn hover_on_included_template_use_shows_expansion() {
    let tmp = TempDir::new("hover_exp_incl");
    tmp.write(
        "voice.patches",
        "template voice() { in: gate out: audio module osc : Osc osc.sine -> $.audio }\n",
    );
    let parent_src = "include \"voice.patches\"\npatch {\n    module v : voice\n}\n";
    tmp.write("main.patches", parent_src);

    let ws = DocumentWorkspace::new();
    let uri = tmp.uri("main.patches");
    let _ = ws.analyse_flat(&uri, parent_src.to_string());

    let pos = position_at(parent_src, "v : voice", 0);
    let h = ws.hover(&uri, pos).expect("hover");
    let value = hover_value(&h);
    assert!(value.contains("expansion"), "{value}");
    assert!(value.contains("Osc"), "{value}");
}
