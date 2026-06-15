//! Pest-tree walkers for expression-level nodes: identifiers, scalars,
//! values, shape args, param entries, port refs, arrows, connections,
//! and the statement dispatcher.

use patches_core::ExpectInvariant;
use pest::iterators::Pair;

use crate::ast::{
    Arrow, AtBlockIndex, CableEndpoint, CallArg, CallBlock, Connection, Direction,
    HostControlBlock, HostControlField, HostControlKind, Ident, ParamEntry, ParamIndex,
    PortIndex, PortLabel, PortRef, RangeEndpoint, RangeKind, ScaleSpec, Scalar, ShapeValue,
    Statement, TapTarget, UnitFamily, Value,
};

use super::decls::build_module_decl;
use super::error::ParseError;
use super::literals::{parse_note_voct, parse_unit_value, unit_family_of};
use super::steps_songs::{build_pattern_block, build_song_block};
use super::{span_of, Rule};

pub(super) fn build_ident(pair: Pair<'_, Rule>) -> Ident {
    let span = span_of(&pair);
    Ident {
        name: pair.as_str().to_owned(),
        span,
    }
}

/// Extract the name string from a `param_ref` pair (`${ "<" ~ param_ref_ident ~ ">" }`).
pub(super) fn build_param_ref_name(pair: Pair<'_, Rule>) -> String {
    // pair.as_rule() == Rule::param_ref; the single inner child is param_ref_ident
    pair.into_inner()
        .next()
        .expect_invariant("grammar guarantees param_ref has a param_ref_ident child")
        .as_str()
        .to_owned()
}

pub(super) fn build_scalar(pair: Pair<'_, Rule>) -> Result<Scalar, ParseError> {
    // pair.as_rule() == Rule::scalar
    let inner = pair
        .into_inner()
        .next()
        .expect_invariant("grammar guarantees scalar has one child");
    let span = span_of(&inner);
    match inner.as_rule() {
        Rule::float_unit => {
            let value = parse_unit_value(inner.as_str(), span)?;
            Ok(Scalar::Float(value))
        }
        Rule::float_lit => inner
            .as_str()
            .parse::<f64>()
            .map(Scalar::Float)
            .map_err(|_| ParseError {
                span,
                message: format!("invalid float literal: {:?}", inner.as_str()),
            }),
        Rule::int_lit => inner
            .as_str()
            .parse::<i64>()
            .map(Scalar::Int)
            .map_err(|_| ParseError {
                span,
                message: format!("invalid integer literal: {:?}", inner.as_str()),
            }),
        Rule::bool_lit => Ok(Scalar::Bool(inner.as_str() == "true")),
        Rule::note_lit => {
            // Atomic rule: e.g. "C1", "Bb2", "A#-1", "f#3".
            parse_note_voct(inner.as_str(), span).map(Scalar::Float)
        }
        Rule::string_lit => {
            let s = inner.as_str();
            Ok(Scalar::Str(s[1..s.len() - 1].to_owned())) // strip surrounding quotes
        }
        Rule::ident => Ok(Scalar::Str(inner.as_str().to_owned())),
        Rule::param_ref => Ok(Scalar::ParamRef(build_param_ref_name(inner))),
        _ => unreachable!("unexpected rule in scalar: {:?}", inner.as_rule()),
    }
}

pub(super) fn build_value(pair: Pair<'_, Rule>) -> Result<Value, ParseError> {
    // pair.as_rule() == Rule::value
    let inner = pair
        .into_inner()
        .next()
        .expect_invariant("grammar guarantees value has one child");
    match inner.as_rule() {
        Rule::file_ref => {
            let child = inner
                .into_inner()
                .next()
                .expect_invariant("grammar guarantees file_ref has one child");
            let path = match child.as_rule() {
                Rule::string_lit => {
                    let s = child.as_str();
                    s[1..s.len() - 1].to_owned()
                }
                Rule::param_ref => {
                    // Template parameter substitution happens at expand time;
                    // store the raw param ref name for now.
                    return Ok(Value::Scalar(Scalar::ParamRef(build_param_ref_name(child))));
                }
                _ => unreachable!("unexpected rule in file_ref: {:?}", child.as_rule()),
            };
            Ok(Value::File(path))
        }
        Rule::scalar => Ok(Value::Scalar(build_scalar(inner)?)),
        _ => unreachable!("unexpected rule in value: {:?}", inner.as_rule()),
    }
}

fn build_shape_value(pair: Pair<'_, Rule>) -> Result<ShapeValue, ParseError> {
    match pair.as_rule() {
        Rule::alias_list => {
            // alias_list = { "[" ~ (ident ~ ","?)* ~ "]" }
            let members = pair.into_inner().map(build_ident).collect();
            Ok(ShapeValue::AliasList(members))
        }
        Rule::scalar => Ok(ShapeValue::Scalar(build_scalar(pair)?)),
        other => unreachable!("unexpected rule in shape value: {:?}", other),
    }
}

pub(super) fn build_call_arg(pair: Pair<'_, Rule>) -> Result<CallArg, ParseError> {
    // pair.as_rule() == Rule::call_arg
    // Grammar: call_arg = { named_arg | shorthand_arg | bare_arg }
    let span = span_of(&pair);
    let inner = pair
        .into_inner()
        .next()
        .expect_invariant("grammar guarantees call_arg has one child");
    match inner.as_rule() {
        Rule::named_arg => {
            // named_arg = { ident ~ ":" ~ (alias_list | scalar) }
            let mut it = inner.into_inner();
            let name = build_ident(
                it.next()
                    .expect_invariant("grammar guarantees named_arg has an ident child"),
            );
            let value = build_shape_value(
                it.next()
                    .expect_invariant("grammar guarantees named_arg has a value child"),
            )?;
            Ok(CallArg::Named { name, value, span })
        }
        Rule::shorthand_arg => {
            // shorthand_arg = { param_ref }
            let pr = inner
                .into_inner()
                .next()
                .expect_invariant("grammar guarantees shorthand_arg has a param_ref child");
            let name = build_param_ref_name(pr);
            Ok(CallArg::Shorthand { name, span })
        }
        Rule::bare_arg => {
            // bare_arg = { alias_list | scalar }
            let v = inner
                .into_inner()
                .next()
                .expect_invariant("grammar guarantees bare_arg has one child");
            let value = build_shape_value(v)?;
            Ok(CallArg::Bare { value, span })
        }
        other => unreachable!("unexpected rule in call_arg: {:?}", other),
    }
}

pub(super) fn build_call_block(pair: Pair<'_, Rule>) -> Result<CallBlock, ParseError> {
    // pair.as_rule() == Rule::call_block
    let span = span_of(&pair);
    let args = pair.into_inner().map(build_call_arg).collect::<Result<_, _>>()?;
    Ok(CallBlock { args, span })
}

pub(super) fn build_at_block(pair: Pair<'_, Rule>) -> Result<ParamEntry, ParseError> {
    // pair.as_rule() == Rule::at_block
    // Grammar: at_block = { "@" ~ at_block_index ~ ":"? ~ at_block_body }
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    let index_pair = it
        .next()
        .expect_invariant("grammar guarantees at_block has an at_block_index child");
    let index_inner = index_pair
        .into_inner()
        .next()
        .expect_invariant("grammar guarantees at_block_index has one child");
    let index = match index_inner.as_rule() {
        Rule::nat => {
            let nat_span = span_of(&index_inner);
            let n = index_inner.as_str().parse::<u32>().map_err(|_| ParseError {
                span: nat_span,
                message: format!("invalid at-block index: {:?}", index_inner.as_str()),
            })?;
            AtBlockIndex::Literal(n)
        }
        Rule::ident => AtBlockIndex::Alias(index_inner.as_str().to_owned()),
        _ => unreachable!("unexpected rule in at_block_index: {:?}", index_inner.as_rule()),
    };

    let body_pair = it
        .next()
        .expect_invariant("grammar guarantees at_block has an at_block_body child");
    let entries: Result<Vec<(Ident, Value)>, ParseError> = body_pair
        .into_inner()
        .map(|entry| {
            let mut entry_it = entry.into_inner();
            let key = build_ident(
                entry_it
                    .next()
                    .expect_invariant("grammar guarantees at_block entry has a key ident"),
            );
            let val = build_value(
                entry_it
                    .next()
                    .expect_invariant("grammar guarantees at_block entry has a value"),
            )?;
            Ok((key, val))
        })
        .collect();

    Ok(ParamEntry::AtBlock { index, entries: entries?, span })
}

pub(super) fn build_param_entry(pair: Pair<'_, Rule>) -> Result<ParamEntry, ParseError> {
    // pair.as_rule() == Rule::param_entry
    // Grammar: param_entry = { at_block | ident ~ param_index? ~ ":" ~ value }
    //          param_index  = { "[" ~ (param_index_arity | nat | ident) ~ "]" }
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    // Check first child: either at_block or ident
    let first = it
        .next()
        .expect_invariant("grammar guarantees param_entry has a first child");
    if first.as_rule() == Rule::at_block {
        return build_at_block(first);
    }

    let name = build_ident(first);

    // Next pair is either param_index (optional) or value.
    let next = it
        .next()
        .expect_invariant("grammar guarantees param_entry has a param_index-or-value child");
    let (index, value) = match next.as_rule() {
        Rule::param_index => {
            // Inner child is param_index_arity, nat, or ident.
            let inner = next
                .into_inner()
                .next()
                .expect_invariant("grammar guarantees param_index has one child");
            let idx = match inner.as_rule() {
                Rule::param_index_arity => {
                    // param_index_arity = ${ "*" ~ ident }; inner child is ident
                    let name = inner
                        .into_inner()
                        .next()
                        .expect_invariant("grammar guarantees param_index_arity has an ident child")
                        .as_str()
                        .to_owned();
                    ParamIndex::Name { name, arity_marker: true }
                }
                Rule::nat => {
                    let nat_span = span_of(&inner);
                    let n = inner.as_str().parse::<u32>().map_err(|_| ParseError {
                        span: nat_span,
                        message: format!("invalid param index: {:?}", inner.as_str()),
                    })?;
                    ParamIndex::Literal(n)
                }
                Rule::ident => ParamIndex::Name { name: inner.as_str().to_owned(), arity_marker: false },
                _ => unreachable!("unexpected rule in param_index: {:?}", inner.as_rule()),
            };
            let val = build_value(
                it.next()
                    .expect_invariant("grammar guarantees param_entry has a value after param_index"),
            )?;
            (Some(idx), val)
        }
        Rule::value => (None, build_value(next)?),
        _ => unreachable!("unexpected rule in param_entry: {:?}", next.as_rule()),
    };

    Ok(ParamEntry::KeyValue { name, index, value, span })
}

pub(super) fn build_port_ref(pair: Pair<'_, Rule>) -> Result<PortRef, ParseError> {
    // pair.as_rule() == Rule::port_ref  (compound-atomic)
    // Grammar: port_ref = ${ module_ident ~ "." ~ port_label ~ port_index? }
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    // module_ident: either "$" or ident
    let module_ident_pair = it
        .next()
        .expect_invariant("grammar guarantees port_ref has a module_ident child");
    let module = module_ident_pair.as_str().to_owned();

    // port label: port_label = ${ param_ref | ident }
    let port_label_pair = it
        .next()
        .expect_invariant("grammar guarantees port_ref has a port_label child");
    // port_label is compound-atomic; its single inner child is param_ref or ident
    let port_inner = port_label_pair
        .into_inner()
        .next()
        .expect_invariant("grammar guarantees port_label has one child");
    let port = match port_inner.as_rule() {
        Rule::ident => PortLabel::Literal(port_inner.as_str().to_owned()),
        Rule::param_ref => PortLabel::Param(build_param_ref_name(port_inner)),
        _ => unreachable!("unexpected rule in port_label: {:?}", port_inner.as_rule()),
    };

    // optional port_index
    let index = it
        .next()
        .map(|idx_pair| {
            // port_index = ${ "[" ~ (port_index_arity | nat | ident) ~ "]" }
            // inner child is one of: port_index_arity, nat, ident
            let inner = idx_pair
                .into_inner()
                .next()
                .expect_invariant("grammar guarantees port_index has one child");
            let inner_span = span_of(&inner);
            match inner.as_rule() {
                Rule::port_index_arity => {
                    // port_index_arity = ${ "*" ~ ident }
                    // inner of port_index_arity is the ident
                    let name = inner
                        .into_inner()
                        .next()
                        .expect_invariant("grammar guarantees port_index_arity has an ident child")
                        .as_str()
                        .to_owned();
                    Ok(PortIndex::Name { name, arity_marker: true })
                }
                Rule::nat => {
                    inner.as_str().parse::<u32>().map(PortIndex::Literal).map_err(|_| {
                        ParseError {
                            span: inner_span,
                            message: format!("invalid port index: {:?}", inner.as_str()),
                        }
                    })
                }
                Rule::ident => Ok(PortIndex::Name { name: inner.as_str().to_owned(), arity_marker: false }),
                Rule::param_ref => Ok(PortIndex::Name { name: build_param_ref_name(inner), arity_marker: false }),
                _ => unreachable!("unexpected rule in port_index: {:?}", inner.as_rule()),
            }
        })
        .transpose()?;

    Ok(PortRef { module, port, index, span })
}

/// Parse a `scale_endpoint` pair into a `(Scalar, UnitFamily)`.
///
/// `scale_endpoint = ${ param_ref | float_unit | note_lit | scale_num }`.
/// Family attribution is parse-time only: notes and Hz/kHz are `Pitch`,
/// `dB` is `Level`, `s`/`c` are `Time`, plain numbers and `<param>`
/// refs are `Plain` (param-resolved scalars are family-erased — see
/// `eval_scale`).
fn build_scale_endpoint(pair: Pair<'_, Rule>) -> Result<(Scalar, UnitFamily), ParseError> {
    // pair.as_rule() == Rule::scale_endpoint
    let inner = pair
        .into_inner()
        .next()
        .expect_invariant("grammar guarantees scale_endpoint has one child");
    let s_span = span_of(&inner);
    match inner.as_rule() {
        Rule::scale_num => inner
            .as_str()
            .parse::<f64>()
            .map(|f| (Scalar::Float(f), UnitFamily::Plain))
            .map_err(|_| ParseError {
                span: s_span,
                message: format!("invalid scale factor: {:?}", inner.as_str()),
            }),
        Rule::float_unit => {
            let family = unit_family_of(inner.as_str());
            parse_unit_value(inner.as_str(), s_span).map(|f| (Scalar::Float(f), family))
        }
        Rule::note_lit => {
            parse_note_voct(inner.as_str(), s_span).map(|f| (Scalar::Float(f), UnitFamily::Pitch))
        }
        Rule::param_ref => Ok((
            Scalar::ParamRef(build_param_ref_name(inner)),
            UnitFamily::Plain,
        )),
        _ => unreachable!("unexpected rule in scale_endpoint: {:?}", inner.as_rule()),
    }
}

/// Build a [`RangeEndpoint`] from a `scale_endpoint` pair.
fn build_range_endpoint(pair: Pair<'_, Rule>) -> Result<RangeEndpoint, ParseError> {
    let span = span_of(&pair);
    let (value, family) = build_scale_endpoint(pair)?;
    Ok(RangeEndpoint { value, family, span })
}

/// Parse a `scale_val` pair into a [`ScaleSpec`].
fn build_scale_val(pair: Pair<'_, Rule>) -> Result<ScaleSpec, ParseError> {
    // pair.as_rule() == Rule::scale_val
    let span = span_of(&pair);
    let inner = pair
        .into_inner()
        .next()
        .expect_invariant("grammar guarantees scale_val has one child");
    match inner.as_rule() {
        Rule::scale_endpoint => {
            let (value, _family) = build_scale_endpoint(inner)?;
            Ok(ScaleSpec::Scalar { value, span })
        }
        Rule::scale_range => {
            let r_span = span_of(&inner);
            let mut children = inner.into_inner();
            let kind_pair = children
                .next()
                .expect_invariant("grammar guarantees scale_range has a range_kind child");
            debug_assert_eq!(kind_pair.as_rule(), Rule::range_kind);
            let kind = match kind_pair.as_str() {
                "uni" => RangeKind::Uni,
                "bi" => RangeKind::Bi,
                other => unreachable!("unexpected range_kind: {:?}", other),
            };
            let lo = build_range_endpoint(
                children
                    .next()
                    .expect_invariant("grammar guarantees scale_range has a lo endpoint"),
            )?;
            let hi = build_range_endpoint(
                children
                    .next()
                    .expect_invariant("grammar guarantees scale_range has a hi endpoint"),
            )?;
            Ok(ScaleSpec::Range { kind, lo, hi, span: r_span })
        }
        _ => unreachable!("unexpected rule in scale_val: {:?}", inner.as_rule()),
    }
}

pub(super) fn build_arrow(pair: Pair<'_, Rule>) -> Result<Arrow, ParseError> {
    // pair.as_rule() == Rule::arrow
    let span = span_of(&pair);
    let inner = pair
        .into_inner()
        .next()
        .expect_invariant("grammar guarantees arrow has a forward_arrow or backward_arrow child");

    let direction = match inner.as_rule() {
        Rule::forward_arrow => Direction::Forward,
        Rule::backward_arrow => Direction::Backward,
        other => unreachable!("unexpected rule in arrow: {:?}", other),
    };
    let scale = inner
        .into_inner()
        .next()
        .map(build_scale_val)
        .transpose()?
        .map(Box::new);
    Ok(Arrow { direction, scale, span })
}

/// Build a `tap_target` pair into a [`TapTarget`].
///
/// Grammar: `"~" ~ tap_components ~ "(" ~ ident ~ ")"`.
/// Component identifiers are emitted with their own spans so 0696 / 0698 can
/// point diagnostics and hover at the exact component token.
pub(super) fn build_tap_target(pair: Pair<'_, Rule>) -> Result<TapTarget, ParseError> {
    // pair.as_rule() == Rule::tap_target
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    let comps_pair = it
        .next()
        .expect_invariant("grammar guarantees tap_target has a tap_components child");
    let components: Vec<Ident> = comps_pair
        .into_inner()
        .map(|c| Ident { name: c.as_str().to_owned(), span: span_of(&c) })
        .collect();

    let name = build_ident(
        it.next()
            .expect_invariant("grammar guarantees tap_target has an ident child"),
    );

    Ok(TapTarget { components, name, span })
}

fn build_cable_endpoint(pair: Pair<'_, Rule>) -> Result<CableEndpoint, ParseError> {
    // pair.as_rule() == Rule::cable_endpoint
    let inner = pair
        .into_inner()
        .next()
        .expect_invariant("grammar guarantees cable_endpoint has one child");
    match inner.as_rule() {
        Rule::tap_target => Ok(CableEndpoint::Tap(build_tap_target(inner)?)),
        Rule::port_ref => Ok(CableEndpoint::Port(build_port_ref(inner)?)),
        Rule::host_control_ref => {
            // host_control_ref = ${ "~" ~ ident } — single ident child.
            let id_pair = inner
                .into_inner()
                .next()
                .expect_invariant("grammar guarantees host_control_ref has an ident child");
            Ok(CableEndpoint::HostControlRef(build_ident(id_pair)))
        }
        _ => unreachable!("unexpected rule in cable_endpoint: {:?}", inner.as_rule()),
    }
}

/// Build a `host_control_block` pair into an AST [`HostControlBlock`]
/// (ADR 0057 Amendment §Surface syntax).
pub(super) fn build_host_control_block(
    pair: Pair<'_, Rule>,
) -> Result<HostControlBlock, ParseError> {
    // host_control_block = { host_control_kind ~ ident ~ "{" ~ (host_control_field ~ ","?)* ~ "}" }
    let span = span_of(&pair);
    let mut it = pair.into_inner();

    let kind_pair = it
        .next()
        .expect_invariant("grammar guarantees host_control_block has a host_control_kind child");
    let kind_span = span_of(&kind_pair);
    let kind = match kind_pair.as_str() {
        "knob" => HostControlKind::Knob,
        "slider" => HostControlKind::Slider,
        "toggle" => HostControlKind::Toggle,
        "trigger" => HostControlKind::Trigger,
        other => unreachable!("unexpected host_control_kind: {other:?}"),
    };

    let name = build_ident(
        it.next()
            .expect_invariant("grammar guarantees host_control_block has an ident child"),
    );

    let mut fields = Vec::new();
    for field_pair in it {
        debug_assert_eq!(field_pair.as_rule(), Rule::host_control_field);
        let f_span = span_of(&field_pair);
        let mut fi = field_pair.into_inner();
        let f_name = build_ident(
            fi.next()
                .expect_invariant("grammar guarantees host_control_field has a name ident"),
        );
        // host_control_field's value side was narrowed from `value` to
        // `scalar` in ticket 0950 (rejecting `file(...)` etc. at parse
        // time). HostControlField.value stays typed as `Value` for
        // downstream stability — wrap the scalar.
        let f_value = Value::Scalar(build_scalar(
            fi.next()
                .expect_invariant("grammar guarantees host_control_field has a scalar value"),
        )?);
        fields.push(HostControlField { name: f_name, value: f_value, span: f_span });
    }

    Ok(HostControlBlock { kind, kind_span, name, fields, span })
}

pub(super) fn build_connection(pair: Pair<'_, Rule>) -> Result<Vec<Connection>, ParseError> {
    // pair.as_rule() == Rule::connection
    // Grammar: cable_endpoint ("," cable_endpoint)* arrow
    //          cable_endpoint ("," cable_endpoint)*
    //
    // Forward (`->`) — list goes on the RHS (fan-out from one source).
    // Backward (`<-`) — list goes on the LHS (fan-in to one source).
    // Lists on the side opposite the arrow direction are rejected: that
    // would mean multiple sources driving the same sink, which the
    // engine has no semantics for.
    //
    // Each emitted Connection shares the rule's span so
    // `PatchReferences::connection_groups` can reunite the fan endpoints.
    let span = span_of(&pair);

    // Walk children, collecting the LHS list, then the arrow, then the
    // RHS list. The arrow is the only non-cable_endpoint child.
    let mut lhs_list: Vec<CableEndpoint> = Vec::new();
    let mut rhs_list: Vec<CableEndpoint> = Vec::new();
    let mut arrow_opt: Option<Arrow> = None;
    for child in pair.into_inner() {
        match child.as_rule() {
            Rule::arrow => arrow_opt = Some(build_arrow(child)?),
            Rule::cable_endpoint => {
                let ep = build_cable_endpoint(child)?;
                if arrow_opt.is_none() {
                    lhs_list.push(ep);
                } else {
                    rhs_list.push(ep);
                }
            }
            other => unreachable!("unexpected rule in connection: {other:?}"),
        }
    }
    let arrow = arrow_opt.expect_invariant("grammar guarantees a connection has an arrow child");

    match arrow.direction {
        Direction::Forward => {
            if lhs_list.len() > 1 {
                return Err(ParseError {
                    span: arrow.span,
                    message: "forward arrow `->` requires a single source on the left; \
                              for fan-in use backward arrow `<-`".to_string(),
                });
            }
        }
        Direction::Backward => {
            if rhs_list.len() > 1 {
                return Err(ParseError {
                    span: arrow.span,
                    message: "backward arrow `<-` requires a single source on the right; \
                              for fan-out use forward arrow `->`".to_string(),
                });
            }
        }
    }

    let mut connections: Vec<Connection> = Vec::new();
    match arrow.direction {
        Direction::Forward => {
            let lhs = lhs_list
                .into_iter()
                .next()
                .expect_invariant("grammar guarantees at least one lhs endpoint");
            for rhs in rhs_list {
                connections.push(Connection {
                    lhs: lhs.clone(),
                    arrow: arrow.clone(),
                    rhs,
                    span,
                });
            }
        }
        Direction::Backward => {
            let rhs = rhs_list
                .into_iter()
                .next()
                .expect_invariant("grammar guarantees at least one rhs endpoint");
            for lhs in lhs_list {
                connections.push(Connection {
                    lhs,
                    arrow: arrow.clone(),
                    rhs: rhs.clone(),
                    span,
                });
            }
        }
    }
    Ok(connections)
}

pub(super) fn build_statements(pair: Pair<'_, Rule>) -> Result<Vec<Statement>, ParseError> {
    // pair.as_rule() == Rule::statement
    let inner = pair
        .into_inner()
        .next()
        .expect_invariant("grammar guarantees statement has one child");
    match inner.as_rule() {
        Rule::module_decl => Ok(vec![Statement::Module(build_module_decl(inner)?)]),
        Rule::song_block => Ok(vec![Statement::Song(build_song_block(inner)?)]),
        Rule::pattern_block => Ok(vec![Statement::Pattern(build_pattern_block(inner)?)]),
        Rule::host_control_block => Ok(vec![Statement::HostControl(build_host_control_block(inner)?)]),
        Rule::connection => Ok(build_connection(inner)?
            .into_iter()
            .map(Statement::Connection)
            .collect()),
        _ => unreachable!("unexpected rule in statement: {:?}", inner.as_rule()),
    }
}
