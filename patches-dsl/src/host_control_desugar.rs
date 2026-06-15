//! Host-control desugaring (ADR 0057 §2, ticket 0808).
//!
//! Walks a parsed [`File`] (already tap-desugared), collects every top-
//! level `knob` / `slider` / `toggle` / `trigger` declaration, sorts them
//! alphabetically by control name (ADR 0057 §3), and synthesises one
//! `~host_control : HostControl(channels: [...])` module instance.
//!
//! Mirrors the Tap-module approach (ADR 0059 §4): a single synth
//! instance with kind-suffixed output ports — `audio_out[name]` for
//! knob / slider / toggle (Mono+Audio) and `trigger_out[name]` for
//! trigger (Mono+Trigger). Slot offsets are alphabetical across the
//! whole declared set, regardless of kind, so the audio / control
//! sides recompute the same mapping independently from the same input
//! list.
//!
//! `~name` [`CableEndpoint::HostControlRef`]s are rewritten into
//! `PortRef`s on the synth instance, dispatching to `audio_out` or
//! `trigger_out` by the declared kind. Declaration statements are
//! consumed (not re-emitted).
//!
//! A `~name` reference without a matching declaration is not an error
//! (ticket 0868): the desugarer synthesises an implicit `knob name {}`
//! block at the first reference span. Implicit and explicit empty-knob
//! forms travel the same downstream code path and produce identical
//! [`FlatPatch`] output.
//!
//! The `~` reserved-prefix rule (ADR 0054 §2) is what guarantees the
//! synthetic instance name cannot collide with user modules — pest
//! rejects `~` inside identifiers.

use std::collections::HashMap;

use patches_core::ExpectInvariant;

use crate::ast::*;
use crate::expand::ExpandError;
use crate::host_control_manifest::{
    HostControlDescriptor, HostControlKind as ManifestKind, HostControlManifest,
    HostControlParamMap, HostControlParamValue,
};
use crate::provenance::Provenance;

/// Synthesised host-control module instance name.
pub const SYNTH_HOST_CONTROL: &str = "~host_control";
const TYPE_HOST_CONTROL: &str = "HostControl";

/// Output-port name for audio-shaped host controls (knob / slider /
/// toggle).
pub const AUDIO_OUT: &str = "audio_out";
/// Output-port name for trigger-shaped host controls.
pub const TRIGGER_OUT: &str = "trigger_out";

fn out_port(kind: HostControlKind) -> &'static str {
    match kind {
        HostControlKind::Knob | HostControlKind::Slider | HostControlKind::Toggle => AUDIO_OUT,
        HostControlKind::Trigger => TRIGGER_OUT,
    }
}

/// Rewrite `file.patch.body` so every host-control declaration lowers
/// into a synthesised instance and every bare-name reference is rewritten
/// to a `PortRef` on the synth instance. Consumes `file`, mutating its
/// body in place when host controls exist; returns it unchanged otherwise.
///
/// If the patch has no declarations and no bare-name references, returns
/// the file unchanged.
pub fn desugar_host_controls(
    mut file: File,
) -> Result<(File, HostControlManifest), ExpandError> {
    // 1. Collect explicit declarations from the patch body. Clone (cheap
    //    on the declaration; rejected stmts disappear in step 5 either
    //    way).
    let mut decls: Vec<HostControlBlock> = file
        .patch
        .body
        .iter()
        .filter_map(|s| match s {
            Statement::HostControl(hc) => Some(hc.clone()),
            _ => None,
        })
        .collect();

    // 2. Synthesise implicit `knob name {}` blocks for every bare-name
    //    reference without a matching declaration (ticket 0868). The
    //    span is the first reference site; field list is empty, so the
    //    implicit form is byte-identical to an explicit `knob name {}`
    //    downstream.
    let explicit_names: std::collections::HashSet<&str> =
        decls.iter().map(|hc| hc.name.name.as_str()).collect();
    let mut implicit: Vec<(String, Span)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for stmt in &file.patch.body {
        let Statement::Connection(c) = stmt else { continue };
        for ep in [&c.lhs, &c.rhs] {
            if let CableEndpoint::HostControlRef(id) = ep {
                if !explicit_names.contains(id.name.as_str())
                    && seen.insert(id.name.clone())
                {
                    implicit.push((id.name.clone(), id.span));
                }
            }
        }
    }
    for (name, span) in implicit {
        decls.push(HostControlBlock {
            kind: HostControlKind::Knob,
            kind_span: span,
            name: Ident { name, span },
            fields: Vec::new(),
            span,
        });
    }

    if decls.is_empty() {
        return Ok((file, Vec::new()));
    }

    // 3. Sort alphabetically by control name (ADR 0057 §3). Slot offset
    //    is the post-sort index; kind dictates the output port.
    decls.sort_by(|a, b| a.name.name.cmp(&b.name.name));

    // 4. Build name → kind lookup for endpoint rewriting.
    let lookup: HashMap<String, HostControlKind> = decls
        .iter()
        .map(|hc| (hc.name.name.clone(), hc.kind))
        .collect();

    // 5. Build new body: synth module first, then the original body
    //    minus consumed declarations and with bare-name refs rewritten.
    let original_body = std::mem::take(&mut file.patch.body);
    let mut new_body: Vec<Statement> = Vec::with_capacity(original_body.len() + 1);
    let decl_refs: Vec<&HostControlBlock> = decls.iter().collect();
    new_body.push(Statement::Module(synth_module(&decl_refs)));
    for stmt in original_body {
        match stmt {
            Statement::HostControl(_) => continue,
            Statement::Connection(c) => {
                new_body.push(Statement::Connection(rewrite_connection(&c, &lookup)));
            }
            other => new_body.push(other),
        }
    }

    let manifest = build_manifest(&decl_refs);

    file.patch.body = new_body;
    Ok((file, manifest))
}

/// Build a [`HostControlManifest`] from the (already alphabetically
/// sorted) declarations. Slot equals the index in the sorted list.
fn build_manifest(decls: &[&HostControlBlock]) -> HostControlManifest {
    decls
        .iter()
        .enumerate()
        .map(|(slot, hc)| {
            let mut params: HostControlParamMap = HostControlParamMap::new();
            for f in &hc.fields {
                params.insert(f.name.name.clone(), HostControlParamValue::from_value(&f.value));
            }
            HostControlDescriptor {
                slot,
                name: hc.name.name.clone(),
                kind: ManifestKind::from_ast(hc.kind),
                params,
                source: Provenance::root(hc.span),
            }
        })
        .collect()
}

fn synth_module(decls: &[&HostControlBlock]) -> ModuleDecl {
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
        name: Ident { name: SYNTH_HOST_CONTROL.to_owned(), span },
        type_name: Ident { name: TYPE_HOST_CONTROL.to_owned(), span },
        call_block,
        params,
        is_stereo: false,
        span,
    }
}

fn rewrite_connection(
    c: &Connection,
    lookup: &HashMap<String, HostControlKind>,
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
    lookup: &HashMap<String, HostControlKind>,
) -> CableEndpoint {
    match ep {
        CableEndpoint::Port(_) | CableEndpoint::Tap(_) => ep.clone(),
        CableEndpoint::HostControlRef(id) => {
            // Every bare ref has an explicit or synthesised implicit
            // declaration by this point, so the lookup is total.
            let kind = *lookup
                .get(&id.name)
                .expect_invariant("every bare host-control ref resolved by decl synthesis");
            CableEndpoint::Port(PortRef {
                module: SYNTH_HOST_CONTROL.to_owned(),
                port: PortLabel::Literal(out_port(kind).to_owned()),
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
