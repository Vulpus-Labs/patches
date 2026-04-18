use super::*;

// ─── Phase 1: shallow scan ──────────────────────────────────────────

#[test]
fn scan_no_templates() {
    let file = parse(
        r#"
patch {
module osc : Osc
module out : AudioOut
osc.sine -> out.in_left
}
"#,
    );
    let decl = shallow_scan(&file);
    assert_eq!(decl.modules.len(), 2);
    assert!(decl.templates.is_empty());
}

#[test]
fn scan_with_templates() {
    let file = parse(
        r#"
template voice(attack: float = 0.01) {
in:  voct, gate
out: audio

module osc : Osc
module env : Adsr
}

patch {
module v : voice
module out : AudioOut
}
"#,
    );
    let decl = shallow_scan(&file);
    assert_eq!(decl.templates.len(), 1);
    let tmpl = &decl.templates["voice"];
    let in_port_names: Vec<&str> = tmpl.in_ports.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(in_port_names, vec!["voct", "gate"]);
    let out_port_names: Vec<&str> = tmpl.out_ports.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(out_port_names, vec!["audio"]);
    assert_eq!(tmpl.body_type_refs, vec!["Osc", "Adsr"]);
}

#[test]
fn pattern_and_song_declarations_scanned() {
    let file = parse(
        r#"
pattern drums {
kick: x . x .
snare: . x . x
}

song my_song(drums) {
play {
    drums
    drums
}
}

patch {}
"#,
    );
    let decl = shallow_scan(&file);
    assert_eq!(decl.patterns.len(), 1);
    assert!(decl.patterns.contains_key("drums"));
    let pat = &decl.patterns["drums"];
    assert_eq!(pat.channel_count, 2);
    assert_eq!(pat.step_count, 4);

    assert_eq!(decl.songs.len(), 1);
    assert!(decl.songs.contains_key("my_song"));
    let song = &decl.songs["my_song"];
    assert_eq!(song.channel_names, vec!["drums"]);
    assert_eq!(song.rows.len(), 1);
}
