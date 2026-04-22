use super::parameter_map::ParameterValue;
use crate::cables::PolyLayout;

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
///
/// `poly_layout` declares the structured frame format for poly ports (ADR 0033).
/// Mono ports always have `PolyLayout::Audio` (ignored by validation). Poly
/// ports default to `Audio` (untyped) unless explicitly tagged.
#[derive(Debug, Clone)]
pub struct PortDescriptor {
    pub name: &'static str,
    pub index: usize,
    pub kind: crate::cables::CableKind,
    pub poly_layout: PolyLayout,
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
    /// A song name parameter. The DSL writes a string (`song: "my_song"`)
    /// which the interpreter resolves to an integer index into the
    /// alphabetically-sorted song bank. The module receives a
    /// `ParameterValue::Int` (−1 if unresolved/empty).
    SongName,
}

impl ParameterKind {
    /// Return the default value for this parameter kind as a [`ParameterValue`].
    pub fn default_value(&self) -> ParameterValue {
        match self {
            ParameterKind::Float { default, .. } => ParameterValue::Float(*default),
            ParameterKind::Int   { default, .. } => ParameterValue::Int(*default),
            ParameterKind::Bool  { default }     => ParameterValue::Bool(*default),
            ParameterKind::Enum  { variants, default } => {
                let idx = variants
                    .iter()
                    .position(|v| v == default)
                    .unwrap_or(0) as u32;
                ParameterValue::Enum(idx)
            }
            ParameterKind::File { .. } => ParameterValue::File(String::new()),
            ParameterKind::SongName => ParameterValue::Int(-1),
        }
    }

    /// Return a short type name suitable for error messages.
    pub fn kind_name(&self) -> &'static str {
        match self {
            ParameterKind::Float { .. } => "float",
            ParameterKind::Int   { .. } => "int",
            ParameterKind::Bool  { .. } => "bool",
            ParameterKind::Enum   { .. } => "enum",
            ParameterKind::File  { .. } => "file",
            ParameterKind::SongName => "song_name",
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
            self.$vec.push(PortDescriptor { name, index: 0, kind: $kind, poly_layout: PolyLayout::Audio });
            self
        }

        pub fn $multi(mut self, name: &'static str, count: usize) -> Self {
            for i in 0..count {
                self.$vec.push(PortDescriptor { name, index: i, kind: $kind, poly_layout: PolyLayout::Audio });
            }
            self
        }
    };
}

// Generates a pair of builder methods for a single parameter kind:
//   $single(name, $args…) — appends one ParameterDescriptor with index 0
//   $multi(name, count, $args…) — appends `count` ParameterDescriptors with indices 0..count
macro_rules! param_builder {
    ($single:ident, $multi:ident, $NameTy:ty, $ArrayTy:ty, ($($arg:ident : $ty:ty),*), $kind:expr) => {
        pub fn $single(mut self, name: impl Into<$NameTy>, $($arg: $ty),*) -> Self {
            let name: $NameTy = name.into();
            self.parameters.push(ParameterDescriptor {
                name: name.as_str(),
                index: 0,
                parameter_type: $kind,
            });
            self
        }

        pub fn $multi(mut self, name: impl Into<$ArrayTy>, count: usize, $($arg: $ty),*) -> Self {
            let name: $ArrayTy = name.into();
            for i in 0..count {
                self.parameters.push(ParameterDescriptor {
                    name: name.as_str(),
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

    port_builder!(mono_in,         mono_in_multi,         inputs,  crate::cables::CableKind::Mono);
    port_builder!(poly_in,         poly_in_multi,         inputs,  crate::cables::CableKind::Poly);
    port_builder!(mono_out,        mono_out_multi,        outputs, crate::cables::CableKind::Mono);
    port_builder!(poly_out,        poly_out_multi,        outputs, crate::cables::CableKind::Poly);
    port_builder!(trigger_in,      trigger_in_multi,      inputs,  crate::cables::CableKind::Trigger);
    port_builder!(trigger_out,     trigger_out_multi,     outputs, crate::cables::CableKind::Trigger);
    port_builder!(poly_trigger_in, poly_trigger_in_multi, inputs,  crate::cables::CableKind::PolyTrigger);
    port_builder!(poly_trigger_out,poly_trigger_out_multi,outputs, crate::cables::CableKind::PolyTrigger);

    /// Declare a poly input port with a specific [`PolyLayout`].
    pub fn poly_in_layout(mut self, name: &'static str, layout: PolyLayout) -> Self {
        self.inputs.push(PortDescriptor {
            name, index: 0, kind: crate::cables::CableKind::Poly, poly_layout: layout,
        });
        self
    }

    /// Declare a poly output port with a specific [`PolyLayout`].
    pub fn poly_out_layout(mut self, name: &'static str, layout: PolyLayout) -> Self {
        self.outputs.push(PortDescriptor {
            name, index: 0, kind: crate::cables::CableKind::Poly, poly_layout: layout,
        });
        self
    }

    // ── Parameter builder methods (generated) ───────────────────────────────

    param_builder!(float_param, float_param_multi,
        crate::params::FloatParamName, crate::params::FloatParamArray,
        (min: f32, max: f32, default: f32),
        ParameterKind::Float { min, max, default });

    param_builder!(int_param, int_param_multi,
        crate::params::IntParamName, crate::params::IntParamArray,
        (min: i64, max: i64, default: i64),
        ParameterKind::Int { min, max, default });

    param_builder!(bool_param, bool_param_multi,
        crate::params::BoolParamName, crate::params::BoolParamArray,
        (default: bool),
        ParameterKind::Bool { default });

    /// Typed enum parameter. Variants come from `E::VARIANTS`; default value
    /// is supplied as a Rust enum value.
    pub fn enum_param<E: crate::params::ParamEnum>(
        mut self,
        name: impl Into<crate::params::EnumParamName<E>>,
        default: E,
    ) -> Self {
        let name: crate::params::EnumParamName<E> = name.into();
        let variants = E::VARIANTS;
        let default_s = variants[default.to_variant() as usize];
        self.parameters.push(ParameterDescriptor {
            name: name.as_str(),
            index: 0,
            parameter_type: ParameterKind::Enum { variants, default: default_s },
        });
        self
    }

    pub fn enum_param_multi<E: crate::params::ParamEnum>(
        mut self,
        name: impl Into<crate::params::EnumParamArray<E>>,
        count: usize,
        default: E,
    ) -> Self {
        let name: crate::params::EnumParamArray<E> = name.into();
        let variants = E::VARIANTS;
        let default_s = variants[default.to_variant() as usize];
        for i in 0..count {
            self.parameters.push(ParameterDescriptor {
                name: name.as_str(),
                index: i,
                parameter_type: ParameterKind::Enum { variants, default: default_s },
            });
        }
        self
    }

    /// Legacy string-typed file parameter (ADR 0046: no `ParamView::get` site,
    /// resolved off-thread into a buffer slot).
    pub fn file_param(
        mut self,
        name: &'static str,
        extensions: &'static [&'static str],
    ) -> Self {
        self.parameters.push(ParameterDescriptor {
            name,
            index: 0,
            parameter_type: ParameterKind::File { extensions },
        });
        self
    }

    pub fn file_param_multi(
        mut self,
        name: &'static str,
        count: usize,
        extensions: &'static [&'static str],
    ) -> Self {
        for i in 0..count {
            self.parameters.push(ParameterDescriptor {
                name,
                index: i,
                parameter_type: ParameterKind::File { extensions },
            });
        }
        self
    }

    /// Declare a song-name parameter. The DSL supplies a string; the
    /// interpreter resolves it to a `ParameterValue::Int` song-bank index.
    pub fn song_name_param(mut self, name: impl Into<crate::params::SongNameParamName>) -> Self {
        self.parameters.push(ParameterDescriptor {
            name: name.into().as_str(),
            index: 0,
            parameter_type: ParameterKind::SongName,
        });
        self
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cables::CableKind;
    use crate::params::{EnumParamArray, EnumParamName};

    crate::params_enum! {
        pub enum Wave {
            Sine => "sine",
            Saw => "saw",
            Square => "square",
        }
    }

    crate::params_enum! {
        pub enum Wave2 {
            Sine => "sine",
            Saw => "saw",
        }
    }

    #[test]
    fn build_a_module_descriptor() {
        let gain_amount = ParameterKind::Float { min: 0.0, max: 1.2, default: 1.0 };
        let pan_amount = ParameterKind::Float { min: -1.0, max: 1.0, default: 0.0 };
        let toggle_off_on = ParameterKind::Bool { default: false };

        let m = ModuleDescriptor {
            module_name: "Mixer",
            shape: ModuleShape { channels: 2, length: 0, ..Default::default() },
            inputs: vec![
                PortDescriptor { name: "in", index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "in", index: 1, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "gain_mod", index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "gain_mod", index: 1, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "pan_mod", index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "pan_mod", index: 1, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
            ],
            outputs: vec![
                PortDescriptor { name: "out_l", index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
                PortDescriptor { name: "out_r", index: 0, kind: CableKind::Mono, poly_layout: PolyLayout::Audio },
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
            .enum_param(EnumParamName::<Wave>::new("wave"), Wave::Sine);

        assert_eq!(m.parameters.len(), 4);

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
    }

    #[test]
    fn builder_multi_parameter_methods() {
        let m = ModuleDescriptor::new("Mixer", ModuleShape { channels: 3, length: 0, ..Default::default() })
            .float_param_multi("gain", 3, 0.0, 1.2, 1.0)
            .int_param_multi("steps", 3, 1, 32, 16)
            .bool_param_multi("mute", 3, false)
            .enum_param_multi(EnumParamArray::<Wave2>::new("wave"), 3, Wave2::Sine);

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
