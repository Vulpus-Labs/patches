//! `patches-interpreter` — validates and builds a [`ModuleGraph`] from a
//! [`patches_dsl::FlatPatch`].
//!
//! # Responsibilities
//!
//! - Holds the module factory registry (type name → descriptor + builder).
//! - Resolves module type names from the flat AST against the registry.
//! - Validates shape args, port references, and parameter values against
//!   the module's [`ModuleDescriptor`].
//! - Calls [`ModuleGraph::add_module`] and [`ModuleGraph::connect`] to
//!   construct the runtime graph.
//! - Propagates source spans from the AST into error messages.
//!
//! This crate knows about concrete module types (via `patches-modules`) but
//! has no audio-backend or engine dependencies.

use std::path::Path;

use patches_core::{
    AudioEnvironment, ModuleGraph, ModuleShape, Registry,
    ParameterMap, ParameterValue, ParameterKind,
    PortRef,
};
use patches_dsl::ast::{Scalar, Span, Value};
use patches_dsl::flat::{FlatConnection, FlatModule, FlatPatch};

/// An error produced during interpretation of a [`FlatPatch`].
///
/// Carries the source [`Span`] of the offending construct and a
/// human-readable message describing the problem.
#[derive(Debug)]
pub struct InterpretError {
    pub span: Span,
    pub message: String,
}

impl std::fmt::Display for InterpretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at {}..{})", self.message, self.span.start, self.span.end)
    }
}

impl std::error::Error for InterpretError {}

/// Build a [`ModuleGraph`] from a validated [`FlatPatch`].
///
/// Module type names are resolved against `registry`. Shape args and
/// parameter values are validated against each module's
/// [`patches_core::ModuleDescriptor`]. Connection port names are validated
/// against the descriptors already added to the graph, so forward references
/// within a single patch are not errors.
///
/// Returns an [`InterpretError`] with the source span of the offending
/// declaration on the first validation failure encountered.
pub fn build(
    flat: &FlatPatch,
    registry: &Registry,
    _env: &AudioEnvironment,
) -> Result<ModuleGraph, InterpretError> {
    build_with_base_dir(flat, registry, _env, None)
}

/// Build a [`ModuleGraph`] from a validated [`FlatPatch`], resolving
/// relative file paths against `base_dir`.
///
/// String parameters whose descriptor name is `"path"` are resolved
/// relative to `base_dir` when the value is not already an absolute path.
/// Pass `None` to leave paths as-is (same behaviour as [`build`]).
pub fn build_with_base_dir(
    flat: &FlatPatch,
    registry: &Registry,
    _env: &AudioEnvironment,
    base_dir: Option<&Path>,
) -> Result<ModuleGraph, InterpretError> {
    let mut graph = ModuleGraph::new();

    // Stage 1 ��� add all module nodes.
    for flat_module in &flat.modules {
        add_module(&mut graph, flat_module, registry, base_dir)?;
    }

    // Stage 2 — add all connections (after all nodes are present so that
    // forward references within a patch are not errors).
    for conn in &flat.connections {
        add_connection(&mut graph, conn)?;
    }

    Ok(graph)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn add_module(
    graph: &mut ModuleGraph,
    flat_module: &FlatModule,
    registry: &Registry,
    base_dir: Option<&Path>,
) -> Result<(), InterpretError> {
    let shape = shape_from_args(&flat_module.shape);

    let descriptor = registry
        .describe(&flat_module.type_name, &shape)
        .map_err(|e| InterpretError {
            span: flat_module.span,
            message: e.to_string(),
        })?;

    let params = convert_params(&flat_module.params, &descriptor, base_dir).map_err(|msg| {
        InterpretError { span: flat_module.span, message: msg }
    })?;

    patches_core::validate_parameters(&params, &descriptor).map_err(|e| InterpretError {
        span: flat_module.span,
        message: e.to_string(),
    })?;

    graph
        .add_module(flat_module.id.clone(), descriptor, &params)
        .map_err(|e| InterpretError {
            span: flat_module.span,
            message: e.to_string(),
        })
}

fn add_connection(
    graph: &mut ModuleGraph,
    conn: &FlatConnection,
) -> Result<(), InterpretError> {
    let from_id = patches_core::NodeId::from(conn.from_module.clone());
    let to_id = patches_core::NodeId::from(conn.to_module.clone());

    // Resolve source output port — borrow and copy the &'static str name from
    // the descriptor so we can call connect() without holding the borrow.
    let output_port = {
        let from_node = graph.get_node(&from_id).ok_or_else(|| InterpretError {
            span: conn.span,
            message: format!("module '{}' not found", conn.from_module),
        })?;
        from_node
            .module_descriptor
            .outputs
            .iter()
            .find(|p| p.name == conn.from_port.as_str() && p.index == conn.from_index as usize)
            .map(|p| PortRef { name: p.name, index: p.index })
            .ok_or_else(|| {
                let available: Vec<String> = from_node.module_descriptor.outputs.iter()
                    .map(|p| format!("{}/{}", p.name, p.index))
                    .collect();
                InterpretError {
                    span: conn.span,
                    message: format!(
                        "module '{}' has no output port '{}/{}'; available outputs: [{}]",
                        conn.from_module, conn.from_port, conn.from_index, available.join(", ")
                    ),
                }
            })?
    };

    // Resolve destination input port.
    let input_port = {
        let to_node = graph.get_node(&to_id).ok_or_else(|| InterpretError {
            span: conn.span,
            message: format!("module '{}' not found", conn.to_module),
        })?;
        to_node
            .module_descriptor
            .inputs
            .iter()
            .find(|p| p.name == conn.to_port.as_str() && p.index == conn.to_index as usize)
            .map(|p| PortRef { name: p.name, index: p.index })
            .ok_or_else(|| {
                let available: Vec<String> = to_node.module_descriptor.inputs.iter()
                    .map(|p| format!("{}/{}", p.name, p.index))
                    .collect();
                InterpretError {
                    span: conn.span,
                    message: format!(
                        "module '{}' has no input port '{}/{}'; available inputs: [{}]",
                        conn.to_module, conn.to_port, conn.to_index, available.join(", ")
                    ),
                }
            })?
    };

    graph
        .connect(&from_id, output_port, &to_id, input_port, conn.scale as f32)
        .map_err(|e| InterpretError {
            span: conn.span,
            message: e.to_string(),
        })
}

/// Convert `Vec<(String, Scalar)>` shape arguments to a [`ModuleShape`].
///
/// Recognised keys are `"channels"` and `"length"`; unrecognised keys are
/// silently ignored (the registry's `describe` implementation is responsible
/// for validating shape semantics).
fn shape_from_args(args: &[(String, Scalar)]) -> ModuleShape {
    let mut channels = 0usize;
    let mut length = 0usize;
    let mut high_quality = false;
    for (name, scalar) in args {
        match name.as_str() {
            "channels" => {
                if let Scalar::Int(n) = scalar {
                    channels = *n as usize;
                }
            }
            "length" => {
                if let Scalar::Int(n) = scalar {
                    length = *n as usize;
                }
            }
            "high_quality" => {
                if let Scalar::Bool(b) = scalar {
                    high_quality = *b;
                }
            }
            _ => {}
        }
    }
    ModuleShape { channels, length, high_quality }
}

/// Parse a parameter name string of the form `"name"` or `"name/N"` into a
/// base name and index.
fn parse_param_name(name: &str) -> (&str, usize) {
    if let Some(pos) = name.rfind('/') {
        let base = &name[..pos];
        let idx_str = &name[pos + 1..];
        if let Ok(idx) = idx_str.parse::<usize>() {
            return (base, idx);
        }
    }
    (name, 0)
}

/// Convert a slice of `(name, Value)` DSL param pairs into a [`ParameterMap`],
/// validating each value's type against the module's [`patches_core::ModuleDescriptor`].
///
/// Returns `Err(message)` on the first type incompatibility or unrecognised
/// parameter name encountered.
fn convert_params(
    params: &[(String, Value)],
    descriptor: &patches_core::ModuleDescriptor,
    base_dir: Option<&Path>,
) -> Result<ParameterMap, String> {
    let mut map = ParameterMap::new();
    for (raw_name, value) in params {
        let (base_name, idx) = parse_param_name(raw_name);

        let param_desc = descriptor
            .parameters
            .iter()
            .find(|p| p.name == base_name && p.index == idx)
            .ok_or_else(|| {
                let mut known: Vec<String> = descriptor
                    .parameters
                    .iter()
                    .map(|p| {
                        if p.index == 0 {
                            p.name.to_string()
                        } else {
                            format!("{}/{}", p.name, p.index)
                        }
                    })
                    .collect();
                known.sort();
                known.dedup();
                format!(
                    "unknown parameter '{raw_name}'; known parameters: {}",
                    known.join(", ")
                )
            })?;

        let mut pv = convert_value(value, &param_desc.parameter_type)
            .map_err(|e| format!("parameter '{raw_name}': {e}"))?;

        // Resolve relative file paths against the patch file's directory.
        if let Some(dir) = base_dir {
            match &mut pv {
                ParameterValue::File(s) if !s.is_empty() && !Path::new(s.as_str()).is_absolute() => {
                    *s = dir.join(s.as_str()).to_string_lossy().into_owned();
                }
                ParameterValue::String(s)
                    if matches!(param_desc.parameter_type, ParameterKind::String { .. })
                        && param_desc.name == "path"
                        && !s.is_empty()
                        && !Path::new(s.as_str()).is_absolute() =>
                {
                    *s = dir.join(s.as_str()).to_string_lossy().into_owned();
                }
                _ => {}
            }
        }

        map.insert_param(base_name.to_string(), idx, pv);
    }
    Ok(map)
}

/// Convert a DSL [`Value`] to a [`ParameterValue`] given the expected
/// [`ParameterKind`] from the module descriptor.
///
/// Integer literals are accepted where a float is expected (widening
/// conversion). Enum string values are resolved to a `&'static str` from the
/// declared variant list.
fn convert_value(value: &Value, kind: &ParameterKind) -> Result<ParameterValue, String> {
    match (value, kind) {
        (Value::Scalar(Scalar::Float(f)), ParameterKind::Float { .. }) => {
            Ok(ParameterValue::Float(*f as f32))
        }
        (Value::Scalar(Scalar::Int(i)), ParameterKind::Float { .. }) => {
            Ok(ParameterValue::Float(*i as f32))
        }
        (Value::Scalar(Scalar::Int(i)), ParameterKind::Int { .. }) => {
            Ok(ParameterValue::Int(*i))
        }
        (Value::Scalar(Scalar::Bool(b)), ParameterKind::Bool { .. }) => {
            Ok(ParameterValue::Bool(*b))
        }
        (Value::Scalar(Scalar::Str(s)), ParameterKind::Enum { variants, .. }) => variants
            .iter()
            .find(|&&v| v == s.as_str())
            .map(|&v| ParameterValue::Enum(v))
            .ok_or_else(|| format!("invalid enum variant '{s}'")),
        (Value::Scalar(Scalar::Str(s)), ParameterKind::String { .. }) => {
            Ok(ParameterValue::String(s.clone()))
        }
        (Value::File(path), ParameterKind::File { extensions }) => {
            // Validate file extension if the path is non-empty.
            if !path.is_empty() {
                let ext = std::path::Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if !extensions.is_empty() && !extensions.iter().any(|&e| e.eq_ignore_ascii_case(ext)) {
                    return Err(format!(
                        "unsupported file extension '.{ext}'; expected one of: {}",
                        extensions.join(", ")
                    ));
                }
            }
            Ok(ParameterValue::File(path.clone()))
        }
        (Value::Array(items), ParameterKind::Array { .. }) => {
            let strings = items
                .iter()
                .map(|item| match item {
                    Value::Scalar(Scalar::Str(s)) => Ok(s.clone()),
                    _ => Err("array elements must be strings".to_string()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ParameterValue::Array(strings.into()))
        }
        _ => Err(format!("expected {}, found {}", kind.kind_name(), value_kind_name(value))),
    }
}

fn value_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Scalar(Scalar::Float(_)) => "float",
        Value::Scalar(Scalar::Int(_)) => "int",
        Value::Scalar(Scalar::Bool(_)) => "bool",
        Value::Scalar(Scalar::Str(_)) => "string",
        Value::Scalar(Scalar::ParamRef(_)) => "param-ref",
        Value::Array(_) => "array",
        Value::Table(_) => "table",
        Value::File(_) => "file",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_dsl::flat::{FlatConnection, FlatModule, FlatPatch};
    use patches_dsl::ast::{Scalar, Span, Value};

    fn span() -> Span {
        Span { start: 0, end: 0 }
    }

    fn env() -> AudioEnvironment {
        AudioEnvironment { sample_rate: 44100.0, poly_voices: 16, periodic_update_interval: 32 }
    }

    fn registry() -> Registry {
        patches_modules::default_registry()
    }

    fn osc_module(id: &str) -> FlatModule {
        FlatModule {
            id: id.to_string(),
            type_name: "Osc".to_string(),
            shape: vec![],
            params: vec![],
            span: span(),
        }
    }

    fn sum_module(id: &str, channels: i64) -> FlatModule {
        FlatModule {
            id: id.to_string(),
            type_name: "Sum".to_string(),
            shape: vec![("channels".to_string(), Scalar::Int(channels))],
            params: vec![],
            span: span(),
        }
    }

    fn connection(
        from_module: &str, from_port: &str, from_index: u32,
        to_module: &str, to_port: &str, to_index: u32,
    ) -> FlatConnection {
        FlatConnection {
            from_module: from_module.to_string(),
            from_port: from_port.to_string(),
            from_index,
            to_module: to_module.to_string(),
            to_port: to_port.to_string(),
            to_index,
            scale: 1.0,
            span: span(),
        }
    }

    #[test]
    fn build_single_module_patch() {
        let flat = FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![osc_module("osc1")],
            connections: vec![],
        };
        let graph = build(&flat, &registry(), &env()).unwrap();
        assert_eq!(graph.node_ids().len(), 1);
    }

    #[test]
    fn build_two_modules_with_connection() {
        let flat = FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![osc_module("osc1"), sum_module("mix", 1)],
            connections: vec![connection("osc1", "sine", 0, "mix", "in", 0)],
        };
        let graph = build(&flat, &registry(), &env()).unwrap();
        assert_eq!(graph.node_ids().len(), 2);
        assert_eq!(graph.edge_list().len(), 1);
    }

    #[test]
    fn forward_references_are_not_errors() {
        // Connections are processed after all modules, so module order doesn't matter.
        let flat = FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![sum_module("mix", 1), osc_module("osc1")],
            connections: vec![connection("osc1", "sine", 0, "mix", "in", 0)],
        };
        assert!(build(&flat, &registry(), &env()).is_ok());
    }

    #[test]
    fn unknown_type_name_returns_interpret_error() {
        let flat = FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![FlatModule {
                id: "x".to_string(),
                type_name: "NonExistentModule".to_string(),
                shape: vec![],
                params: vec![],
                span: Span { start: 10, end: 20 },
            }],
            connections: vec![],
        };
        let err = match build(&flat, &registry(), &env()) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e,
        };
        assert!(err.message.contains("NonExistentModule"));
        assert_eq!(err.span, Span { start: 10, end: 20 });
    }

    #[test]
    fn unknown_output_port_returns_interpret_error() {
        let flat = FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![osc_module("osc1"), sum_module("mix", 1)],
            connections: vec![FlatConnection {
                from_module: "osc1".to_string(),
                from_port: "no_such_out".to_string(),
                from_index: 0,
                to_module: "mix".to_string(),
                to_port: "in".to_string(),
                to_index: 0,
                scale: 1.0,
                span: Span { start: 5, end: 15 },
            }],
        };
        let err = match build(&flat, &registry(), &env()) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e,
        };
        assert!(err.message.contains("no_such_out"));
        assert_eq!(err.span, Span { start: 5, end: 15 });
    }

    #[test]
    fn unknown_input_port_returns_interpret_error() {
        let flat = FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![osc_module("osc1"), sum_module("mix", 1)],
            connections: vec![FlatConnection {
                from_module: "osc1".to_string(),
                from_port: "sine".to_string(),
                from_index: 0,
                to_module: "mix".to_string(),
                to_port: "no_such_in".to_string(),
                to_index: 0,
                scale: 1.0,
                span: Span { start: 3, end: 9 },
            }],
        };
        let err = match build(&flat, &registry(), &env()) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e,
        };
        assert!(err.message.contains("no_such_in"));
        assert_eq!(err.span, Span { start: 3, end: 9 });
    }

    #[test]
    fn graph_error_wrapped_with_span() {
        // Connect same input twice — GraphError::InputAlreadyConnected.
        let osc2 = FlatModule {
            id: "osc2".to_string(),
            type_name: "Osc".to_string(),
            shape: vec![],
            params: vec![],
            span: span(),
        };
        let dup_conn = FlatConnection {
            from_module: "osc2".to_string(),
            from_port: "sine".to_string(),
            from_index: 0,
            to_module: "mix".to_string(),
            to_port: "in".to_string(),
            to_index: 0,
            scale: 1.0,
            span: Span { start: 50, end: 60 },
        };
        let flat = FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![osc_module("osc1"), osc2, sum_module("mix", 1)],
            connections: vec![
                connection("osc1", "sine", 0, "mix", "in", 0),
                dup_conn,
            ],
        };
        let err = match build(&flat, &registry(), &env()) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e,
        };
        assert_eq!(err.span, Span { start: 50, end: 60 });
    }

    #[test]
    fn float_param_is_accepted() {
        let flat = FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![FlatModule {
                id: "osc1".to_string(),
                type_name: "Osc".to_string(),
                shape: vec![],
                params: vec![
                    // V/OCT offset from C0 (≈16.35 Hz); 440 Hz ≈ 4.75 V/OCT
                    ("frequency".to_string(), Value::Scalar(Scalar::Float((440.0_f64 / 16.351_597_831_287_414).log2()))),
                ],
                span: span(),
            }],
            connections: vec![],
        };
        assert!(build(&flat, &registry(), &env()).is_ok());
    }

    #[test]
    fn enum_param_is_accepted() {
        let flat = FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![FlatModule {
                id: "osc1".to_string(),
                type_name: "Osc".to_string(),
                shape: vec![],
                params: vec![
                    ("fm_type".to_string(), Value::Scalar(Scalar::Str("logarithmic".to_string()))),
                ],
                span: span(),
            }],
            connections: vec![],
        };
        assert!(build(&flat, &registry(), &env()).is_ok());
    }

    #[test]
    fn poly_synth_layered_patches_file_builds() {
        let src = std::fs::read_to_string(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/poly_synth_layered.patches"),
        )
        .expect("poly_synth_layered.patches not found");
        let file = patches_dsl::parse(&src).expect("parse failed");
        let result = patches_dsl::expand(&file).expect("expand failed");
        let graph = build(&result.patch, &registry(), &env()).expect("build failed");
        // 12 patch-level modules + lo/voice (9) + hi/noise_voice (8) = 29
        assert_eq!(graph.node_ids().len(), 27);
    }

    #[test]
    fn poly_synth_patches_file_builds() {
        // End-to-end smoke test: parse → expand → build the example patch file.
        let src = std::fs::read_to_string(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/poly_synth.patches"),
        )
        .expect("poly_synth.patches not found");
        let file = patches_dsl::parse(&src).expect("parse failed");
        let result = patches_dsl::expand(&file).expect("expand failed");
        let graph = build(&result.patch, &registry(), &env()).expect("build failed");
        assert_eq!(graph.node_ids().len(), 11);
        assert_eq!(graph.edge_list().len(), 16);
    }

    #[test]
    fn unknown_param_name_returns_interpret_error() {
        let flat = FlatPatch {
            patterns: vec![],
            songs: vec![],
            modules: vec![FlatModule {
                id: "osc1".to_string(),
                type_name: "Osc".to_string(),
                shape: vec![],
                params: vec![
                    ("no_such_param".to_string(), Value::Scalar(Scalar::Float(1.0))),
                ],
                span: Span { start: 1, end: 5 },
            }],
            connections: vec![],
        };
        let err = match build(&flat, &registry(), &env()) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e,
        };
        assert!(err.message.contains("no_such_param"));
    }
}
