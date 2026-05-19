//! Stereo module sugar desugaring (ADR 0070, ticket 0844).
//!
//! Walks the patch body, splits each `stereo module X : T` into two mono
//! decls `X__l : T`, `X__r : T` with shared params plus per-channel
//! overrides, rewrites `X.<port>[l]` / `X.<port>[r]` selector references
//! to the corresponding `__l` / `__r` instance, and inserts
//! `StereoSplitter` / `StereoJoiner` instances at bus-form boundaries
//! per the connection rules in ADR 0070 §"Connection rules".
//!
//! ## No descriptor lookup
//!
//! The DSL crate has no descriptor registry, so this pass cannot ask
//! whether a non-stereo-module port (e.g. `Console.out`) carries a
//! mono or stereo cable. The trick: **always insert a `StereoSplitter`
//! at a stereo module's bus input, regardless of source kind.** The
//! existing `CableKind::Mono → CableKind::Stereo` broadcast rule in the
//! planner (`patches-planner::state::graph_index`) handles the mono
//! case — a mono signal feeding the splitter's stereo input gets
//! duplicated to L/R, which the splitter then routes to the two mono
//! instances. A stereo signal feeds the splitter directly. Both cases
//! produce identical-to-hand-written output without the desugar
//! needing to know which is which.
//!
//! Symmetrically: a stereo module's bus output is always wrapped in a
//! `StereoJoiner` when consumed in bus form. The joiner produces a
//! stereo cable that the consumer accepts (or fails on stereo→mono per
//! the existing connection validator).
//!
//! The one optimisation worth keeping is **pair-direct** for the
//! stereo-module → stereo-module case: when both endpoints are stereo
//! modules with bus form, the underlying `__l` / `__r` instances on
//! both sides already exist and we can pair them directly without
//! emitting a join+split sandwich. This is purely name-based — no
//! descriptor info needed.
//!
//! ## Templates
//!
//! Stereo decls inside `template { ... }` bodies are rejected. Templates
//! get inlined at expand time; supporting stereo there cleanly is left
//! as a follow-up.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ast::*;
use crate::expand::ExpandError;
use crate::structural::StructuralCode as Code;

const SUFFIX_L: &str = "__l";
const SUFFIX_R: &str = "__r";
const TYPE_SPLITTER: &str = "StereoSplitter";
const TYPE_JOINER: &str = "StereoJoiner";

/// Run stereo-module desugaring on `file`. Returns the rewritten file
/// (mutated in place when there are stereo decls) so a no-op pass costs
/// no clones beyond the public API's existing `&File → File` boundary.
pub fn desugar_stereo(mut file: File) -> Result<File, ExpandError> {
    // Reject stereo decls in templates (out of scope for 0844).
    for tmpl in &file.templates {
        for stmt in &tmpl.body {
            if let Statement::Module(m) = stmt {
                if m.is_stereo {
                    return Err(ExpandError::new(
                        Code::StereoInTemplate,
                        m.span,
                        "`stereo module` is not yet supported inside template bodies",
                    ));
                }
            }
        }
    }

    let has_stereo = file
        .patch
        .body
        .iter()
        .any(|s| matches!(s, Statement::Module(m) if m.is_stereo));
    if !has_stereo {
        return Ok(file);
    }

    // Index stereo decls + all module names for the clash check.
    let mut stereo_decls: HashMap<String, ModuleDecl> = HashMap::new();
    let mut all_module_names: HashMap<String, Span> = HashMap::new();
    for stmt in &file.patch.body {
        if let Statement::Module(m) = stmt {
            all_module_names.insert(m.name.name.clone(), m.name.span);
            if m.is_stereo {
                stereo_decls.insert(m.name.name.clone(), m.clone());
            }
        }
    }

    // Identifier-clash check: a user module named `X__l` / `X__r` would
    // collide with the synthesised name when `stereo module X` lands.
    for (name, decl) in &stereo_decls {
        for suffix in [SUFFIX_L, SUFFIX_R] {
            let synth = format!("{name}{suffix}");
            if let Some(other_span) = all_module_names.get(&synth) {
                if *other_span != decl.name.span {
                    return Err(ExpandError::new(
                        Code::StereoIdentClash,
                        *other_span,
                        format!(
                            "module name {synth:?} collides with the synthesised \
                             instance from `stereo module {name}`; rename one of \
                             the two to avoid the clash",
                        ),
                    ));
                }
            }
        }
    }

    // Heuristic single-channel constraint check: reject `stereo module
    // x : T(channels: N)` for N > 1 since the wrapped type is multi-
    // channel by construction.
    for decl in stereo_decls.values() {
        check_single_channel_constraint(decl)?;
    }

    let mut ctx = RewriteCtx::new(&stereo_decls);
    let new_body = ctx.transform_body(&file.patch.body)?;
    file.patch.body = new_body;
    Ok(file)
}

/// Reject `stereo module x : T(channels: N)` when `N > 1`. Catches the
/// obvious case of wrapping a multi-channel module (per ADR 0070
/// "Wrapped module type must be single-channel"). The descriptor-driven
/// check that catches default-multi-channel types is deferred to the
/// binding stage; this heuristic uses only what the AST carries.
fn check_single_channel_constraint(decl: &ModuleDecl) -> Result<(), ExpandError> {
    let Some(cb) = &decl.call_block else { return Ok(()); };
    for arg in &cb.args {
        if let CallArg::Named { name, value: ShapeValue::Scalar(Scalar::Int(n)), span } = arg {
            if name.name == "channels" && *n > 1 {
                return Err(ExpandError::new(
                    Code::StereoMultiChannelType,
                    *span,
                    format!(
                        "`stereo module {}` wraps `{}(channels: {})` — a stereo \
                         module must wrap a single-channel type. Drop the \
                         `channels` arg or remove the `stereo` keyword.",
                        decl.name.name, decl.type_name.name, n,
                    ),
                ));
            }
        }
    }
    Ok(())
}

// ─── Rewrite context ─────────────────────────────────────────────────────────

struct RewriteCtx<'a> {
    stereo_decls: &'a HashMap<String, ModuleDecl>,
    /// `(stereo_module_name, port) → joiner_instance_name`. Emitted once
    /// per (origin, port) pair; reused for every consumer that reads the
    /// stereo module's bus output.
    joiner_for: BTreeMap<(String, String), String>,
    /// `(src_module_name, src_port) → splitter_instance_name`. Emitted
    /// once per non-stereo-module source feeding any stereo-module bus
    /// input; the existing mono→stereo broadcast rule promotes a mono
    /// source feeding the splitter so we never need to know whether the
    /// source is mono or stereo at desugar time.
    splitter_for: BTreeMap<(String, String), String>,
    /// Decls + cables synthesised during cable rewriting; appended to
    /// the final body in registration order so all module decls precede
    /// connections that reference them.
    synth_modules: Vec<ModuleDecl>,
    synth_connections: Vec<Connection>,
    splitter_count: usize,
    joiner_count: usize,
}

impl<'a> RewriteCtx<'a> {
    fn new(stereo_decls: &'a HashMap<String, ModuleDecl>) -> Self {
        Self {
            stereo_decls,
            joiner_for: BTreeMap::new(),
            splitter_for: BTreeMap::new(),
            synth_modules: Vec::new(),
            synth_connections: Vec::new(),
            splitter_count: 0,
            joiner_count: 0,
        }
    }

    fn transform_body(&mut self, body: &[Statement]) -> Result<Vec<Statement>, ExpandError> {
        let mut out: Vec<Statement> = Vec::new();
        for stmt in body {
            match stmt {
                Statement::Module(m) if m.is_stereo => {
                    let (l, r) = split_stereo_decl(m);
                    out.push(Statement::Module(l));
                    out.push(Statement::Module(r));
                }
                Statement::Module(m) => out.push(Statement::Module(m.clone())),
                Statement::Connection(c) => {
                    self.transform_connection(c, &mut out)?;
                }
                other => out.push(other.clone()),
            }
        }
        // Append synthesised splitter / joiner modules + their feed cables
        // after user-authored statements.
        for m in self.synth_modules.drain(..) {
            out.push(Statement::Module(m));
        }
        for c in self.synth_connections.drain(..) {
            out.push(Statement::Connection(c));
        }
        Ok(out)
    }

    fn transform_connection(
        &mut self,
        c: &Connection,
        out: &mut Vec<Statement>,
    ) -> Result<(), ExpandError> {
        // Direction-normalise to (source, target).
        let (src, tgt) = match c.arrow.direction {
            Direction::Forward => (&c.lhs, &c.rhs),
            Direction::Backward => (&c.rhs, &c.lhs),
        };

        let src_kind = self.classify(src);
        let tgt_kind = self.classify(tgt);

        match (&src_kind, &tgt_kind) {
            // Pair-direct: both endpoints are stereo modules in bus form.
            // Wire underlying `__l` / `__r` instances directly without an
            // intermediate join+split. Purely a name-based optimisation.
            (
                EndpointKind::StereoModBus { name: s, port: sp },
                EndpointKind::StereoModBus { name: t, port: tp },
            ) => {
                let l = mono_endpoint(&suffix_name(s, SUFFIX_L), sp, src.span());
                let r = mono_endpoint(&suffix_name(s, SUFFIX_R), sp, src.span());
                let lt = mono_endpoint(&suffix_name(t, SUFFIX_L), tp, tgt.span());
                let rt = mono_endpoint(&suffix_name(t, SUFFIX_R), tp, tgt.span());
                out.push(Statement::Connection(directed(c, l, lt)));
                out.push(Statement::Connection(directed(c, r, rt)));
            }
            // Stereo-bus output → side selector on another stereo module.
            // ADR 0070 rejects this: pick a side from the source first.
            (
                EndpointKind::StereoModBus { name: s, .. },
                EndpointKind::StereoModSide { name: t, side, .. },
            ) => {
                let side_str = side_label(*side);
                return Err(ExpandError::new(
                    Code::StereoBusToSide,
                    src.span(),
                    format!(
                        "stereo source `{s}` cannot feed `{t}.<port>[{side_str}]`; \
                         pick a side from `{s}` first (`{s}.<port>[l]` or `{s}.<port>[r]`), \
                         or address `{t}.<port>` (bus form, both sides)",
                    ),
                ));
            }
            // Plain → stereo-bus on a stereo module: always insert a
            // `StereoSplitter` (CSE per source). The planner's mono→stereo
            // broadcast rule ensures a mono source produces L=R after the
            // splitter, identical to a direct mono-broadcast cable.
            //
            // Cable scale lands on the per-consumer
            // `~split.out_{l,r} → target` cables (not the shared feed),
            // so multiple consumers of the same source can each carry
            // their own scale while sharing the splitter via CSE.
            (EndpointKind::Plain, EndpointKind::StereoModBus { name: t, port: tp }) => {
                let p = src.as_port().expect("Plain endpoint is a port ref");
                let split = self.ensure_splitter(&p.module, &port_label_string(&p.port), src.span());
                let lt = mono_endpoint(&suffix_name(t, SUFFIX_L), tp, tgt.span());
                let rt = mono_endpoint(&suffix_name(t, SUFFIX_R), tp, tgt.span());
                let split_l = mono_endpoint(&split, "out_left", tgt.span());
                let split_r = mono_endpoint(&split, "out_right", tgt.span());
                out.push(Statement::Connection(directed(c, split_l, lt)));
                out.push(Statement::Connection(directed(c, split_r, rt)));
            }
            // Side selector on stereo module → stereo bus on (another)
            // stereo module: rewrite the selector to its mono instance,
            // then always insert splitter. The selector instance is mono,
            // so the splitter sees a mono source and broadcasts.
            (EndpointKind::StereoModSide { .. }, EndpointKind::StereoModBus { name: t, port: tp }) => {
                let src_resolved = self.rewrite_selector(src);
                let p = src_resolved.as_port().expect("rewritten selector is a port ref");
                let split = self.ensure_splitter(&p.module, &port_label_string(&p.port), src.span());
                let lt = mono_endpoint(&suffix_name(t, SUFFIX_L), tp, tgt.span());
                let rt = mono_endpoint(&suffix_name(t, SUFFIX_R), tp, tgt.span());
                let split_l = mono_endpoint(&split, "out_left", tgt.span());
                let split_r = mono_endpoint(&split, "out_right", tgt.span());
                out.push(Statement::Connection(directed(c, split_l, lt)));
                out.push(Statement::Connection(directed(c, split_r, rt)));
            }
            // Stereo-bus output → plain target: always insert a
            // `StereoJoiner` (CSE per origin port). If the target is mono,
            // the existing connection validator catches the stereo→mono
            // mismatch with the canonical message.
            (
                EndpointKind::StereoModBus { name: s, port: sp },
                EndpointKind::Plain,
            ) => {
                let join = self.ensure_joiner(s, sp, src.span());
                let join_out = mono_endpoint(&join, "out", src.span());
                let tgt_resolved = self.rewrite_selector(tgt);
                out.push(Statement::Connection(directed(c, join_out, tgt_resolved)));
            }
            // No stereo-module endpoint: cable passes through unchanged.
            // `[l]` / `[r]` indices on non-stereo modules remain ordinary
            // alias indices, untouched.
            (EndpointKind::Plain, EndpointKind::Plain) => {
                out.push(Statement::Connection(c.clone()));
            }
            // Both sides are selector forms on stereo modules: rewrite each
            // to its underlying mono instance and emit one cable.
            (EndpointKind::StereoModSide { .. }, EndpointKind::StereoModSide { .. }) => {
                let lhs = self.rewrite_selector(&c.lhs);
                let rhs = self.rewrite_selector(&c.rhs);
                out.push(Statement::Connection(Connection {
                    lhs,
                    arrow: c.arrow.clone(),
                    rhs,
                    span: c.span,
                }));
            }
            // Plain source → side selector on stereo module: direct edge
            // to the underlying mono instance.
            (EndpointKind::Plain, EndpointKind::StereoModSide { .. }) => {
                let rhs = self.rewrite_selector(tgt);
                out.push(Statement::Connection(directed(c, src.clone(), rhs)));
            }
            // Side selector → plain target: direct edge from the mono
            // instance.
            (EndpointKind::StereoModSide { .. }, EndpointKind::Plain) => {
                let lhs = self.rewrite_selector(src);
                out.push(Statement::Connection(directed(c, lhs, tgt.clone())));
            }
        }
        Ok(())
    }

    fn classify(&self, ep: &CableEndpoint) -> EndpointKind {
        let p = match ep {
            CableEndpoint::Port(p) => p,
            // Tap / host-control endpoints are non-stereo by construction.
            CableEndpoint::Tap(_) | CableEndpoint::HostControlRef(_) => return EndpointKind::Plain,
        };
        if self.stereo_decls.contains_key(&p.module) {
            // On a stereo module: `[l]` / `[r]` is a side selector, anything
            // else is bus form.
            return match &p.index {
                Some(PortIndex::Name { name, arity_marker: false }) if name == "l" => {
                    EndpointKind::StereoModSide {
                        name: p.module.clone(),
                        port: port_label_string(&p.port),
                        side: Side::L,
                    }
                }
                Some(PortIndex::Name { name, arity_marker: false }) if name == "r" => {
                    EndpointKind::StereoModSide {
                        name: p.module.clone(),
                        port: port_label_string(&p.port),
                        side: Side::R,
                    }
                }
                _ => EndpointKind::StereoModBus {
                    name: p.module.clone(),
                    port: port_label_string(&p.port),
                },
            };
        }
        EndpointKind::Plain
    }
}

// ─── Endpoint classification ─────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum EndpointKind {
    /// Any non-stereo-module endpoint: regular module port refs, taps,
    /// host-control refs. Cable kind (mono / stereo) is determined later
    /// by the planner via descriptor lookup; this pass treats them all
    /// uniformly because the planner's mono→stereo broadcast rule
    /// handles the kind asymmetry transparently.
    Plain,
    /// Bus form on a stereo module (no `[l]` / `[r]` selector).
    StereoModBus { name: String, port: String },
    /// Side selector on a stereo module. `port` is retained for error
    /// messages that name the offending endpoint precisely.
    StereoModSide {
        name: String,
        #[allow(dead_code)]
        port: String,
        side: Side,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    L,
    R,
}

fn side_label(s: Side) -> &'static str {
    match s {
        Side::L => "l",
        Side::R => "r",
    }
}

// ─── Decl rewrite ────────────────────────────────────────────────────────────

fn split_stereo_decl(decl: &ModuleDecl) -> (ModuleDecl, ModuleDecl) {
    let (l_params, r_params) = split_params(&decl.params);

    let span = decl.span;
    let l = ModuleDecl {
        name: Ident { name: format!("{}{SUFFIX_L}", decl.name.name), span: decl.name.span },
        type_name: decl.type_name.clone(),
        call_block: decl.call_block.clone(),
        params: l_params,
        is_stereo: false,
        span,
    };
    let r = ModuleDecl {
        name: Ident { name: format!("{}{SUFFIX_R}", decl.name.name), span: decl.name.span },
        type_name: decl.type_name.clone(),
        call_block: decl.call_block.clone(),
        params: r_params,
        is_stereo: false,
        span,
    };
    (l, r)
}

/// Partition a stereo decl's `param` block into `(left, right)` mono
/// param blocks. Top-level entries apply to both sides; `@l: { ... }` /
/// `@r: { ... }` at_blocks and `key[l]` / `key[r]` indexed entries
/// override the matching shared key on that side only (override wins).
fn split_params(params: &[ParamEntry]) -> (Vec<ParamEntry>, Vec<ParamEntry>) {
    let mut l_overrides: Vec<ParamEntry> = Vec::new();
    let mut r_overrides: Vec<ParamEntry> = Vec::new();
    let mut l_override_keys: HashSet<String> = HashSet::new();
    let mut r_override_keys: HashSet<String> = HashSet::new();
    let mut shared: Vec<ParamEntry> = Vec::new();

    for entry in params {
        match entry {
            ParamEntry::AtBlock { index: AtBlockIndex::Alias(alias), entries, span }
                if alias == "l" || alias == "r" =>
            {
                for (k, v) in entries {
                    let kv = ParamEntry::KeyValue {
                        name: k.clone(),
                        index: None,
                        value: v.clone(),
                        span: *span,
                    };
                    if alias == "l" {
                        l_override_keys.insert(k.name.clone());
                        l_overrides.push(kv);
                    } else {
                        r_override_keys.insert(k.name.clone());
                        r_overrides.push(kv);
                    }
                }
            }
            ParamEntry::KeyValue {
                name,
                index: Some(ParamIndex::Name { name: idx, arity_marker: false }),
                value,
                span,
            } if idx == "l" || idx == "r" => {
                let kv = ParamEntry::KeyValue {
                    name: name.clone(),
                    index: None,
                    value: value.clone(),
                    span: *span,
                };
                if idx == "l" {
                    l_override_keys.insert(name.name.clone());
                    l_overrides.push(kv);
                } else {
                    r_override_keys.insert(name.name.clone());
                    r_overrides.push(kv);
                }
            }
            other => shared.push(other.clone()),
        }
    }

    let mut l = shared
        .iter()
        .filter(|e| match e {
            ParamEntry::KeyValue { name, .. } => !l_override_keys.contains(&name.name),
            _ => true,
        })
        .cloned()
        .collect::<Vec<_>>();
    l.extend(l_overrides);

    let mut r = shared
        .into_iter()
        .filter(|e| match e {
            ParamEntry::KeyValue { name, .. } => !r_override_keys.contains(&name.name),
            _ => true,
        })
        .collect::<Vec<_>>();
    r.extend(r_overrides);

    (l, r)
}

// ─── Splitter / joiner emission ──────────────────────────────────────────────

impl<'a> RewriteCtx<'a> {
    fn ensure_splitter(&mut self, src: &str, port: &str, span: Span) -> String {
        let key = (src.to_owned(), port.to_owned());
        if let Some(name) = self.splitter_for.get(&key) {
            return name.clone();
        }
        let name = format!("~split_{}", self.splitter_count);
        self.splitter_count += 1;
        self.synth_modules.push(synth_module(&name, TYPE_SPLITTER, span));
        // Feed cable: src.port → name.in
        self.synth_connections.push(synth_cable(
            mono_endpoint(src, port, span),
            mono_endpoint(&name, "in", span),
            span,
        ));
        self.splitter_for.insert(key, name.clone());
        name
    }

    fn ensure_joiner(&mut self, stereo: &str, port: &str, span: Span) -> String {
        let key = (stereo.to_owned(), port.to_owned());
        if let Some(name) = self.joiner_for.get(&key) {
            return name.clone();
        }
        let name = format!("~join_{}", self.joiner_count);
        self.joiner_count += 1;
        self.synth_modules.push(synth_module(&name, TYPE_JOINER, span));
        self.synth_connections.push(synth_cable(
            mono_endpoint(&suffix_name(stereo, SUFFIX_L), port, span),
            mono_endpoint(&name, "in_left", span),
            span,
        ));
        self.synth_connections.push(synth_cable(
            mono_endpoint(&suffix_name(stereo, SUFFIX_R), port, span),
            mono_endpoint(&name, "in_right", span),
            span,
        ));
        self.joiner_for.insert(key, name.clone());
        name
    }

    /// Rewrite a side-selector port_ref (`name.port[l|r]` on a stereo
    /// module) to the underlying mono instance form. Pass-through for
    /// any other endpoint shape.
    fn rewrite_selector(&self, ep: &CableEndpoint) -> CableEndpoint {
        let p = match ep {
            CableEndpoint::Port(p) => p,
            other => return other.clone(),
        };
        let Some(_decl) = self.stereo_decls.get(&p.module) else {
            return ep.clone();
        };
        let Some(PortIndex::Name { name: idx, arity_marker: false }) = &p.index else {
            return ep.clone();
        };
        let suffix = match idx.as_str() {
            "l" => SUFFIX_L,
            "r" => SUFFIX_R,
            _ => return ep.clone(),
        };
        CableEndpoint::Port(PortRef {
            module: format!("{}{suffix}", p.module),
            port: p.port.clone(),
            index: None,
            span: p.span,
        })
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn suffix_name(stereo: &str, suffix: &str) -> String {
    format!("{stereo}{suffix}")
}

fn port_label_string(label: &PortLabel) -> String {
    match label {
        PortLabel::Literal(s) => s.clone(),
        // `<param>` references in port labels can't be statically named
        // here. Stereo desugar runs before template expansion so this
        // shouldn't trigger; if it does, fall back to a placeholder so
        // classification can still proceed (binding will reject).
        PortLabel::Param(s) => format!("<{s}>"),
    }
}

fn mono_endpoint(module: &str, port: &str, span: Span) -> CableEndpoint {
    CableEndpoint::Port(PortRef {
        module: module.to_owned(),
        port: PortLabel::Literal(port.to_owned()),
        index: None,
        span,
    })
}

fn directed(template: &Connection, lhs_or_src: CableEndpoint, rhs_or_tgt: CableEndpoint) -> Connection {
    // `transform_connection` direction-normalises to (src, tgt). To
    // preserve the user's authored arrow direction on each emitted
    // cable, swap LHS / RHS for backward arrows.
    let (lhs, rhs) = match template.arrow.direction {
        Direction::Forward => (lhs_or_src, rhs_or_tgt),
        Direction::Backward => (rhs_or_tgt, lhs_or_src),
    };
    Connection { lhs, arrow: template.arrow.clone(), rhs, span: template.span }
}

fn synth_cable(lhs: CableEndpoint, rhs: CableEndpoint, span: Span) -> Connection {
    Connection {
        lhs,
        arrow: Arrow {
            direction: Direction::Forward,
            scale: None,
            span,
        },
        rhs,
        span,
    }
}

fn synth_module(name: &str, type_name: &str, span: Span) -> ModuleDecl {
    ModuleDecl {
        name: Ident { name: name.to_owned(), span },
        type_name: Ident { name: type_name.to_owned(), span },
        call_block: None,
        params: Vec::new(),
        is_stereo: false,
        span,
    }
}
