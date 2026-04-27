//! Tap-target desugaring (ticket 0697, ADR 0054 §§2, 3, 6).
//!
//! Walks a parsed [`File`], collects every top-level tap target,
//! synthesises one `~audio_tap` and/or one `~trigger_tap` module
//! instance carrying a per-channel `slot_offset: int` parameter,
//! rewrites each cable to land on the synthetic instance, and returns
//! the [`Manifest`] for the observer side. Tap names live in the
//! manifest only — the audio thread never sees them.
//!
//! The synthetic module names use the reserved `~` prefix that user
//! source can't write; the lexer in `grammar.pest` rejects `~` in
//! identifiers, so these names are guaranteed unique.

use crate::ast::*;
use crate::manifest::{Manifest, TapDescriptor, TapType};
use crate::provenance::Provenance;

/// Synthetic instance name for the audio-cable tap module.
pub const SYNTH_AUDIO_TAP: &str = "~audio_tap";
/// Synthetic instance name for the trigger-cable tap module.
pub const SYNTH_TRIGGER_TAP: &str = "~trigger_tap";

/// Module-side type names. Phase 2 will register these with the module
/// registry.
const TYPE_AUDIO_TAP: &str = "AudioTap";
const TYPE_TRIGGER_TAP: &str = "TriggerTap";

/// Rewrite `file.patch.body` so every tap-endpoint cable lands on a
/// synthetic tap-module instance, and return the observer manifest.
///
/// If the patch contains no tap targets, returns the file unchanged
/// (clone) and an empty manifest.
pub fn desugar_taps(file: &File) -> (File, Manifest) {
    // 1. Collect every TapTarget that appears as a cable endpoint.
    let mut taps: Vec<TapTarget> = Vec::new();
    for stmt in &file.patch.body {
        if let Statement::Connection(c) = stmt {
            if let CableEndpoint::Tap(t) = &c.lhs {
                taps.push(t.clone());
            }
            if let CableEndpoint::Tap(t) = &c.rhs {
                taps.push(t.clone());
            }
        }
    }

    if taps.is_empty() {
        return (file.clone(), Vec::new());
    }

    // 2. Global alphabetical sort → slot index per tap.
    let mut sorted: Vec<usize> = (0..taps.len()).collect();
    sorted.sort_by(|&a, &b| taps[a].name.name.cmp(&taps[b].name.name));
    let mut slot_of = vec![0usize; taps.len()];
    for (slot, &orig) in sorted.iter().enumerate() {
        slot_of[orig] = slot;
    }

    // 3. Partition by underlying module (audio vs trigger).
    let is_audio: Vec<bool> = taps
        .iter()
        .map(|t| !t.components.iter().any(|c| c.name == "trigger_led"))
        .collect();
    let mut audio_idx: Vec<usize> = (0..taps.len()).filter(|&i| is_audio[i]).collect();
    let mut trigger_idx: Vec<usize> = (0..taps.len()).filter(|&i| !is_audio[i]).collect();
    audio_idx.sort_by(|&a, &b| taps[a].name.name.cmp(&taps[b].name.name));
    trigger_idx.sort_by(|&a, &b| taps[a].name.name.cmp(&taps[b].name.name));

    // 4. Synthesise tap-module declarations.
    let mut new_body: Vec<Statement> = Vec::new();
    if !audio_idx.is_empty() {
        new_body.push(Statement::Module(synth_module(
            SYNTH_AUDIO_TAP,
            TYPE_AUDIO_TAP,
            &audio_idx,
            &taps,
            &slot_of,
        )));
    }
    if !trigger_idx.is_empty() {
        new_body.push(Statement::Module(synth_module(
            SYNTH_TRIGGER_TAP,
            TYPE_TRIGGER_TAP,
            &trigger_idx,
            &taps,
            &slot_of,
        )));
    }

    // Per-tap channel-name → (synthetic-module, alias-name) lookup. Used
    // to rewrite each cable's tap endpoint to a port_ref on the synth.
    let target_for: std::collections::HashMap<String, (&'static str, String)> = taps
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let module = if is_audio[i] { SYNTH_AUDIO_TAP } else { SYNTH_TRIGGER_TAP };
            (t.name.name.clone(), (module, t.name.name.clone()))
        })
        .collect();

    // 5. Rewrite each connection.
    for stmt in &file.patch.body {
        match stmt {
            Statement::Connection(c) => {
                new_body.push(Statement::Connection(rewrite_connection(c, &target_for)));
            }
            other => new_body.push(other.clone()),
        }
    }

    // 6. Build manifest.
    let mut manifest: Manifest = Vec::with_capacity(taps.len());
    for &orig in &sorted {
        let tap = &taps[orig];
        manifest.push(TapDescriptor {
            slot: slot_of[orig],
            name: tap.name.name.clone(),
            components: tap
                .components
                .iter()
                .map(|c| TapType::from_ast_name(&c.name).expect("validated component"))
                .collect(),
            source: Provenance::root(tap.span),
        });
    }

    let new_file = File {
        includes: file.includes.clone(),
        templates: file.templates.clone(),
        patterns: file.patterns.clone(),
        songs: file.songs.clone(),
        sections: file.sections.clone(),
        patch: Patch { body: new_body, span: file.patch.span },
        span: file.span,
    };
    (new_file, manifest)
}

/// Build a synthetic `module ~audio_tap : AudioTap(channels: [a, b, ...]) {
/// @a: { slot_offset: <slot> }, @b: { ... } }` declaration.
fn synth_module(
    inst_name: &str,
    type_name: &str,
    members: &[usize],
    taps: &[TapTarget],
    slot_of: &[usize],
) -> ModuleDecl {
    let span = synth_span();
    let alias_list: Vec<Ident> = members
        .iter()
        .map(|&i| Ident { name: taps[i].name.name.clone(), span })
        .collect();
    let shape = vec![ShapeArg {
        name: Ident { name: "channels".into(), span },
        value: ShapeArgValue::AliasList(alias_list),
        span,
    }];

    let params: Vec<ParamEntry> = members
        .iter()
        .map(|&i| {
            let alias = taps[i].name.name.clone();
            ParamEntry::AtBlock {
                index: AtBlockIndex::Alias(alias),
                entries: vec![(
                    Ident { name: "slot_offset".into(), span },
                    Value::Scalar(Scalar::Int(slot_of[i] as i64)),
                )],
                span,
            }
        })
        .collect();

    ModuleDecl {
        name: Ident { name: inst_name.to_owned(), span },
        type_name: Ident { name: type_name.to_owned(), span },
        shape,
        params,
        span,
    }
}

/// Replace any tap-endpoint side of a connection with a port_ref to the
/// matching synthetic tap module, indexed by alias.
fn rewrite_connection(
    c: &Connection,
    target_for: &std::collections::HashMap<String, (&'static str, String)>,
) -> Connection {
    let lhs = rewrite_endpoint(&c.lhs, target_for);
    let rhs = rewrite_endpoint(&c.rhs, target_for);
    Connection { lhs, arrow: c.arrow.clone(), rhs, span: c.span }
}

fn rewrite_endpoint(
    ep: &CableEndpoint,
    target_for: &std::collections::HashMap<String, (&'static str, String)>,
) -> CableEndpoint {
    match ep {
        CableEndpoint::Port(p) => CableEndpoint::Port(p.clone()),
        CableEndpoint::Tap(t) => {
            let (module, alias) = target_for
                .get(&t.name.name)
                .expect("tap name was collected; lookup must succeed");
            CableEndpoint::Port(PortRef {
                module: (*module).to_owned(),
                port: PortLabel::Literal("in".to_owned()),
                index: Some(PortIndex::Name { name: alias.clone(), arity_marker: false }),
                span: t.span,
            })
        }
    }
}

fn synth_span() -> Span {
    Span::new(SourceId::SYNTHETIC, 0, 0)
}
