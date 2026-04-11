use super::parameter_map::ParameterValue;

/// Describes a single port on a module by name and index.
///
/// The `index` field is the user-visible number in a multi-port group (e.g.
/// `in/2` has `name = "in"` and `index = 2`). For modules with a single port
/// of a given name, `index` is `0`. The position of a `PortDescriptor` in
/// `ModuleDescriptor::inputs` / `outputs` determines the slice offset passed to
/// `Module::process`; `index` is semantically distinct from that position.
///
/// `kind` declares whether the port carries a mono or poly signal. Port arity
/// is fixed at module-definition time and used by [`ModuleGraph::connect`] to
/// reject kind-mismatched connections at graph-construction time.
#[derive(Debug, Clone)]
pub struct PortDescriptor {
    pub name: &'static str,
    pub index: usize,
    pub kind: crate::cables::CableKind,
}

/// A reference to a named, indexed port used in `ModuleGraph::connect()`.
///
/// Port names are always `&'static str` (defined by module implementations at
/// compile time), so producing a `PortRef` never allocates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortRef {
    pub name: &'static str,
    pub index: usize,
}

/// A static-lifetime reference to a named, indexed parameter.
///
/// Mirrors [`PortRef`] for parameters. Parameter names are compile-time
/// constants defined by module implementations, so producing a `ParameterRef`
/// never allocates.
///
/// Use [`ParameterKey`](super::parameter_map::ParameterKey) when an owned key
/// is needed (e.g. as a map entry from DSL parsing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterRef {
    pub name: &'static str,
    pub index: usize,
}

#[derive(Debug, Clone)]
pub enum ParameterKind {
    Float { min: f32, max: f32, default: f32 },
    Int   { min: i64, max: i64, default: i64 },
    Bool  { default: bool },
    Enum  { variants: &'static [&'static str], default: &'static str },
    /// A single runtime string (e.g. a file path).
    String { default: &'static str },
    /// Variable-length array of strings (e.g. a step-sequencer pattern).
    ///
    /// The `default` field uses `&'static [&'static str]` so that the descriptor itself
    /// never allocates (consistent with ADR 0011). The `ParameterValue` it produces does
    /// allocate, but only at the non-realtime boundary.
    ///
    /// `length` is the maximum number of elements the pre-allocated backing array can hold.
    /// It must match `ModuleShape::length` for the module that declares this parameter.
    /// `validate_parameters` rejects any `ParameterValue::Array` whose element count
    /// exceeds this limit.
    Array { default: &'static [&'static str], length: usize },
    /// A file path parameter. The DSL writes `file("path")` which the interpreter
    /// resolves to an absolute path against the patch file's directory.
    ///
    /// `extensions` declares the set of accepted file extensions (e.g. `&["wav", "aiff"]`).
    /// The interpreter validates the extension before the value reaches the planner.
    ///
    /// At plan-build time, modules that implement [`FileProcessor`] have their
    /// `process_file` method called, and the `ParameterValue::File` is replaced with
    /// `ParameterValue::FloatBuffer(Arc<[f32]>)` before the plan reaches the audio thread.
    File { extensions: &'static [&'static str] },
}

impl ParameterKind {
    /// Return the default value for this parameter kind as a [`ParameterValue`].
    pub fn default_value(&self) -> ParameterValue {
        match self {
            ParameterKind::Float { default, .. } => ParameterValue::Float(*default),
            ParameterKind::Int   { default, .. } => ParameterValue::Int(*default),
            ParameterKind::Bool  { default }     => ParameterValue::Bool(*default),
            ParameterKind::Enum  { default, .. } => ParameterValue::Enum(default),
            ParameterKind::String { default }    => ParameterValue::String(default.to_string()),
            ParameterKind::Array { default, .. } => ParameterValue::Array(
                default.iter().map(|s| s.to_string()).collect::<Vec<_>>().into()
            ),
            ParameterKind::File { .. } => ParameterValue::File(String::new()),
        }
    }

    /// Return a short type name suitable for error messages.
    pub fn kind_name(&self) -> &'static str {
        match self {
            ParameterKind::Float { .. } => "float",
            ParameterKind::Int   { .. } => "int",
            ParameterKind::Bool  { .. } => "bool",
            ParameterKind::Enum   { .. } => "enum",
            ParameterKind::String { .. } => "string",
            ParameterKind::Array  { .. } => "array",
            ParameterKind::File  { .. } => "file",
        }
    }
}

pub struct ParameterSpec {
    pub name: &'static str,
    pub kind: ParameterKind,
}

#[derive(Debug, Clone)]
pub struct ParameterDescriptor {
    pub name: &'static str,
    pub index: usize,
    pub parameter_type: ParameterKind,
}

impl ParameterDescriptor {
    /// Return `true` if `name` and `index` refer to this parameter.
    pub fn matches(&self, name: &str, index: usize) -> bool {
        self.name == name && self.index == index
    }

    /// Return a [`ParameterRef`] identifying this parameter.
    pub fn as_ref(&self) -> ParameterRef {
        ParameterRef { name: self.name, index: self.index }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModuleShape {
    pub channels: usize,
    /// Pre-allocated step/slot count for sequencer-style modules.
    ///
    /// Set to `0` for modules that do not use array parameters. When non-zero,
    /// the module factory uses this value to pre-allocate the backing array so
    /// that subsequent `update_parameters` calls can write into the existing
    /// allocation. If the shape changes between builds the planner will
    /// tombstone the old instance and create a fresh one.
    pub length: usize,
    /// When true, modules may use larger internal buffers, higher overlap
    /// factors, or other settings that improve quality at the cost of
    /// latency and CPU.  Defaults to `false`.
    pub high_quality: bool,
}

/// Describes the full layout of a module.
///
/// Inputs, outputs, and parameters are stored in separate vecs.
///
/// The index of a port in `inputs` corresponds to the index in the `inputs` slice passed to
/// [`Module::process`], and similarly for `outputs`. The graph and patch builder
/// use this to resolve port names to slice indices at build time.
#[derive(Debug, Clone)]
pub struct ModuleDescriptor {
    pub module_name: &'static str,
    pub shape: ModuleShape,
    pub inputs: Vec<PortDescriptor>,
    pub outputs: Vec<PortDescriptor>,
    pub parameters: Vec<ParameterDescriptor>,
}

// Generates a pair of builder methods for a single port kind/direction:
//   $single(name) — appends one PortDescriptor with index 0
//   $multi(name, count) — appends `count` PortDescriptors with indices 0..count
macro_rules! port_builder {
    ($single:ident, $multi:ident, $vec:ident, $kind:expr) => {
        pub fn $single(mut self, name: &'static str) -> Self {
            self.$vec.push(PortDescriptor { name, index: 0, kind: $kind });
            self
        }

        pub fn $multi(mut self, name: &'static str, count: usize) -> Self {
            for i in 0..count {
                self.$vec.push(PortDescriptor { name, index: i, kind: $kind });
            }
            self
        }
    };
}

// Generates a pair of builder methods for a single parameter kind:
//   $single(name, $args…) — appends one ParameterDescriptor with index 0
//   $multi(name, count, $args…) — appends `count` ParameterDescriptors with indices 0..count
macro_rules! param_builder {
    ($single:ident, $multi:ident, ($($arg:ident : $ty:ty),*), $kind:expr) => {
        pub fn $single(mut self, name: &'static str, $($arg: $ty),*) -> Self {
            self.parameters.push(ParameterDescriptor {
                name,
                index: 0,
                parameter_type: $kind,
            });
            self
        }

        pub fn $multi(mut self, name: &'static str, count: usize, $($arg: $ty),*) -> Self {
            for i in 0..count {
                self.parameters.push(ParameterDescriptor {
                    name,
                    index: i,
                    parameter_type: $kind,
                });
            }
            self
        }
    };
}

impl ModuleDescriptor {
    /// Construct an empty descriptor with no ports or parameters.
    pub fn new(name: &'static str, shape: ModuleShape) -> Self {
        Self {
            module_name: name,
            shape,
            inputs: Vec::new(),
            outputs: Vec::new(),
            parameters: Vec::new(),
        }
    }

    // ── Port builder methods (generated) ────────────────────────────────────

    port_builder!(mono_in,  mono_in_multi,  inputs,  crate::cables::CableKind::Mono);
    port_builder!(poly_in,  poly_in_multi,  inputs,  crate::cables::CableKind::Poly);
    port_builder!(mono_out, mono_out_multi, outputs, crate::cables::CableKind::Mono);
    port_builder!(poly_out, poly_out_multi, outputs, crate::cables::CableKind::Poly);

    // ── Parameter builder methods (generated) ───────────────────────────────

    param_builder!(float_param, float_param_multi,
        (min: f32, max: f32, default: f32),
        ParameterKind::Float { min, max, default });

    param_builder!(int_param, int_param_multi,
        (min: i64, max: i64, default: i64),
        ParameterKind::Int { min, max, default });

    param_builder!(bool_param, bool_param_multi,
        (default: bool),
        ParameterKind::Bool { default });

    param_builder!(enum_param, enum_param_multi,
        (variants: &'static [&'static str], default: &'static str),
        ParameterKind::Enum { variants, default });

    param_builder!(string_param, string_param_multi,
        (default: &'static str),
        ParameterKind::String { default });

    param_builder!(file_param, file_param_multi,
        (extensions: &'static [&'static str]),
        ParameterKind::File { extensions });

    // array_param has no _multi sibling so it is written by hand.
    pub fn array_param(
        mut self,
        name: &'static str,
        default: &'static [&'static str],
        length: usize,
    ) -> Self {
        self.parameters.push(ParameterDescriptor {
            name,
            index: 0,
            parameter_type: ParameterKind::Array { default, length },
        });
        self
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cables::CableKind;

    #[test]
    fn build_a_module_descriptor() {
        let gain_amount = ParameterKind::Float { min: 0.0, max: 1.2, default: 1.0 };
        let pan_amount = ParameterKind::Float { min: -1.0, max: 1.0, default: 0.0 };
        let toggle_off_on = ParameterKind::Bool { default: false };

        let m = ModuleDescriptor {
            module_name: "Mixer",
            shape: ModuleShape { channels: 2, length: 0, ..Default::default() },
            inputs: vec![
                PortDescriptor { name: "in", index: 0, kind: CableKind::Mono },
                PortDescriptor { name: "in", index: 1, kind: CableKind::Mono },
                PortDescriptor { name: "gain_mod", index: 0, kind: CableKind::Mono },
                PortDescriptor { name: "gain_mod", index: 1, kind: CableKind::Mono },
                PortDescriptor { name: "pan_mod", index: 0, kind: CableKind::Mono },
                PortDescriptor { name: "pan_mod", index: 1, kind: CableKind::Mono },
            ],
            outputs: vec![
                PortDescriptor { name: "out_l", index: 0, kind: CableKind::Mono },
                PortDescriptor { name: "out_r", index: 0, kind: CableKind::Mono },
            ],
            parameters: vec![
                ParameterDescriptor { name: "gain", index: 0, parameter_type: gain_amount.clone() },
                ParameterDescriptor { name: "gain", index: 1, parameter_type: gain_amount },
                ParameterDescriptor { name: "pan", index: 0, parameter_type: pan_amount.clone() },
                ParameterDescriptor { name: "pan", index: 1, parameter_type: pan_amount },
                ParameterDescriptor { name: "mute", index: 0, parameter_type: toggle_off_on.clone() },
                ParameterDescriptor { name: "mute", index: 1, parameter_type: toggle_off_on.clone() },
                ParameterDescriptor { name: "solo", index: 0, parameter_type: toggle_off_on.clone() },
                ParameterDescriptor { name: "solo", index: 1, parameter_type: toggle_off_on },
            ],
        };
        assert_eq!(m.module_name, "Mixer");
        assert_eq!(m.shape.channels, 2);
        assert_eq!(m.shape.length, 0);
        assert_eq!(m.inputs.len(), 6);
        assert_eq!(m.outputs.len(), 2);
        assert_eq!(m.parameters.len(), 8);
    }

    #[test]
    fn builder_single_port_methods() {
        let m = ModuleDescriptor::new("Vca", ModuleShape { channels: 1, length: 0, ..Default::default() })
            .mono_in("in")
            .mono_in("cv")
            .mono_out("out")
            .poly_in("poly_in")
            .poly_out("poly_out");

        assert_eq!(m.inputs.len(), 3);
        assert_eq!(m.inputs[0].name, "in");
        assert_eq!(m.inputs[0].index, 0);
        assert_eq!(m.inputs[0].kind, CableKind::Mono);
        assert_eq!(m.inputs[1].name, "cv");
        assert_eq!(m.inputs[1].index, 0);
        assert_eq!(m.inputs[1].kind, CableKind::Mono);
        assert_eq!(m.inputs[2].name, "poly_in");
        assert_eq!(m.inputs[2].index, 0);
        assert_eq!(m.inputs[2].kind, CableKind::Poly);

        assert_eq!(m.outputs.len(), 2);
        assert_eq!(m.outputs[0].name, "out");
        assert_eq!(m.outputs[0].index, 0);
        assert_eq!(m.outputs[0].kind, CableKind::Mono);
        assert_eq!(m.outputs[1].name, "poly_out");
        assert_eq!(m.outputs[1].index, 0);
        assert_eq!(m.outputs[1].kind, CableKind::Poly);

    }

    #[test]
    fn builder_multi_port_methods_count_3() {
        let m = ModuleDescriptor::new("Mixer", ModuleShape { channels: 3, length: 0, ..Default::default() })
            .mono_in_multi("in", 3)
            .poly_out_multi("out", 3);

        assert_eq!(m.inputs.len(), 3);
        for i in 0..3usize {
            assert_eq!(m.inputs[i].name, "in");
            assert_eq!(m.inputs[i].index, i);
            assert_eq!(m.inputs[i].kind, CableKind::Mono);
        }

        assert_eq!(m.outputs.len(), 3);
        for i in 0..3usize {
            assert_eq!(m.outputs[i].name, "out");
            assert_eq!(m.outputs[i].index, i);
            assert_eq!(m.outputs[i].kind, CableKind::Poly);
        }
    }

    #[test]
    fn builder_parameter_methods() {
        let m = ModuleDescriptor::new("Synth", ModuleShape { channels: 1, length: 0, ..Default::default() })
            .float_param("gain", 0.0, 1.0, 0.5)
            .int_param("voices", 1, 8, 4)
            .bool_param("active", true)
            .enum_param("wave", &["sine", "saw", "square"], "sine")
            .array_param("pattern", &["C4", "E4"], 16);

        assert_eq!(m.parameters.len(), 5);

        let p = &m.parameters[0];
        assert_eq!(p.name, "gain");
        assert_eq!(p.index, 0);
        assert!(matches!(p.parameter_type, ParameterKind::Float { min, max, default } if min == 0.0 && max == 1.0 && default == 0.5));

        let p = &m.parameters[1];
        assert_eq!(p.name, "voices");
        assert_eq!(p.index, 0);
        assert!(matches!(p.parameter_type, ParameterKind::Int { min, max, default } if min == 1 && max == 8 && default == 4));

        let p = &m.parameters[2];
        assert_eq!(p.name, "active");
        assert_eq!(p.index, 0);
        assert!(matches!(p.parameter_type, ParameterKind::Bool { default } if default));

        let p = &m.parameters[3];
        assert_eq!(p.name, "wave");
        assert_eq!(p.index, 0);
        assert!(matches!(p.parameter_type, ParameterKind::Enum { default, .. } if default == "sine"));

        let p = &m.parameters[4];
        assert_eq!(p.name, "pattern");
        assert_eq!(p.index, 0);
        assert!(matches!(p.parameter_type, ParameterKind::Array { length, .. } if length == 16));
    }

    #[test]
    fn builder_multi_parameter_methods() {
        let m = ModuleDescriptor::new("Mixer", ModuleShape { channels: 3, length: 0, ..Default::default() })
            .float_param_multi("gain", 3, 0.0, 1.2, 1.0)
            .int_param_multi("steps", 3, 1, 32, 16)
            .bool_param_multi("mute", 3, false)
            .enum_param_multi("wave", 3, &["sine", "saw"], "sine");

        assert_eq!(m.parameters.len(), 12);

        // Check indices for first group
        for i in 0..3 {
            assert_eq!(m.parameters[i].name, "gain");
            assert_eq!(m.parameters[i].index, i);
        }
        for i in 0..3 {
            assert_eq!(m.parameters[3 + i].name, "steps");
            assert_eq!(m.parameters[3 + i].index, i);
        }
        for i in 0..3 {
            assert_eq!(m.parameters[6 + i].name, "mute");
            assert_eq!(m.parameters[6 + i].index, i);
        }
        for i in 0..3 {
            assert_eq!(m.parameters[9 + i].name, "wave");
            assert_eq!(m.parameters[9 + i].index, i);
        }
    }

}
