//! Tap-target desugaring (ADR 0054 §§2, 3, 6 — superseded in part by
//! ADR 0059 §4).
//!
//! Walks a parsed [`File`], collects every top-level tap target,
//! synthesises one `~tap` module instance carrying per-channel
//! `slot_offset: int` and `kind: enum` parameters, rewrites each cable
//! to land on the matching input port (`mono_in[name]`,
//! `stereo_in[name]`, or `trigger_in[name]`) on the synthetic instance,
//! and returns the [`Manifest`] for the observer side. Tap names live
//! in the manifest only — the audio thread never sees them.
//!
//! The synthetic module name uses the reserved `~` prefix that user
//! source can't write; the lexer in `grammar.pest` rejects `~` in
//! identifiers, so this name is guaranteed unique. Tap identity is
//! `(tap_type, name)` and slot ordering follows source location
//! (ADR 0059 §6); two taps of different type sharing a name (e.g.
//! `~trigger_led(kick)` and `~meter(kick)`) coexist as separate
//! synthetic channels. Multiple cables referring to the same
//! `(kind, name)` collapse onto one channel; the first source
//! occurrence determines its slot.

use crate::ast::*;
use crate::manifest::{Manifest, TapDescriptor, TapType};
use crate::provenance::Provenance;

/// Synthetic instance name for the unified tap module.
pub const SYNTH_TAP: &str = "~tap";

/// Module-side type name registered in `patches-modules`.
const TYPE_TAP: &str = "Tap";

/// Per-channel cable kind. Mirrors `patches_modules::TapKind` (kept
/// duplicated here to keep `patches-dsl` free of a runtime dependency
/// on the module crate).
#[derive(Clone, Copy)]
enum Kind { Mono, Stereo, Trigger }

impl Kind {
    fn as_enum_str(self) -> &'static str {
        match self {
            Kind::Mono => "mono",
            Kind::Stereo => "stereo",
            Kind::Trigger => "trigger",
        }
    }
    fn port_name(self) -> &'static str {
        match self {
            Kind::Mono => "mono_in",
            Kind::Stereo => "stereo_in",
            Kind::Trigger => "trigger_in",
        }
    }
    /// Slots claimed by a channel of this kind (ADR 0059 §5). Stereo
    /// reserves two consecutive slots (`L` at offset, `R` at offset+1).
    fn width(self) -> u8 {
        match self {
            Kind::Stereo => 2,
            Kind::Mono | Kind::Trigger => 1,
        }
    }
}

/// Decide which input port a tap target lands on, per ADR 0059 §4.
/// `trigger_led` → `Trigger`, `stereo_meter` → `Stereo`, everything else
/// (`meter`, `osc`, `spectrum`, `gate_led`) → `Mono`. Compound taps mix
/// only mono components today (validated in `validate_tap_components`).
fn classify(tap: &TapTarget) -> Kind {
    if tap.components.iter().any(|c| c.name == "trigger_led") {
        Kind::Trigger
    } else if tap.components.iter().any(|c| c.name == "stereo_meter") {
        Kind::Stereo
    } else {
        Kind::Mono
    }
}

/// Rewrite `file.patch.body` so every tap-endpoint cable lands on the
/// synthetic `~tap` module instance, and return the observer manifest.
///
/// If the patch contains no tap targets, returns the file unchanged
/// (clone) and an empty manifest.
pub fn desugar_taps(file: &File) -> (File, Manifest) {
    // 1. Walk source in order, collecting every TapTarget occurrence.
    // Each `(kind, name)` deduplicates onto a single synthetic channel;
    // the first occurrence sets its slot. Multiple cables to the same
    // tap reuse that channel.
    let mut occurrences: Vec<TapTarget> = Vec::new();
    for stmt in &file.patch.body {
        if let Statement::Connection(c) = stmt {
            if let CableEndpoint::Tap(t) = &c.lhs {
                occurrences.push(t.clone());
            }
            if let CableEndpoint::Tap(t) = &c.rhs {
                occurrences.push(t.clone());
            }
        }
    }

    if occurrences.is_empty() {
        return (file.clone(), Vec::new());
    }

    // 2. Coalesce by `(kind, name)` preserving source order. Same-kind
    // taps that share a name fold into one channel — their components
    // union onto a single backplane slot, so writing `~meter(kick)`,
    // `~osc(kick)` and `~spectrum(kick)` separately is equivalent to
    // the compound `~meter+osc+spectrum(kick)`. The first occurrence
    // sets the channel's source span; cables from matching producers
    // collapse to one edge in step 7.
    let mut unique: Vec<TapTarget> = Vec::new();
    let mut index_of: std::collections::HashMap<(&'static str, String), usize> =
        std::collections::HashMap::new();
    for t in &occurrences {
        let key = (classify(t).as_enum_str(), t.name.name.clone());
        if let Some(&i) = index_of.get(&key) {
            for c in &t.components {
                if !unique[i].components.iter().any(|e| e.name == c.name) {
                    unique[i].components.push(c.clone());
                }
            }
        } else {
            index_of.insert(key, unique.len());
            unique.push(t.clone());
        }
    }
    let kinds: Vec<Kind> = unique.iter().map(classify).collect();

    // 3. Sequential slot assignment in source order, advancing by the
    // per-channel width (ADR 0059 §5). Renaming a single tap shifts
    // only its name index entry; slot offsets stay stable.
    let mut slot_of = vec![0usize; unique.len()];
    let mut next_slot = 0usize;
    for i in 0..unique.len() {
        slot_of[i] = next_slot;
        next_slot += kinds[i].width() as usize;
    }

    // 4. Per-channel synthetic alias: `<kind>_<name>` so two taps of
    // different type with the same name produce distinct alias tokens
    // (the synth module's `channels: [...]` list cannot contain
    // duplicates).
    let aliases: Vec<String> = unique
        .iter()
        .zip(kinds.iter())
        .map(|(t, k)| format!("{}_{}", k.as_enum_str(), t.name.name))
        .collect();

    // 5. Synthesise the single `~tap` module declaration.
    let mut new_body: Vec<Statement> = Vec::new();
    new_body.push(Statement::Module(synth_tap_module(&unique, &slot_of, &kinds, &aliases)));

    // 6. `(kind, name)` → (port-name, alias) lookup used by the
    // endpoint rewriter.
    let target_for: std::collections::HashMap<(&'static str, String), (&'static str, String)> =
        unique
            .iter()
            .zip(kinds.iter())
            .zip(aliases.iter())
            .map(|((t, k), alias)| {
                ((k.as_enum_str(), t.name.name.clone()), (k.port_name(), alias.clone()))
            })
            .collect();

    // 7. Rewrite each connection. After coalescing, several source
    // cables can rewrite to the same `(from, to)` pair — e.g.
    // `kick.out -> ~meter(kick)` and `kick.out -> ~osc(kick)` both
    // become `kick.out -> ~tap.mono_in[mono_kick]`. Drop the duplicate
    // so the connectivity validator doesn't see a phantom
    // input-already-connected error. Distinct producers fanning into
    // the same tap channel are still rejected (correctly): they are
    // genuinely two cables fighting for one input.
    let mut emitted_tap_edges: std::collections::HashSet<EdgeKey> =
        std::collections::HashSet::new();
    for stmt in &file.patch.body {
        match stmt {
            Statement::Connection(c) => {
                let rewritten = rewrite_connection(c, &target_for);
                if let Some(key) = tap_edge_key(&rewritten) {
                    if !emitted_tap_edges.insert(key) {
                        continue;
                    }
                }
                new_body.push(Statement::Connection(rewritten));
            }
            other => new_body.push(other.clone()),
        }
    }

    // 8. Build manifest in source order (slot order = declaration
    // order). Stereo channels publish two scalar entries — `foo/left`
    // at `slot` and `foo/right` at `slot + 1` — so observers can group
    // them into a single paired widget by stem (ADR 0059 §7).
    let mut manifest: Manifest = Vec::with_capacity(unique.len());
    for i in 0..unique.len() {
        let tap = &unique[i];
        let components: Vec<TapType> = tap
            .components
            .iter()
            .map(|c| TapType::from_ast_name(&c.name).expect("validated component"))
            .collect();
        match kinds[i] {
            Kind::Stereo => {
                manifest.push(TapDescriptor {
                    slot: slot_of[i],
                    width: 1,
                    name: format!("{}/left", tap.name.name),
                    components: components.clone(),
                    source: Provenance::root(tap.span),
                });
                manifest.push(TapDescriptor {
                    slot: slot_of[i] + 1,
                    width: 1,
                    name: format!("{}/right", tap.name.name),
                    components,
                    source: Provenance::root(tap.span),
                });
            }
            Kind::Mono | Kind::Trigger => {
                manifest.push(TapDescriptor {
                    slot: slot_of[i],
                    width: 1,
                    name: tap.name.name.clone(),
                    components,
                    source: Provenance::root(tap.span),
                });
            }
        }
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

/// Build a synthetic `module ~tap : Tap(channels: [a, b, ...]) {
/// @a: { slot_offset: <slot>, kind: <kind> }, @b: { ... } }` declaration.
fn synth_tap_module(
    taps: &[TapTarget],
    slot_of: &[usize],
    kinds: &[Kind],
    aliases: &[String],
) -> ModuleDecl {
    let span = synth_span();
    let alias_list: Vec<Ident> = aliases
        .iter()
        .map(|a| Ident { name: a.clone(), span })
        .collect();
    let call_block = Some(CallBlock {
        args: vec![CallArg::Bare {
            value: ShapeValue::AliasList(alias_list),
            span,
        }],
        span,
    });

    let params: Vec<ParamEntry> = aliases
        .iter()
        .enumerate()
        .map(|(i, alias)| ParamEntry::AtBlock {
            index: AtBlockIndex::Alias(alias.clone()),
            entries: vec![
                (
                    Ident { name: "slot_offset".into(), span },
                    Value::Scalar(Scalar::Int(slot_of[i] as i64)),
                ),
                (
                    Ident { name: "kind".into(), span },
                    Value::Scalar(Scalar::Str(kinds[i].as_enum_str().to_owned())),
                ),
            ],
            span,
        })
        .collect();
    let _ = taps; // Names are sourced via `aliases`; taps retained for parity.

    ModuleDecl {
        name: Ident { name: SYNTH_TAP.to_owned(), span },
        type_name: Ident { name: TYPE_TAP.to_owned(), span },
        call_block,
        params,
        span,
    }
}

/// Replace any tap-endpoint side of a connection with a port_ref to the
/// synthetic tap module, indexed by alias.
fn rewrite_connection(
    c: &Connection,
    target_for: &std::collections::HashMap<(&'static str, String), (&'static str, String)>,
) -> Connection {
    let lhs = rewrite_endpoint(&c.lhs, target_for);
    let rhs = rewrite_endpoint(&c.rhs, target_for);
    Connection { lhs, arrow: c.arrow.clone(), rhs, span: c.span }
}

fn rewrite_endpoint(
    ep: &CableEndpoint,
    target_for: &std::collections::HashMap<(&'static str, String), (&'static str, String)>,
) -> CableEndpoint {
    match ep {
        CableEndpoint::Port(p) => CableEndpoint::Port(p.clone()),
        CableEndpoint::Tap(t) => {
            let key = (classify(t).as_enum_str(), t.name.name.clone());
            let (port_name, alias) = target_for
                .get(&key)
                .expect("tap (kind, name) was collected; lookup must succeed");
            CableEndpoint::Port(PortRef {
                module: SYNTH_TAP.to_owned(),
                port: PortLabel::Literal((*port_name).to_owned()),
                index: Some(PortIndex::Name { name: alias.clone(), arity_marker: false }),
                span: t.span,
            })
        }
    }
}

/// Canonical key for deduplicating coalesced tap-edge cables. Keyed on
/// the source-port text plus the synthetic destination port; equal keys
/// mean the same producer was already wired to the same coalesced
/// channel via another component declaration.
type EdgeKey = (String, String);

fn tap_edge_key(c: &Connection) -> Option<EdgeKey> {
    let (from, to) = match c.arrow.direction {
        Direction::Forward => (c.lhs.as_port()?, c.rhs.as_port()?),
        Direction::Backward => (c.rhs.as_port()?, c.lhs.as_port()?),
    };
    if to.module != SYNTH_TAP {
        return None;
    }
    Some((portref_canonical(from), portref_canonical(to)))
}

fn portref_canonical(p: &PortRef) -> String {
    let port = match &p.port {
        PortLabel::Literal(s) => s.clone(),
        PortLabel::Param(s) => format!("<{s}>"),
    };
    let idx = match &p.index {
        None => String::new(),
        Some(PortIndex::Literal(n)) => format!("[{n}]"),
        Some(PortIndex::Name { name, arity_marker }) => {
            let star = if *arity_marker { "*" } else { "" };
            format!("[{star}{name}]")
        }
    };
    format!("{}.{port}{idx}", p.module)
}

fn synth_span() -> Span {
    Span::new(SourceId::SYNTHETIC, 0, 0)
}
