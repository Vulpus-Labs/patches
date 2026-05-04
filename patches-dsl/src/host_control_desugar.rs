//! Host-control desugaring (ADR 0057 §2, ticket 0808).
//!
//! Walks a parsed [`File`] (already tap-desugared), collects every top-
//! level `knob` / `slider` / `toggle` / `trigger` declaration, sorts each
//! cable-kind group alphabetically by control name, and synthesises one
//! module instance per non-empty group:
//!
//! - knob / slider / toggle → `~host_control : HostControl(channels: [...])`
//!   carrying `Mono+Audio` outputs.
//! - trigger → `~host_control_trigger : HostControlTrigger(channels: [...])`
//!   carrying `Mono+Trigger` outputs.
//!
//! Bare-name [`CableEndpoint::HostControlRef`]s are rewritten into
//! `PortRef`s on the matching synthesised instance, indexed by control
//! name. Declaration statements are consumed (not re-emitted).
//!
//! The `~` reserved-prefix rule (ADR 0054 §2) is what guarantees the
//! synthetic instance names cannot collide with user modules — pest
//! rejects `~` inside identifiers, so a user `module ~host_control ...`
//! cannot exist.

use std::collections::HashMap;

use crate::ast::*;
use crate::expand::ExpandError;
use crate::structural::StructuralCode as Code;

/// Synthesised audio host-control module instance name.
pub const SYNTH_HOST_CONTROL: &str = "~host_control";
/// Synthesised trigger host-control module instance name.
pub const SYNTH_HOST_CONTROL_TRIGGER: &str = "~host_control_trigger";

const TYPE_HOST_CONTROL: &str = "HostControl";
const TYPE_HOST_CONTROL_TRIGGER: &str = "HostControlTrigger";

/// Cable-kind grouping for host controls. Audio covers knob, slider, and
/// toggle (Mono+Audio); Trigger covers the one-shot trigger kind
/// (Mono+Trigger).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Audio,
    Trigger,
}

fn group_of(kind: HostControlKind) -> Group {
    match kind {
        HostControlKind::Knob | HostControlKind::Slider | HostControlKind::Toggle => Group::Audio,
        HostControlKind::Trigger => Group::Trigger,
    }
}

fn synth_name(g: Group) -> &'static str {
    match g {
        Group::Audio => SYNTH_HOST_CONTROL,
        Group::Trigger => SYNTH_HOST_CONTROL_TRIGGER,
    }
}

fn type_name(g: Group) -> &'static str {
    match g {
        Group::Audio => TYPE_HOST_CONTROL,
        Group::Trigger => TYPE_HOST_CONTROL_TRIGGER,
    }
}

/// Rewrite `file.patch.body` so every host-control declaration lowers
/// into a synthesised instance and every bare-name reference is rewritten
/// to a `PortRef` on the matching instance. Returns the new file.
///
/// If the patch contains no host-control declarations, returns the file
/// unchanged.
pub fn desugar_host_controls(file: &File) -> Result<File, ExpandError> {
    // 1. Collect declarations from the patch body.
    let mut decls_audio: Vec<&HostControlBlock> = Vec::new();
    let mut decls_trigger: Vec<&HostControlBlock> = Vec::new();
    for stmt in &file.patch.body {
        if let Statement::HostControl(hc) = stmt {
            match group_of(hc.kind) {
                Group::Audio => decls_audio.push(hc),
                Group::Trigger => decls_trigger.push(hc),
            }
        }
    }
    // Even with no declarations, a stray bare-name ref must be rejected.
    if decls_audio.is_empty() && decls_trigger.is_empty() {
        reject_unresolved_refs(file, &HashMap::new())?;
        return Ok(file.clone());
    }

    // 2. Sort each group alphabetically by control name (ADR 0057 §3).
    decls_audio.sort_by(|a, b| a.name.name.cmp(&b.name.name));
    decls_trigger.sort_by(|a, b| a.name.name.cmp(&b.name.name));

    // 3. Build per-name lookup: name → (group, slot index within group).
    let mut lookup: HashMap<String, (Group, usize)> = HashMap::new();
    for (i, hc) in decls_audio.iter().enumerate() {
        lookup.insert(hc.name.name.clone(), (Group::Audio, i));
    }
    for (i, hc) in decls_trigger.iter().enumerate() {
        lookup.insert(hc.name.name.clone(), (Group::Trigger, i));
    }

    reject_unresolved_refs(file, &lookup)?;

    // 4. Build new body: one synthesised module per non-empty group,
    //    then the original body minus the consumed declarations and with
    //    bare-name refs rewritten.
    let mut new_body: Vec<Statement> = Vec::new();
    if !decls_audio.is_empty() {
        new_body.push(Statement::Module(synth_module(Group::Audio, &decls_audio)));
    }
    if !decls_trigger.is_empty() {
        new_body.push(Statement::Module(synth_module(Group::Trigger, &decls_trigger)));
    }
    for stmt in &file.patch.body {
        match stmt {
            Statement::HostControl(_) => continue,
            Statement::Connection(c) => {
                new_body.push(Statement::Connection(rewrite_connection(c, &lookup)));
            }
            other => new_body.push(other.clone()),
        }
    }

    Ok(File {
        includes: file.includes.clone(),
        templates: file.templates.clone(),
        patterns: file.patterns.clone(),
        songs: file.songs.clone(),
        sections: file.sections.clone(),
        patch: Patch { body: new_body, span: file.patch.span },
        span: file.span,
    })
}

/// Walk the patch body's connections and report the first
/// `HostControlRef` whose name is not in `lookup`.
fn reject_unresolved_refs(
    file: &File,
    lookup: &HashMap<String, (Group, usize)>,
) -> Result<(), ExpandError> {
    for stmt in &file.patch.body {
        let Statement::Connection(c) = stmt else { continue };
        for ep in [&c.lhs, &c.rhs] {
            if let CableEndpoint::HostControlRef(id) = ep {
                if !lookup.contains_key(&id.name) {
                    return Err(ExpandError::new(
                        Code::HostControlUnknownRef,
                        id.span,
                        format!(
                            "bare reference to undeclared host control {:?}; \
                             declare a top-level `knob` / `slider` / `toggle` / \
                             `trigger` block with this name first",
                            id.name
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn synth_module(group: Group, decls: &[&HostControlBlock]) -> ModuleDecl {
    let span = synth_span();
    let aliases: Vec<Ident> = decls
        .iter()
        .map(|hc| Ident { name: hc.name.name.clone(), span: hc.name.span })
        .collect();

    let call_block = Some(CallBlock {
        args: vec![CallArg::Bare {
            value: ShapeValue::AliasList(aliases),
            span,
        }],
        span,
    });

    let params: Vec<ParamEntry> = decls
        .iter()
        .enumerate()
        .map(|(i, hc)| ParamEntry::AtBlock {
            index: AtBlockIndex::Alias(hc.name.name.clone()),
            entries: vec![
                (
                    Ident { name: "slot_offset".into(), span },
                    Value::Scalar(Scalar::Int(i as i64)),
                ),
                (
                    Ident { name: "kind".into(), span },
                    Value::Scalar(Scalar::Str(hc.kind.as_str().to_owned())),
                ),
            ],
            span: hc.span,
        })
        .collect();

    ModuleDecl {
        name: Ident { name: synth_name(group).to_owned(), span },
        type_name: Ident { name: type_name(group).to_owned(), span },
        call_block,
        params,
        span,
    }
}

fn rewrite_connection(
    c: &Connection,
    lookup: &HashMap<String, (Group, usize)>,
) -> Connection {
    Connection {
        lhs: rewrite_endpoint(&c.lhs, lookup),
        arrow: c.arrow.clone(),
        rhs: rewrite_endpoint(&c.rhs, lookup),
        span: c.span,
    }
}

fn rewrite_endpoint(
    ep: &CableEndpoint,
    lookup: &HashMap<String, (Group, usize)>,
) -> CableEndpoint {
    match ep {
        CableEndpoint::Port(_) | CableEndpoint::Tap(_) => ep.clone(),
        CableEndpoint::HostControlRef(id) => {
            // Unresolved refs were rejected up-front; lookup is total.
            let (group, _) = lookup.get(&id.name).expect("ref already validated");
            CableEndpoint::Port(PortRef {
                module: synth_name(*group).to_owned(),
                port: PortLabel::Literal("out".to_owned()),
                index: Some(PortIndex::Name {
                    name: id.name.clone(),
                    arity_marker: false,
                }),
                span: id.span,
            })
        }
    }
}

fn synth_span() -> Span {
    Span::new(SourceId::SYNTHETIC, 0, 0)
}
