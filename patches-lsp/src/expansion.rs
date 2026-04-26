//! Deep-analysis caches: pest-parsed [`File`]s per document and flattened
//! [`FlatPatch`]es per root, with a [`PatchReferences`] index over the
//! flattened structure for feature lookups.
//!
//! Separate from the tolerant tree-sitter pipeline. Pest parse runs only when
//! tree-sitter reports a clean tree; expansion runs lazily when a feature
//! handler calls [`super::workspace::DocumentWorkspace::ensure_flat`].

use std::collections::HashMap;

use patches_core::{QName, SourceId, Span as CoreSpan};
use patches_dsl::ast::{
    self as dsl_ast, PortLabel as DslPortLabel, Statement as DslStatement,
};
use patches_dsl::flat::FlatPatch;
use patches_dsl::File as PestFile;

/// Discriminated pointer into a [`FlatPatch`]'s node arrays. The variant
/// selects which array; the `usize` is the index within that array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlatNodeRef {
    Module(usize),
    Connection(usize),
    PortRef(usize),
    Pattern(usize),
    Song(usize),
}

/// Reverse index from authored spans to flat nodes. Built once per root
/// [`FlatPatch`] and invalidated together with it.
#[derive(Debug, Default)]
pub(crate) struct SpanIndex {
    /// `(provenance.site, node ref)`. Unsorted; lookups scan linearly and
    /// pick the smallest enclosing span. Flat patches in the LSP are small
    /// enough that this is cheaper than maintaining an interval tree.
    pub entries: Vec<(CoreSpan, FlatNodeRef)>,
}

impl SpanIndex {
    pub fn build(flat: &FlatPatch) -> Self {
        let mut entries = Vec::with_capacity(
            flat.modules.len()
                + flat.connections.len()
                + flat.port_refs.len()
                + flat.song_data.patterns.len()
                + flat.song_data.songs.len(),
        );
        for (i, m) in flat.modules.iter().enumerate() {
            entries.push((m.provenance.site, FlatNodeRef::Module(i)));
        }
        for (i, c) in flat.connections.iter().enumerate() {
            entries.push((c.provenance.site, FlatNodeRef::Connection(i)));
        }
        for (i, p) in flat.port_refs.iter().enumerate() {
            entries.push((p.provenance.site, FlatNodeRef::PortRef(i)));
        }
        for (i, p) in flat.song_data.patterns.iter().enumerate() {
            entries.push((p.provenance.site, FlatNodeRef::Pattern(i)));
        }
        for (i, s) in flat.song_data.songs.iter().enumerate() {
            entries.push((s.provenance.site, FlatNodeRef::Song(i)));
        }
        Self { entries }
    }

    /// Smallest authored span at `(source, offset)`. Synthetic spans are
    /// skipped — they have no author-visible location.
    pub fn find_at(&self, source: SourceId, offset: usize) -> Option<FlatNodeRef> {
        self.entries
            .iter()
            .filter(|(s, _)| {
                s.source == source
                    && s.source != SourceId::SYNTHETIC
                    && s.start <= offset
                    && offset < s.end
            })
            .min_by_key(|(s, _)| s.end.saturating_sub(s.start))
            .map(|(_, r)| *r)
    }
}

/// Reference to a template instantiated by a particular `module` declaration.
#[derive(Debug, Clone)]
pub(crate) struct TemplateRef {
    /// Template name (looks up into [`PatchReferences::wires_by_template`]).
    pub name: String,
    /// Span of the template definition itself (for goto/peek consumers).
    #[allow(dead_code)]
    pub def_span: CoreSpan,
}

/// Wiring information for one declared template port.
#[derive(Debug, Default, Clone)]
pub(crate) struct WiredPort {
    /// Declared port name on the template (`$.<port>`).
    pub port: String,
    /// Internal port references this template port is wired to. For input
    /// ports these are the targets the input drives (`$.in -> X.y`); for
    /// output ports these are the sources driving the output (`X.y -> $.out`).
    /// Backward arrows are normalised so the entries always read as listed.
    pub wires: Vec<dsl_ast::PortRef>,
}

/// Per-template precomputed wiring tables, one entry per declared in/out port.
#[derive(Debug, Default, Clone)]
pub(crate) struct TemplateWires {
    pub ins: Vec<WiredPort>,
    pub outs: Vec<WiredPort>,
}

impl TemplateWires {
    pub fn from_template(t: &dsl_ast::Template) -> Self {
        let ins = t
            .in_ports
            .iter()
            .map(|p| WiredPort {
                port: p.name.name.clone(),
                wires: collect_port_wires(&t.body, &p.name.name, /* input= */ true),
            })
            .collect();
        let outs = t
            .out_ports
            .iter()
            .map(|p| WiredPort {
                port: p.name.name.clone(),
                wires: collect_port_wires(&t.body, &p.name.name, /* input= */ false),
            })
            .collect();
        Self { ins, outs }
    }
}

/// Walk `body` for every connection touching `$.<port>` and return the
/// internal port references on the opposite side. `input=true` looks for
/// `$.port -> X.y`; `input=false` looks for `X.y -> $.port`. Backward arrows
/// are handled by swapping sides during inspection so the returned list is
/// always the "internal" side.
fn collect_port_wires(
    body: &[DslStatement],
    port: &str,
    input: bool,
) -> Vec<dsl_ast::PortRef> {
    let mut out = Vec::new();
    for stmt in body {
        let DslStatement::Connection(c) = stmt else {
            continue;
        };
        let (dollar_side, other_side) = match c.arrow.direction {
            dsl_ast::Direction::Forward => (&c.lhs, &c.rhs),
            dsl_ast::Direction::Backward => (&c.rhs, &c.lhs),
        };
        // Tap endpoints (ADR 0054) are not template-port wires; skip them.
        let (Some(dollar_pr), Some(other_pr)) = (dollar_side.as_port(), other_side.as_port())
        else {
            continue;
        };
        if input && is_dollar_port(dollar_pr, port) {
            out.push(other_pr.clone());
        } else if !input && is_dollar_port(other_pr, port) {
            out.push(dollar_pr.clone());
        }
    }
    out
}

fn is_dollar_port(pr: &dsl_ast::PortRef, port: &str) -> bool {
    pr.module == "$"
        && match &pr.port {
            DslPortLabel::Literal(name) => name == port,
            _ => false,
        }
}

/// Unified reverse-and-grouped index over a [`FlatPatch`] plus its merged
/// pest [`File`]. Built once in `ensure_flat_locked` and cached lockstep
/// with [`FlatPatch`]. See ADR 0037.
#[derive(Debug, Default)]
pub(crate) struct PatchReferences {
    pub span_index: SpanIndex,
    /// Every span appearing in any node's `Provenance.expansion` chain →
    /// flat nodes emitted under it. Used by call-site hover, inlay hints,
    /// peek expansion.
    pub call_sites: HashMap<CoreSpan, Vec<FlatNodeRef>>,
    /// Authored connection span → indices of every [`FlatConnection`] sharing
    /// that span. Collapses top-level fan-out and arity expansion.
    pub connection_groups: HashMap<CoreSpan, Vec<usize>>,
    /// Instance [`QName`] → module index. Avoids linear scans for port_ref
    /// → owning module hops.
    #[allow(dead_code)]
    pub module_by_qname: HashMap<QName, usize>,
    /// Call-site span (the span of an instantiating `ModuleDecl`) →
    /// template name + defining span.
    pub template_by_call_site: HashMap<CoreSpan, TemplateRef>,
    /// Template name → per-port wiring tables.
    pub wires_by_template: HashMap<String, TemplateWires>,
}

impl PatchReferences {
    pub fn build(flat: &FlatPatch, merged: &PestFile) -> Self {
        let span_index = SpanIndex::build(flat);
        let mut call_sites: HashMap<CoreSpan, Vec<FlatNodeRef>> = HashMap::new();
        let mut connection_groups: HashMap<CoreSpan, Vec<usize>> = HashMap::new();
        let mut module_by_qname: HashMap<QName, usize> = HashMap::new();

        for (i, m) in flat.modules.iter().enumerate() {
            module_by_qname.insert(m.id.clone(), i);
            for s in &m.provenance.expansion {
                call_sites.entry(*s).or_default().push(FlatNodeRef::Module(i));
            }
        }
        for (i, c) in flat.connections.iter().enumerate() {
            connection_groups
                .entry(c.provenance.site)
                .or_default()
                .push(i);
            for s in &c.provenance.expansion {
                call_sites
                    .entry(*s)
                    .or_default()
                    .push(FlatNodeRef::Connection(i));
            }
        }
        for (i, p) in flat.port_refs.iter().enumerate() {
            for s in &p.provenance.expansion {
                call_sites.entry(*s).or_default().push(FlatNodeRef::PortRef(i));
            }
        }
        for (i, p) in flat.song_data.patterns.iter().enumerate() {
            for s in &p.provenance.expansion {
                call_sites.entry(*s).or_default().push(FlatNodeRef::Pattern(i));
            }
        }
        for (i, s) in flat.song_data.songs.iter().enumerate() {
            for sp in &s.provenance.expansion {
                call_sites.entry(*sp).or_default().push(FlatNodeRef::Song(i));
            }
        }

        let template_spans: HashMap<&str, CoreSpan> = merged
            .templates
            .iter()
            .map(|t| (t.name.name.as_str(), t.span))
            .collect();
        let mut template_by_call_site: HashMap<CoreSpan, TemplateRef> = HashMap::new();
        index_template_calls(&merged.patch.body, &template_spans, &mut template_by_call_site);
        for t in &merged.templates {
            index_template_calls(&t.body, &template_spans, &mut template_by_call_site);
        }

        let wires_by_template = merged
            .templates
            .iter()
            .map(|t| (t.name.name.clone(), TemplateWires::from_template(t)))
            .collect();

        Self {
            span_index,
            call_sites,
            connection_groups,
            module_by_qname,
            template_by_call_site,
            wires_by_template,
        }
    }
}

fn index_template_calls(
    body: &[DslStatement],
    templates: &HashMap<&str, CoreSpan>,
    out: &mut HashMap<CoreSpan, TemplateRef>,
) {
    for stmt in body {
        if let DslStatement::Module(m) = stmt {
            if let Some(def_span) = templates.get(m.type_name.name.as_str()) {
                out.insert(
                    m.span,
                    TemplateRef {
                        name: m.type_name.name.clone(),
                        def_span: *def_span,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use patches_dsl::{expand, parse_with_source};

    fn build_refs(src: &str) -> (FlatPatch, PatchReferences) {
        let file = parse_with_source(src, SourceId(1)).expect("parse");
        let flat = expand(&file).expect("expand").patch;
        let refs = PatchReferences::build(&flat, &file);
        (flat, refs)
    }

    #[test]
    fn call_sites_group_per_template_call() {
        let src = "\
template voice() {
    in: gate
    out: audio
    module osc : Osc
    $.gate -> osc.sine
    osc.sine -> $.audio
}
patch {
    module v1 : voice
    module v2 : voice
}
";
        let (flat, refs) = build_refs(src);
        // Two distinct call-site entries, each carrying the single emitted
        // module from its own expansion.
        let mut by_text: HashMap<String, Vec<usize>> = HashMap::new();
        for (span, refs) in &refs.call_sites {
            let text = &src[span.start..span.end];
            let mods: Vec<usize> = refs
                .iter()
                .filter_map(|r| match r {
                    FlatNodeRef::Module(i) => Some(*i),
                    _ => None,
                })
                .collect();
            by_text.entry(text.to_string()).or_default().extend(mods);
        }
        let v1 = by_text
            .iter()
            .find(|(k, _)| k.contains("v1"))
            .map(|(_, v)| v)
            .unwrap_or_else(|| panic!("v1 call site; got: {:?}", by_text.keys().collect::<Vec<_>>()));
        let v2 = by_text
            .iter()
            .find(|(k, _)| k.contains("v2"))
            .map(|(_, v)| v)
            .unwrap_or_else(|| panic!("v2 call site; got: {:?}", by_text.keys().collect::<Vec<_>>()));
        assert_eq!(v1.len(), 1);
        assert_eq!(v2.len(), 1);
        assert!(v1.iter().all(|i| !v2.contains(i)));
        assert_eq!(flat.modules[v1[0]].id.to_string(), "v1/osc");
        assert_eq!(flat.modules[v2[0]].id.to_string(), "v2/osc");
    }

    #[test]
    fn connection_groups_collapse_fanout() {
        let src = "\
patch {
    module a : Osc
    module b : Osc
    module c : Osc
    a.sine -> b.fm, c.fm
}
";
        let (flat, refs) = build_refs(src);
        // The fan-out desugars to two connection entries sharing one span.
        let span = flat.connections[0].provenance.site;
        let group = refs.connection_groups.get(&span).expect("group");
        assert_eq!(group.len(), 2);
    }

    #[test]
    fn template_wires_lists_targets() {
        let src = "\
template voice() {
    in: gate, voct
    out: audio
    module osc : Osc
    $.gate -> osc.sine
    osc.sine <- $.voct
    osc.sine -> $.audio
}
patch { module v : voice }
";
        let (_flat, refs) = build_refs(src);
        let w = refs.wires_by_template.get("voice").expect("wires");
        let gate = w.ins.iter().find(|p| p.port == "gate").expect("gate");
        assert_eq!(gate.wires.len(), 1);
        assert_eq!(gate.wires[0].module, "osc");
        let voct = w.ins.iter().find(|p| p.port == "voct").expect("voct");
        // Backward arrow `osc.sine <- $.voct` is normalised: `$.voct` is on
        // the rhs, so the wire target is the lhs (`osc.sine`).
        assert_eq!(voct.wires.len(), 1);
        assert_eq!(voct.wires[0].module, "osc");
        let audio = w.outs.iter().find(|p| p.port == "audio").expect("audio");
        assert_eq!(audio.wires.len(), 1);
        assert_eq!(audio.wires[0].module, "osc");
    }

    #[test]
    fn module_by_qname_round_trips_nested_instances() {
        let src = "\
template voice() {
    in: gate
    out: audio
    module osc : Osc
    osc.sine -> $.audio
}
patch {
    module v : voice
    module out : AudioOut
    v.audio -> out.in_left
    v.audio -> out.in_right
}
";
        let (flat, refs) = build_refs(src);
        let v_osc = QName { path: vec!["v".to_string()], name: "osc".to_string() };
        let i = refs.module_by_qname.get(&v_osc).copied().expect("v/osc");
        assert_eq!(flat.modules[i].id, v_osc);
        let out = QName::bare("out");
        let j = refs.module_by_qname.get(&out).copied().expect("out");
        assert_eq!(flat.modules[j].id, out);
    }
}
