//! Compile-time-constant module descriptor templates (ADR 0066).
//!
//! A `ModuleDescriptorTemplate` is a static, allocation-free description of a
//! module type's port and parameter layout, parameterized by one or more
//! count axes. Today exactly one axis is surfaced (`channels`); the
//! multi-axis machinery is forward-looking insurance against future shape
//! dimensions (matrix mixers, multi-tap delays).
//!
//! Templates are converted to runtime [`ModuleDescriptor`] values via
//! [`ModuleDescriptorTemplate::build_channels`] (single-axis convenience) or
//! [`ModuleDescriptorTemplate::build`] (multi-axis).
//!
//! Two flavours of port/parameter declaration:
//!
//! - **Global** entries appear once in the built descriptor, with `index = 0`.
//! - **Per-axis** entries are paired with an [`AxisId`]; when the axis has
//!   count `N`, the template emits `N` copies with indices `0..N`.

use crate::cables::{CableKind, MonoLayout, PolyLayout};
use super::module_descriptor::{
    ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind, PortDescriptor,
};

/// Identifies a count axis. Today the only surfaced axis is
/// [`AxisId::CHANNELS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisId(pub &'static str);

impl AxisId {
    pub const CHANNELS: AxisId = AxisId("channels");
}

/// A named count dimension. Templates declare zero or more axes; callers of
/// [`ModuleDescriptorTemplate::build`] supply a count for each.
#[derive(Debug, Clone, Copy)]
pub struct CountAxis {
    pub id: AxisId,
    pub name: &'static str,
}

impl CountAxis {
    pub const CHANNELS: CountAxis = CountAxis {
        id: AxisId::CHANNELS,
        name: "channels",
    };
}

/// Static port template. Mirrors [`PortDescriptor`] without `index` (the
/// index is filled in at build time from the port's position within its
/// axis fan-out).
#[derive(Debug, Clone)]
pub struct PortTemplate {
    pub name: &'static str,
    pub kind: CableKind,
    pub mono_layout: MonoLayout,
    pub poly_layout: PolyLayout,
}

impl PortTemplate {
    pub const fn mono(name: &'static str) -> Self {
        Self { name, kind: CableKind::Mono, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }
    }
    pub const fn poly(name: &'static str) -> Self {
        Self { name, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }
    }
    pub const fn stereo(name: &'static str) -> Self {
        Self { name, kind: CableKind::Stereo, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Audio }
    }
    pub const fn trigger(name: &'static str) -> Self {
        Self { name, kind: CableKind::Mono, mono_layout: MonoLayout::Trigger, poly_layout: PolyLayout::Audio }
    }
    pub const fn poly_trigger(name: &'static str) -> Self {
        Self { name, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Trigger }
    }
    pub const fn midi(name: &'static str) -> Self {
        Self { name, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout: PolyLayout::Midi }
    }
    pub const fn poly_with_layout(name: &'static str, layout: PolyLayout) -> Self {
        Self { name, kind: CableKind::Poly, mono_layout: MonoLayout::Audio, poly_layout: layout }
    }

    fn at(&self, index: usize) -> PortDescriptor {
        PortDescriptor {
            name: self.name,
            index,
            kind: self.kind.clone(),
            mono_layout: self.mono_layout,
            poly_layout: self.poly_layout,
        }
    }
}

/// Static parameter template. Mirrors [`ParameterDescriptor`] without `index`.
#[derive(Debug, Clone)]
pub struct ParameterTemplate {
    pub name: &'static str,
    pub kind: ParameterKind,
}

impl ParameterTemplate {
    fn at(&self, index: usize) -> ParameterDescriptor {
        ParameterDescriptor {
            name: self.name,
            index,
            parameter_type: self.kind.clone(),
        }
    }
}

/// Compile-time-constant template for a [`ModuleDescriptor`].
///
/// Declarative replacement for the imperative `describe(shape)` builder
/// chain. Each module type owns one of these as a `const`; the runtime
/// descriptor is built per-instance with [`build`](Self::build) /
/// [`build_channels`](Self::build_channels).
#[derive(Debug, Clone, Copy)]
pub struct ModuleDescriptorTemplate {
    pub name: &'static str,
    pub axes: &'static [CountAxis],
    pub global_inputs: &'static [PortTemplate],
    pub per_axis_inputs: &'static [(AxisId, PortTemplate)],
    pub global_outputs: &'static [PortTemplate],
    pub per_axis_outputs: &'static [(AxisId, PortTemplate)],
    pub realtime_params: &'static [ParameterTemplate],
    pub structural_params: &'static [ParameterTemplate],
    pub per_axis_realtime_params: &'static [(AxisId, ParameterTemplate)],
    pub per_axis_structural_params: &'static [(AxisId, ParameterTemplate)],
}

impl ModuleDescriptorTemplate {
    pub const EMPTY: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
        name: "",
        axes: &[],
        global_inputs: &[],
        per_axis_inputs: &[],
        global_outputs: &[],
        per_axis_outputs: &[],
        realtime_params: &[],
        structural_params: &[],
        per_axis_realtime_params: &[],
        per_axis_structural_params: &[],
    };

    fn axis_count(&self, counts: &[(AxisId, u32)], axis: AxisId) -> u32 {
        counts
            .iter()
            .find_map(|(id, n)| if *id == axis { Some(*n) } else { None })
            .unwrap_or_else(|| {
                panic!(
                    "ModuleDescriptorTemplate {:?}: missing count for axis {:?}",
                    self.name, axis
                )
            })
    }

    /// Build a runtime [`ModuleDescriptor`] from this template plus a count
    /// for each axis declared in `axes`.
    pub fn build(&self, counts: &[(AxisId, u32)]) -> ModuleDescriptor {
        let mut inputs = Vec::with_capacity(self.global_inputs.len() + self.per_axis_inputs.len());
        for p in self.global_inputs {
            inputs.push(p.at(0));
        }
        for (axis, p) in self.per_axis_inputs {
            let n = self.axis_count(counts, *axis);
            for i in 0..n as usize {
                inputs.push(p.at(i));
            }
        }

        let mut outputs = Vec::with_capacity(self.global_outputs.len() + self.per_axis_outputs.len());
        for p in self.global_outputs {
            outputs.push(p.at(0));
        }
        for (axis, p) in self.per_axis_outputs {
            let n = self.axis_count(counts, *axis);
            for i in 0..n as usize {
                outputs.push(p.at(i));
            }
        }

        let mut realtime_params = Vec::with_capacity(
            self.realtime_params.len() + self.per_axis_realtime_params.len(),
        );
        for p in self.realtime_params {
            realtime_params.push(p.at(0));
        }
        for (axis, p) in self.per_axis_realtime_params {
            let n = self.axis_count(counts, *axis);
            for i in 0..n as usize {
                realtime_params.push(p.at(i));
            }
        }

        let mut structural_params = Vec::with_capacity(
            self.structural_params.len() + self.per_axis_structural_params.len(),
        );
        for p in self.structural_params {
            structural_params.push(p.at(0));
        }
        for (axis, p) in self.per_axis_structural_params {
            let n = self.axis_count(counts, *axis);
            for i in 0..n as usize {
                structural_params.push(p.at(i));
            }
        }

        let channels = counts
            .iter()
            .find_map(|(id, n)| if *id == AxisId::CHANNELS { Some(*n as usize) } else { None })
            .unwrap_or(1);

        ModuleDescriptor {
            module_name: self.name,
            shape: ModuleShape { channels },
            inputs,
            outputs,
            realtime_params,
            structural_params,
        }
    }

    /// Single-axis convenience: build with `channels = n` and no other axes.
    /// Panics if the template declares any axis other than
    /// [`AxisId::CHANNELS`].
    pub fn build_channels(&self, channels: u32) -> ModuleDescriptor {
        for axis in self.axes {
            assert!(
                axis.id == AxisId::CHANNELS,
                "build_channels called on template {:?} with non-channels axis {:?}",
                self.name,
                axis.id,
            );
        }
        self.build(&[(AxisId::CHANNELS, channels)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic Mixer-shaped template: 1 mono input per channel ("in"),
    // 1 mono output per channel ("out"), 1 global mono "cv" input,
    // a per-channel float "gain" param, and a global structural "label".
    const MIXER_TEMPLATE: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
        name: "Mixer",
        axes: &[CountAxis::CHANNELS],
        global_inputs: &[PortTemplate::mono("cv")],
        per_axis_inputs: &[(AxisId::CHANNELS, PortTemplate::mono("in"))],
        global_outputs: &[],
        per_axis_outputs: &[(AxisId::CHANNELS, PortTemplate::mono("out"))],
        realtime_params: &[],
        structural_params: &[ParameterTemplate {
            name: "label",
            kind: ParameterKind::Bool { default: false },
        }],
        per_axis_realtime_params: &[(
            AxisId::CHANNELS,
            ParameterTemplate {
                name: "gain",
                kind: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
            },
        )],
        per_axis_structural_params: &[],
    };

    fn manual_mixer(channels: usize) -> ModuleDescriptor {
        let mut inputs = vec![PortDescriptor {
            name: "cv",
            index: 0,
            kind: CableKind::Mono,
            mono_layout: MonoLayout::Audio,
            poly_layout: PolyLayout::Audio,
        }];
        for i in 0..channels {
            inputs.push(PortDescriptor {
                name: "in",
                index: i,
                kind: CableKind::Mono,
                mono_layout: MonoLayout::Audio,
                poly_layout: PolyLayout::Audio,
            });
        }
        let mut outputs = Vec::new();
        for i in 0..channels {
            outputs.push(PortDescriptor {
                name: "out",
                index: i,
                kind: CableKind::Mono,
                mono_layout: MonoLayout::Audio,
                poly_layout: PolyLayout::Audio,
            });
        }
        let mut realtime_params = Vec::new();
        for i in 0..channels {
            realtime_params.push(ParameterDescriptor {
                name: "gain",
                index: i,
                parameter_type: ParameterKind::Float { min: 0.0, max: 1.0, default: 1.0 },
            });
        }
        let structural_params = vec![ParameterDescriptor {
            name: "label",
            index: 0,
            parameter_type: ParameterKind::Bool { default: false },
        }];
        ModuleDescriptor {
            module_name: "Mixer",
            shape: ModuleShape { channels },
            inputs,
            outputs,
            realtime_params,
            structural_params,
        }
    }

    fn assert_descriptors_equivalent(a: &ModuleDescriptor, b: &ModuleDescriptor) {
        assert_eq!(a.module_name, b.module_name);
        assert_eq!(a.shape, b.shape);
        assert_eq!(a.inputs.len(), b.inputs.len());
        for (x, y) in a.inputs.iter().zip(b.inputs.iter()) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.index, y.index);
            assert_eq!(x.kind, y.kind);
            assert_eq!(x.mono_layout, y.mono_layout);
            assert_eq!(x.poly_layout, y.poly_layout);
        }
        assert_eq!(a.outputs.len(), b.outputs.len());
        for (x, y) in a.outputs.iter().zip(b.outputs.iter()) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.index, y.index);
            assert_eq!(x.kind, y.kind);
        }
        assert_eq!(a.realtime_params.len(), b.realtime_params.len());
        for (x, y) in a.realtime_params.iter().zip(b.realtime_params.iter()) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.index, y.index);
        }
        assert_eq!(a.structural_params.len(), b.structural_params.len());
        for (x, y) in a.structural_params.iter().zip(b.structural_params.iter()) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.index, y.index);
        }
    }

    #[test]
    fn build_channels_matches_manual_for_channels_1() {
        let built = MIXER_TEMPLATE.build_channels(1);
        let manual = manual_mixer(1);
        assert_descriptors_equivalent(&built, &manual);
    }

    #[test]
    fn build_channels_matches_manual_for_channels_4() {
        let built = MIXER_TEMPLATE.build_channels(4);
        let manual = manual_mixer(4);
        assert_descriptors_equivalent(&built, &manual);
    }

    #[test]
    fn build_multi_axis_equivalent_to_build_channels() {
        let a = MIXER_TEMPLATE.build(&[(AxisId::CHANNELS, 3)]);
        let b = MIXER_TEMPLATE.build_channels(3);
        assert_descriptors_equivalent(&a, &b);
    }

    #[test]
    #[should_panic(expected = "missing count for axis")]
    fn build_panics_on_missing_axis() {
        let _ = MIXER_TEMPLATE.build(&[]);
    }
}
