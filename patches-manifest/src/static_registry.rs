//! Build a runtime [`Registry`] from a [`ModuleManifest`] (ADR 0066,
//! ticket 0793).
//!
//! Bridging owned manifest types (`String`, `Vec`) into the runtime's
//! `&'static`-backed [`ModuleDescriptorTemplate`] requires interning —
//! we [`Box::leak`] once per template-load. Total leak is bounded
//! (~50 modules × O(10) strings each, kilobytes) and lasts the lifetime
//! of the host process.
//!
//! The resulting registry is **describe-only**: every entry's
//! `ModuleBuilder::build` returns [`BuildError::UnknownModule`]. This is
//! the registry shape the LSP and other "shape-only" consumers need;
//! actual audio-engine instantiation goes through
//! `patches_modules::default_registry()` in the audio crate.

use patches_core::cables::{CableKind, MonoLayout, PolyLayout};
use patches_core::modules::ModuleDescriptorTemplate;
use patches_core::{
    AudioEnvironment, AxisId, BuildError, CountAxis, InstanceId, Module, ModuleShape,
    ParameterKind, ParameterMap, ParameterTemplate, PortTemplate, StructuralParams,
};
use patches_core::registry::{ModuleBuilder, Registry};

use crate::{
    ModuleManifest, OwnedAxisId, OwnedCableKind, OwnedMonoLayout, OwnedParam, OwnedParamKind,
    OwnedPolyLayout, OwnedPort, OwnedTemplate,
};

/// Build a [`Registry`] populated from `manifest`. Each entry registers
/// a describe-only builder; `build()` always returns
/// [`BuildError::UnknownModule`].
pub fn registry_from_manifest(manifest: &ModuleManifest) -> Registry {
    let mut registry = Registry::new();
    for entry in &manifest.modules {
        let template = static_template_from_owned(&entry.template);
        registry.register_builder(entry.name.clone(), Box::new(DescribeOnlyBuilder { template }));
    }
    registry
}

struct DescribeOnlyBuilder {
    template: ModuleDescriptorTemplate,
}

impl ModuleBuilder for DescribeOnlyBuilder {
    fn template(&self) -> ModuleDescriptorTemplate {
        self.template
    }

    fn build(
        &self,
        _audio_environment: &AudioEnvironment,
        _shape: &ModuleShape,
        _params: &ParameterMap,
        _structural: &StructuralParams,
        _instance_id: InstanceId,
    ) -> Result<Box<dyn Module>, BuildError> {
        Err(BuildError::UnknownModule {
            name: self.template.name.to_string(),
            origin: None,
        })
    }
}

// ── Owned → static template (interns via Box::leak) ─────────────────────

fn intern_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn intern_str_slice(items: &[String]) -> &'static [&'static str] {
    let v: Vec<&'static str> = items.iter().map(|s| intern_str(s)).collect();
    Box::leak(v.into_boxed_slice())
}

fn intern<T>(v: Vec<T>) -> &'static [T] {
    Box::leak(v.into_boxed_slice())
}

fn cable_kind(k: OwnedCableKind) -> CableKind {
    match k {
        OwnedCableKind::Mono => CableKind::Mono,
        OwnedCableKind::Poly => CableKind::Poly,
        OwnedCableKind::Stereo => CableKind::Stereo,
    }
}

fn mono_layout(l: OwnedMonoLayout) -> MonoLayout {
    match l {
        OwnedMonoLayout::Audio => MonoLayout::Audio,
        OwnedMonoLayout::Trigger => MonoLayout::Trigger,
    }
}

fn poly_layout(l: OwnedPolyLayout) -> PolyLayout {
    match l {
        OwnedPolyLayout::Audio => PolyLayout::Audio,
        OwnedPolyLayout::Trigger => PolyLayout::Trigger,
        OwnedPolyLayout::Transport => PolyLayout::Transport,
        OwnedPolyLayout::Midi => PolyLayout::Midi,
    }
}

fn port_template(p: &OwnedPort) -> PortTemplate {
    PortTemplate {
        name: intern_str(&p.name),
        kind: cable_kind(p.kind),
        mono_layout: mono_layout(p.mono_layout),
        poly_layout: poly_layout(p.poly_layout),
    }
}

fn parameter_kind(k: &OwnedParamKind) -> ParameterKind {
    match k {
        OwnedParamKind::Float { min, max, default } => ParameterKind::Float {
            min: *min,
            max: *max,
            default: *default,
        },
        OwnedParamKind::Int { min, max, default } => ParameterKind::Int {
            min: *min,
            max: *max,
            default: *default,
        },
        OwnedParamKind::Bool { default } => ParameterKind::Bool { default: *default },
        OwnedParamKind::Enum { variants, default } => ParameterKind::Enum {
            variants: intern_str_slice(variants),
            default: intern_str(default),
        },
        OwnedParamKind::File { extensions } => ParameterKind::File {
            extensions: intern_str_slice(extensions),
        },
        OwnedParamKind::SongName => ParameterKind::SongName,
    }
}

fn parameter_template(p: &OwnedParam) -> ParameterTemplate {
    ParameterTemplate {
        name: intern_str(&p.name),
        kind: parameter_kind(&p.kind),
    }
}

fn axis_id(id: &OwnedAxisId) -> AxisId {
    if id.0 == "channels" {
        AxisId::CHANNELS
    } else {
        AxisId(intern_str(&id.0))
    }
}

fn count_axis(id: &OwnedAxisId, name: &str) -> CountAxis {
    if id.0 == "channels" && name == "channels" {
        CountAxis::CHANNELS
    } else {
        CountAxis {
            id: axis_id(id),
            name: intern_str(name),
        }
    }
}

fn static_template_from_owned(t: &OwnedTemplate) -> ModuleDescriptorTemplate {
    let axes: Vec<CountAxis> = t.axes.iter().map(|a| count_axis(&a.id, &a.name)).collect();
    let global_inputs: Vec<PortTemplate> = t.global_inputs.iter().map(port_template).collect();
    let per_axis_inputs: Vec<(AxisId, PortTemplate)> = t
        .per_axis_inputs
        .iter()
        .map(|(a, p)| (axis_id(a), port_template(p)))
        .collect();
    let global_outputs: Vec<PortTemplate> = t.global_outputs.iter().map(port_template).collect();
    let per_axis_outputs: Vec<(AxisId, PortTemplate)> = t
        .per_axis_outputs
        .iter()
        .map(|(a, p)| (axis_id(a), port_template(p)))
        .collect();
    let realtime_params: Vec<ParameterTemplate> =
        t.realtime_params.iter().map(parameter_template).collect();
    let structural_params: Vec<ParameterTemplate> =
        t.structural_params.iter().map(parameter_template).collect();
    let per_axis_realtime_params: Vec<(AxisId, ParameterTemplate)> = t
        .per_axis_realtime_params
        .iter()
        .map(|(a, p)| (axis_id(a), parameter_template(p)))
        .collect();
    let per_axis_structural_params: Vec<(AxisId, ParameterTemplate)> = t
        .per_axis_structural_params
        .iter()
        .map(|(a, p)| (axis_id(a), parameter_template(p)))
        .collect();

    ModuleDescriptorTemplate {
        name: intern_str(&t.name),
        axes: intern(axes),
        global_inputs: intern(global_inputs),
        per_axis_inputs: intern(per_axis_inputs),
        global_outputs: intern(global_outputs),
        per_axis_outputs: intern(per_axis_outputs),
        realtime_params: intern(realtime_params),
        structural_params: intern(structural_params),
        per_axis_realtime_params: intern(per_axis_realtime_params),
        per_axis_structural_params: intern(per_axis_structural_params),
    }
}
