//! Connection flattening phase of expansion.
//!
//! Owns port-index resolution, template-boundary port bookkeeping, and the
//! free helpers that resolve the source and destination endpoints of a
//! connection against the surrounding template-instance port maps.
//!
//! The [`Expander`](super::Expander) orchestrates the per-connection logic
//! (scale composition, boundary emission); this module provides the stateless
//! primitives it composes.

use std::collections::HashMap;

use patches_core::QName;

use super::{list_keys, qualify, scalar_to_u32, ExpandError};
use crate::ast::{
    PortIndex, PortLabel, PortRef, RangeEndpoint, RangeKind, ScaleSpec, Scalar, Span, UnitFamily,
};
use crate::flat::{CableMap, PortDirection};
use crate::structural::StructuralCode as Code;

/// The `(module, port, index)` triple that identifies a port on a module.
///
/// Generic over the module type because the expander uses two flavours:
/// `PortAddr<QName>` for fully-qualified endpoints (inside `PortEntry` /
/// resolved template-boundary maps), and `PortAddr<String>` for authored
/// references held inside [`PortBinding`](super::PortBinding) before
/// qualification.
#[derive(Debug, Clone)]
pub(super) struct PortAddr<M> {
    pub(super) module: M,
    pub(super) port: String,
    pub(super) index: u32,
}

impl<M> PortAddr<M> {
    pub(super) fn new(module: M, port: String, index: u32) -> Self {
        Self { module, port, index }
    }
}

/// A resolved port endpoint: fully-qualified address plus the affine
/// + clip map accumulated along the path to it (ADR 0062).
#[derive(Debug, Clone)]
pub(super) struct PortEntry {
    pub(super) addr: PortAddr<QName>,
    pub(super) map: CableMap,
}

impl PortEntry {
    /// Construct an entry with a pure-scalar map (`CableMap::scalar(k)`).
    pub(super) fn scalar(module: QName, port: String, index: u32, scale: f64) -> Self {
        Self {
            addr: PortAddr::new(module, port, index),
            map: CableMap::scalar(scale),
        }
    }

    /// Construct an entry with an explicit map.
    pub(super) fn with_map(addr: PortAddr<QName>, map: CableMap) -> Self {
        Self { addr, map }
    }
}

/// Port maps produced when expanding a template body.
pub(super) struct TemplatePorts {
    /// Template in-port key → list of inner module port endpoints.
    ///
    /// Keys are either a plain port name (`"freq"`) for scalar ports, or
    /// `"port/i"` for arity-expanded ports (e.g. `"in/0"`, `"in/1"`).
    /// An in-port may fan out to multiple inner ports.
    pub(super) in_ports: HashMap<String, Vec<PortEntry>>,
    /// Template out-port key → inner module port endpoint (source).
    ///
    /// Keys follow the same convention as `in_ports`.
    pub(super) out_ports: HashMap<String, PortEntry>,
}

/// Resolved form of a port index.
pub(super) enum IndexResolution {
    /// No explicit index (`None`) or a literal (`Literal(k)`).
    /// Uses the plain boundary-map key (`"port"`) so scalar in/out-ports work
    /// correctly.
    Single(u32),
    /// A param index (`[k]`): concrete value but must use the indexed
    /// boundary-map key (`"port/k"`) so it slots into the right group-port
    /// entry alongside any `[*n]` arity expansion on the same port.
    Keyed(u32),
    /// An arity expansion (`[*n]`): expand over `0..n`, each using the indexed
    /// boundary-map key.
    Arity(u32),
}

/// Look up a module-level index alias (`[name]`) in `alias_map`, returning
/// the concrete port-group slot index. Used by both `ParamEntry::KeyValue`
/// and `ParamEntry::AtBlock` param-expansion arms. `context` is appended to
/// the error message suffix (e.g. `" for @-block"`).
pub(super) fn deref_index_alias(
    alias: &str,
    alias_map: &HashMap<String, u32>,
    span: &Span,
    context: &str,
) -> Result<u32, ExpandError> {
    alias_map.get(alias).copied().ok_or_else(|| {
        ExpandError::new(
            Code::UnknownAlias,
            *span,
            format!("alias '{}' not found in alias map{}", alias, context),
        )
    })
}

/// Resolve `Option<PortIndex>` to an [`IndexResolution`].
///
/// - `None`          → `Single(0)` (implicit default, plain boundary key).
/// - `Literal(k)`    → `Single(k)` (plain boundary key).
/// - `Alias(name)`   → `Keyed(k)`  (indexed boundary key; see [`IndexResolution::Keyed`]).
///   Looks up in `alias_map` first (module-level aliases), then falls back to `param_env`.
/// - `Arity(name)`   → `Arity(n)`  (fan-out over `0..n`, indexed boundary key).
pub(super) fn deref_port_index(
    index: &Option<PortIndex>,
    param_env: &HashMap<String, Scalar>,
    span: &Span,
    alias_map: Option<&HashMap<String, u32>>,
) -> Result<IndexResolution, ExpandError> {
    match index {
        None => Ok(IndexResolution::Single(0)),
        Some(PortIndex::Literal(k)) => Ok(IndexResolution::Single(*k)),
        Some(PortIndex::Name { name, arity_marker: false }) => {
            // Try alias map first, then fall back to param_env.
            if let Some(map) = alias_map {
                if let Some(&idx) = map.get(name.as_str()) {
                    return Ok(IndexResolution::Keyed(idx));
                }
            }
            let scalar = param_env.get(name.as_str()).ok_or_else(|| {
                ExpandError::new(
                    Code::UnknownAlias,
                    *span,
                    format!(
                        "unknown alias or param '{}' in port index; known aliases: {}; known params: {}",
                        name,
                        list_keys(alias_map.map(|m| m.keys().map(|s| s.as_str()))),
                        list_keys(Some(param_env.keys().map(|s| s.as_str()))),
                    ),
                )
            })?;
            Ok(IndexResolution::Keyed(scalar_to_u32(scalar, span)?))
        }
        Some(PortIndex::Name { name, arity_marker: true }) => {
            let scalar = param_env.get(name.as_str()).ok_or_else(|| {
                ExpandError::new(
                    Code::UnknownParam,
                    *span,
                    format!(
                        "unknown arity param '{}' in port index [*{}]; known params: {}",
                        name,
                        name,
                        list_keys(Some(param_env.keys().map(|s| s.as_str()))),
                    ),
                )
            })?;
            Ok(IndexResolution::Arity(scalar_to_u32(scalar, span)?))
        }
    }
}

/// Combine two [`IndexResolution`]s into a list of `(from_i, to_i, from_is_keyed, to_is_keyed)`.
///
/// The boolean flags indicate whether the boundary-map key should use the
/// indexed `"port/i"` format (`true`) or the plain `"port"` format (`false`).
///
/// - `Arity` vs `Arity`: sizes must agree; fan-out N pairs, both keyed.
/// - `Arity` vs `Single`/`Keyed`: fan-out N pairs; the non-arity side repeats.
/// - `Keyed` vs anything non-arity: single pair, both sides keyed.
/// - `Single` vs `Single`: single pair, neither keyed.
pub(super) fn combine_index_resolutions(
    from_res: IndexResolution,
    to_res: IndexResolution,
    span: &Span,
) -> Result<Vec<(u32, u32, bool, bool)>, ExpandError> {
    use IndexResolution::{Arity, Keyed, Single};
    match (from_res, to_res) {
        (Single(f), Single(t)) => Ok(vec![(f, t, false, false)]),
        (Keyed(f),  Single(t)) => Ok(vec![(f, t, true,  false)]),
        (Single(f), Keyed(t))  => Ok(vec![(f, t, false, true)]),
        (Keyed(f),  Keyed(t))  => Ok(vec![(f, t, true,  true)]),
        (Arity(n), Arity(m)) => {
            if n != m {
                return Err(ExpandError::new(
                    Code::ArityMismatch,
                    *span,
                    format!(
                        "arity mismatch on both sides of connection: [*{}] vs [*{}]",
                        n, m
                    ),
                ));
            }
            Ok((0..n).map(|i| (i, i, true, true)).collect())
        }
        (Arity(n), Single(t)) | (Arity(n), Keyed(t)) => {
            Ok((0..n).map(|i| (i, t, true, false)).collect())
        }
        (Single(f), Arity(n)) | (Keyed(f), Arity(n)) => {
            Ok((0..n).map(|i| (f, i, false, true)).collect())
        }
    }
}

/// Resolve a `PortLabel` to a concrete port name string.
///
/// `PortLabel::Literal` is returned as-is.
/// `PortLabel::Param(name)` is looked up in `param_env`; the resolved scalar
/// must be string-compatible (`Scalar::Str`).
pub(super) fn subst_port_label(
    label: &PortLabel,
    param_env: &HashMap<String, Scalar>,
    span: &Span,
) -> Result<String, ExpandError> {
    match label {
        PortLabel::Literal(s) => Ok(s.clone()),
        PortLabel::Param(name) => match param_env.get(name.as_str()) {
            Some(Scalar::Str(s)) => Ok(s.clone()),
            Some(other) => Err(ExpandError::new(
                Code::ParamTypeMismatch,
                *span,
                format!(
                    "param '{}' used as port label must resolve to a string, got {:?}",
                    name, other
                ),
            )),
            None => Err(ExpandError::new(
                Code::UnknownParam,
                *span,
                format!("unknown param '{}' referenced in port label", name),
            )),
        },
    }
}

/// Resolve `Option<&ScaleSpec>` arrow scale to a [`CableMap`].
///
/// `None` → `CableMap::identity()`.
/// `Some(ScaleSpec::Scalar)` substitutes via `param_env` and produces
/// `CableMap::scalar(k)` (the fast path).
/// `Some(ScaleSpec::Range)` performs unit-family resolution (ticket
/// 0766) and lowers per ADR 0062 §Lowering:
///
/// - `uni(lo, hi)` → `scale = hi - lo`, `offset = lo`,
///   `clip = Some((min(lo,hi), max(lo,hi)))`.
/// - `bi(lo, hi)`  → `scale = (hi - lo) / 2`,
///   `offset = (hi + lo) / 2`,
///   `clip = Some((min(lo,hi), max(lo,hi)))`.
pub(super) fn eval_scale(
    scale: Option<&ScaleSpec>,
    param_env: &HashMap<String, Scalar>,
    arrow_span: &Span,
) -> Result<CableMap, ExpandError> {
    match scale {
        None => Ok(CableMap::identity()),
        Some(ScaleSpec::Scalar { value: s, .. }) => {
            let resolved = if let Scalar::ParamRef(name) = s {
                param_env.get(name.as_str()).unwrap_or(s)
            } else {
                s
            };
            match resolved {
                Scalar::Float(f) => Ok(CableMap::scalar(*f)),
                Scalar::Int(i) => Ok(CableMap::scalar(*i as f64)),
                other => Err(ExpandError::new(
                    Code::InvalidCableScale,
                    *arrow_span,
                    format!("arrow scale must resolve to a number, got {:?}", other),
                )),
            }
        }
        Some(ScaleSpec::Range { kind, lo, hi, span: _ }) => {
            let (_family, lo_val, hi_val) = resolve_range_endpoints(lo, hi, param_env)?;
            let clip_lo = lo_val.min(hi_val);
            let clip_hi = lo_val.max(hi_val);
            let (scale, offset) = match kind {
                RangeKind::Uni => (hi_val - lo_val, lo_val),
                RangeKind::Bi => ((hi_val - lo_val) / 2.0, (hi_val + lo_val) / 2.0),
            };
            Ok(CableMap { scale, offset, clip: Some((clip_lo, clip_hi)) })
        }
    }
}

/// Resolve a range scale's endpoints into a unified family and
/// concrete `(lo, hi)` f64 values.
///
/// Param refs are looked up in `param_env`; a resolved `Scalar::Float`
/// or `Scalar::Int` becomes a numeric value with `UnitFamily::Plain`
/// (parse-time unit context cannot be reconstructed from a bare
/// scalar). Unresolved param refs likewise stay `Plain` — runtime
/// recompute is ticket 0769.
///
/// `Plain` mixes with any other family; two distinct non-Plain
/// families produce a cross-family `InvalidCableScale` error naming
/// both sides.
pub(super) fn resolve_range_endpoints(
    lo: &RangeEndpoint,
    hi: &RangeEndpoint,
    param_env: &HashMap<String, Scalar>,
) -> Result<(UnitFamily, f64, f64), ExpandError> {
    let (lo_val, lo_family) = resolve_range_endpoint(lo, param_env)?;
    let (hi_val, hi_family) = resolve_range_endpoint(hi, param_env)?;
    let unified = lo_family.unify(hi_family).ok_or_else(|| {
        ExpandError::new(
            Code::InvalidCableScale,
            hi.span,
            format!(
                "cable range endpoints have incompatible unit families: {} vs {}",
                lo_family.name(),
                hi_family.name(),
            ),
        )
    })?;
    Ok((unified, lo_val, hi_val))
}

fn resolve_range_endpoint(
    ep: &RangeEndpoint,
    param_env: &HashMap<String, Scalar>,
) -> Result<(f64, UnitFamily), ExpandError> {
    let (resolved, family) = if let Scalar::ParamRef(name) = &ep.value {
        match param_env.get(name.as_str()) {
            Some(s) => (s.clone(), UnitFamily::Plain),
            None => (ep.value.clone(), ep.family),
        }
    } else {
        (ep.value.clone(), ep.family)
    };
    let value = match resolved {
        Scalar::Float(f) => f,
        Scalar::Int(i) => i as f64,
        Scalar::ParamRef(_) => {
            // Unresolved param ref; lowering is 0769. For now treat as
            // 0.0 so family unification still runs and downstream code
            // sees the (intentional) "not yet implemented" error from
            // the caller.
            0.0
        }
        other => {
            return Err(ExpandError::new(
                Code::InvalidCableScale,
                ep.span,
                format!("range endpoint must resolve to a number, got {:?}", other),
            ));
        }
    };
    Ok((value, family))
}

/// Fail-fast: if `pr` refers to a known template instance, verify that
/// `port` appears in the appropriate boundary map (in or out) before any
/// downstream index-alias resolution. `$` and plain (non-template) modules
/// are skipped — boundaries are defined on the fly, and plain-module port
/// descriptors aren't visible to the DSL.
pub(super) fn check_template_port(
    pr: &PortRef,
    port: &str,
    instance_ports: &HashMap<String, TemplatePorts>,
    dir: PortDirection,
) -> Result<(), ExpandError> {
    if pr.module == "$" {
        return Ok(());
    }
    let Some(ports) = instance_ports.get(pr.module.as_str()) else {
        return Ok(());
    };
    let keys: Vec<&String> = match dir {
        PortDirection::Output => ports.out_ports.keys().collect(),
        PortDirection::Input => ports.in_ports.keys().collect(),
    };
    let prefix = format!("{}/", port);
    let exists = keys.iter().any(|k| k.as_str() == port || k.starts_with(&prefix));
    if exists {
        Ok(())
    } else {
        let kind = match dir {
            PortDirection::Output => "out-port",
            PortDirection::Input => "in-port",
        };
        // Collapse "port/i" keys to bare base names for the suggestion list.
        let mut names: Vec<&str> = keys
            .iter()
            .map(|k| k.split_once('/').map(|(b, _)| b).unwrap_or(k.as_str()))
            .collect();
        names.sort_unstable();
        names.dedup();
        let known = if names.is_empty() {
            "(none)".to_owned()
        } else {
            names.join(", ")
        };
        Err(ExpandError::new(
            Code::UnknownPortOnModule,
            pr.span,
            format!(
                "template instance '{}' has no {} '{}'; known {}s: {}",
                pr.module, kind, port, kind, known
            ),
        ))
    }
}

/// Resolve the **source** side of a connection.
///
/// If `from_module` is a known template instance, looks up its out-port map.
/// The lookup tries the indexed key `"port/i"` first (for arity-declared ports),
/// then falls back to the plain key `"port"`.
pub(super) fn resolve_from(
    from_module: &str,
    from_port: &str,
    from_index: u32,
    namespace: Option<&QName>,
    instance_ports: &HashMap<String, TemplatePorts>,
    span: &Span,
) -> Result<PortEntry, ExpandError> {
    if let Some(ports) = instance_ports.get(from_module) {
        // Try indexed key first (arity port), then plain key.
        let indexed_key = format!("{}/{}", from_port, from_index);
        if let Some(entry) = ports.out_ports.get(&indexed_key) {
            return Ok(entry.clone());
        }
        ports.out_ports.get(from_port).cloned().ok_or_else(|| {
            ExpandError::new(
                Code::UnknownPortOnModule,
                *span,
                format!(
                    "template instance '{}' has no out-port '{}'",
                    from_module, from_port
                ),
            )
        })
    } else {
        Ok(PortEntry::scalar(
            qualify(namespace, from_module),
            from_port.to_owned(),
            from_index,
            1.0,
        ))
    }
}

/// Resolve the **destination** side of a connection.
///
/// If `to_module` is a known template instance, looks up its in-port map.
/// The lookup tries the indexed key `"port/i"` first (for arity-declared ports),
/// then falls back to the plain key `"port"` (for plain ports — the explicit
/// index on the calling side is passed through to the concrete destination).
pub(super) fn resolve_to(
    to_module: &str,
    to_port: &str,
    to_index: u32,
    namespace: Option<&QName>,
    instance_ports: &HashMap<String, TemplatePorts>,
    span: &Span,
) -> Result<Vec<PortEntry>, ExpandError> {
    if let Some(ports) = instance_ports.get(to_module) {
        // Try indexed key first (arity port), then plain key.
        let indexed_key = format!("{}/{}", to_port, to_index);
        if let Some(entries) = ports.in_ports.get(&indexed_key) {
            return Ok(entries.clone());
        }
        ports.in_ports.get(to_port).cloned().ok_or_else(|| {
            ExpandError::new(
                Code::UnknownPortOnModule,
                *span,
                format!(
                    "template instance '{}' has no in-port '{}'",
                    to_module, to_port
                ),
            )
        })
    } else {
        Ok(vec![PortEntry::scalar(
            qualify(namespace, to_module),
            to_port.to_owned(),
            to_index,
            1.0,
        )])
    }
}
