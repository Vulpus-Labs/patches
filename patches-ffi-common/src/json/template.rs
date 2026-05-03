//! Hand-rolled JSON serialization for [`ModuleDescriptorTemplate`] crossing
//! the FFI boundary at plugin load (ADR 0066, ticket 0795).
//!
//! Mirrors `module_descriptor` JSON: the deserializer leaks strings and
//! slices via `Box::leak` so the resulting `ModuleDescriptorTemplate`
//! carries `&'static` pointers compatible with the static templates
//! declared by built-in modules. One leak per loaded plugin per process.

use patches_core::cables::CableKind;
use patches_core::modules::descriptor_template::{
    AxisId, CountAxis, ModuleDescriptorTemplate, ParameterTemplate, PortTemplate,
};
use patches_core::ParameterKind;

use super::ser::{write_f32, write_string};

// ── serialize ────────────────────────────────────────────────────────────────

pub fn serialize_module_descriptor_template(t: &ModuleDescriptorTemplate) -> Vec<u8> {
    let mut out = String::with_capacity(512);
    out.push('{');

    out.push_str("\"name\":");
    write_string(&mut out, t.name);

    out.push_str(",\"axes\":[");
    for (i, axis) in t.axes.iter().enumerate() {
        if i > 0 { out.push(','); }
        write_string(&mut out, axis.name);
    }
    out.push(']');

    out.push_str(",\"global_inputs\":");
    write_port_array(&mut out, t.global_inputs);
    out.push_str(",\"per_axis_inputs\":");
    write_axis_port_array(&mut out, t.per_axis_inputs);
    out.push_str(",\"global_outputs\":");
    write_port_array(&mut out, t.global_outputs);
    out.push_str(",\"per_axis_outputs\":");
    write_axis_port_array(&mut out, t.per_axis_outputs);
    out.push_str(",\"realtime_params\":");
    write_param_array(&mut out, t.realtime_params);
    out.push_str(",\"structural_params\":");
    write_param_array(&mut out, t.structural_params);
    out.push_str(",\"per_axis_realtime_params\":");
    write_axis_param_array(&mut out, t.per_axis_realtime_params);
    out.push_str(",\"per_axis_structural_params\":");
    write_axis_param_array(&mut out, t.per_axis_structural_params);

    out.push('}');
    out.into_bytes()
}

fn write_port_array(out: &mut String, ports: &[PortTemplate]) {
    out.push('[');
    for (i, p) in ports.iter().enumerate() {
        if i > 0 { out.push(','); }
        write_port_template(out, p);
    }
    out.push(']');
}

fn write_axis_port_array(out: &mut String, ports: &[(AxisId, PortTemplate)]) {
    out.push('[');
    for (i, (axis, p)) in ports.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push_str("{\"axis\":");
        write_string(out, axis.0);
        out.push_str(",\"port\":");
        write_port_template(out, p);
        out.push('}');
    }
    out.push(']');
}

fn write_param_array(out: &mut String, params: &[ParameterTemplate]) {
    out.push('[');
    for (i, p) in params.iter().enumerate() {
        if i > 0 { out.push(','); }
        write_param_template(out, p);
    }
    out.push(']');
}

fn write_axis_param_array(out: &mut String, params: &[(AxisId, ParameterTemplate)]) {
    out.push('[');
    for (i, (axis, p)) in params.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push_str("{\"axis\":");
        write_string(out, axis.0);
        out.push_str(",\"param\":");
        write_param_template(out, p);
        out.push('}');
    }
    out.push(']');
}

fn write_port_template(out: &mut String, p: &PortTemplate) {
    out.push('{');
    out.push_str("\"name\":");
    write_string(out, p.name);
    out.push_str(",\"kind\":");
    match p.kind {
        CableKind::Mono => write_string(out, "mono"),
        CableKind::Poly => write_string(out, "poly"),
        CableKind::Stereo => write_string(out, "stereo"),
    }
    out.push_str(",\"mono_layout\":");
    match p.mono_layout {
        patches_core::MonoLayout::Audio => write_string(out, "audio"),
        patches_core::MonoLayout::Trigger => write_string(out, "trigger"),
    }
    out.push_str(",\"poly_layout\":");
    match p.poly_layout {
        patches_core::PolyLayout::Audio => write_string(out, "audio"),
        patches_core::PolyLayout::Trigger => write_string(out, "trigger"),
        patches_core::PolyLayout::Transport => write_string(out, "transport"),
        patches_core::PolyLayout::Midi => write_string(out, "midi"),
    }
    out.push('}');
}

fn write_param_template(out: &mut String, p: &ParameterTemplate) {
    out.push('{');
    out.push_str("\"name\":");
    write_string(out, p.name);
    out.push_str(",\"kind\":");
    write_param_kind(out, &p.kind);
    out.push('}');
}

fn write_param_kind(out: &mut String, kind: &ParameterKind) {
    match kind {
        ParameterKind::Float { min, max, default } => {
            out.push_str("{\"type\":\"float\",\"min\":");
            write_f32(out, *min);
            out.push_str(",\"max\":");
            write_f32(out, *max);
            out.push_str(",\"default\":");
            write_f32(out, *default);
            out.push('}');
        }
        ParameterKind::Int { min, max, default } => {
            out.push_str(&format!(
                "{{\"type\":\"int\",\"min\":{min},\"max\":{max},\"default\":{default}}}"
            ));
        }
        ParameterKind::Bool { default } => {
            out.push_str(&format!("{{\"type\":\"bool\",\"default\":{default}}}"));
        }
        ParameterKind::Enum { variants, default } => {
            out.push_str("{\"type\":\"enum\",\"variants\":[");
            for (i, v) in variants.iter().enumerate() {
                if i > 0 { out.push(','); }
                write_string(out, v);
            }
            out.push_str("],\"default\":");
            write_string(out, default);
            out.push('}');
        }
        ParameterKind::File { extensions } => {
            out.push_str("{\"type\":\"file\",\"extensions\":[");
            for (i, ext) in extensions.iter().enumerate() {
                if i > 0 { out.push(','); }
                write_string(out, ext);
            }
            out.push_str("]}");
        }
        ParameterKind::SongName => {
            out.push_str("{\"type\":\"song_name\"}");
        }
    }
}

// ── deserialize ──────────────────────────────────────────────────────────────

use super::de::parse_json_value;

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

pub fn deserialize_module_descriptor_template(
    data: &[u8],
) -> Result<ModuleDescriptorTemplate, String> {
    let root = parse_json_value(data)?;

    let name = leak_str(
        root.get_str("name").ok_or("missing name")?.to_string(),
    );

    let axes: Vec<CountAxis> = root
        .get_array("axes")
        .unwrap_or(&[])
        .iter()
        .filter_map(|v| v.as_str().map(|s| {
            let leaked = leak_str(s.to_string());
            CountAxis { id: AxisId(leaked), name: leaked }
        }))
        .collect();
    let axes: &'static [CountAxis] = Box::leak(axes.into_boxed_slice());

    let global_inputs = leak_ports(de_ports(root.get_array("global_inputs").unwrap_or(&[]))?);
    let per_axis_inputs = leak_axis_ports(de_axis_ports(root.get_array("per_axis_inputs").unwrap_or(&[]))?);
    let global_outputs = leak_ports(de_ports(root.get_array("global_outputs").unwrap_or(&[]))?);
    let per_axis_outputs = leak_axis_ports(de_axis_ports(root.get_array("per_axis_outputs").unwrap_or(&[]))?);
    let realtime_params = leak_params(de_params(root.get_array("realtime_params").unwrap_or(&[]))?);
    let structural_params = leak_params(de_params(root.get_array("structural_params").unwrap_or(&[]))?);
    let per_axis_realtime_params = leak_axis_params(de_axis_params(root.get_array("per_axis_realtime_params").unwrap_or(&[]))?);
    let per_axis_structural_params = leak_axis_params(de_axis_params(root.get_array("per_axis_structural_params").unwrap_or(&[]))?);

    Ok(ModuleDescriptorTemplate {
        name,
        axes,
        global_inputs,
        per_axis_inputs,
        global_outputs,
        per_axis_outputs,
        realtime_params,
        structural_params,
        per_axis_realtime_params,
        per_axis_structural_params,
    })
}

fn leak_ports(v: Vec<PortTemplate>) -> &'static [PortTemplate] {
    Box::leak(v.into_boxed_slice())
}
fn leak_axis_ports(v: Vec<(AxisId, PortTemplate)>) -> &'static [(AxisId, PortTemplate)] {
    Box::leak(v.into_boxed_slice())
}
fn leak_params(v: Vec<ParameterTemplate>) -> &'static [ParameterTemplate] {
    Box::leak(v.into_boxed_slice())
}
fn leak_axis_params(v: Vec<(AxisId, ParameterTemplate)>) -> &'static [(AxisId, ParameterTemplate)] {
    Box::leak(v.into_boxed_slice())
}

use super::de::JsonValue;

fn de_ports(arr: &[JsonValue]) -> Result<Vec<PortTemplate>, String> {
    arr.iter().map(de_port).collect()
}

fn de_axis_ports(arr: &[JsonValue]) -> Result<Vec<(AxisId, PortTemplate)>, String> {
    arr.iter()
        .map(|v| {
            let axis_name = leak_str(v.get_str("axis").ok_or("axis missing")?.to_string());
            let port = de_port(v.get_field("port").ok_or("port missing")?)?;
            Ok((AxisId(axis_name), port))
        })
        .collect()
}

fn de_params(arr: &[JsonValue]) -> Result<Vec<ParameterTemplate>, String> {
    arr.iter().map(de_param).collect()
}

fn de_axis_params(arr: &[JsonValue]) -> Result<Vec<(AxisId, ParameterTemplate)>, String> {
    arr.iter()
        .map(|v| {
            let axis_name = leak_str(v.get_str("axis").ok_or("axis missing")?.to_string());
            let p = de_param(v.get_field("param").ok_or("param missing")?)?;
            Ok((AxisId(axis_name), p))
        })
        .collect()
}

fn de_port(v: &JsonValue) -> Result<PortTemplate, String> {
    let name = leak_str(v.get_str("name").ok_or("port name missing")?.to_string());
    let kind = match v.get_str("kind") {
        Some("poly") => CableKind::Poly,
        Some("stereo") => CableKind::Stereo,
        _ => CableKind::Mono,
    };
    let mono_layout = match v.get_str("mono_layout") {
        Some("trigger") => patches_core::MonoLayout::Trigger,
        _ => patches_core::MonoLayout::Audio,
    };
    let poly_layout = match v.get_str("poly_layout") {
        Some("trigger") => patches_core::PolyLayout::Trigger,
        Some("transport") => patches_core::PolyLayout::Transport,
        Some("midi") => patches_core::PolyLayout::Midi,
        _ => patches_core::PolyLayout::Audio,
    };
    Ok(PortTemplate { name, kind, mono_layout, poly_layout })
}

fn de_param(v: &JsonValue) -> Result<ParameterTemplate, String> {
    let name = leak_str(v.get_str("name").ok_or("param name missing")?.to_string());
    let kind = de_param_kind(v.get_field("kind").ok_or("param kind missing")?)?;
    Ok(ParameterTemplate { name, kind })
}

fn de_param_kind(v: &JsonValue) -> Result<ParameterKind, String> {
    let ty = v.get_str("type").ok_or("kind missing type")?;
    match ty {
        "float" => Ok(ParameterKind::Float {
            min: v.get_f64("min").unwrap_or(0.0) as f32,
            max: v.get_f64("max").unwrap_or(1.0) as f32,
            default: v.get_f64("default").unwrap_or(0.0) as f32,
        }),
        "int" => Ok(ParameterKind::Int {
            min: v.get_i64("min").unwrap_or(0),
            max: v.get_i64("max").unwrap_or(100),
            default: v.get_i64("default").unwrap_or(0),
        }),
        "bool" => Ok(ParameterKind::Bool {
            default: v.get_bool("default").unwrap_or(false),
        }),
        "enum" => {
            let variants: Vec<&'static str> = v
                .get_array("variants")
                .unwrap_or(&[])
                .iter()
                .filter_map(|x| x.as_str().map(|s| leak_str(s.to_string())))
                .collect();
            let variants: &'static [&'static str] = Box::leak(variants.into_boxed_slice());
            let default = leak_str(v.get_str("default").unwrap_or("").to_string());
            Ok(ParameterKind::Enum { variants, default })
        }
        "file" => {
            let extensions: Vec<&'static str> = v
                .get_array("extensions")
                .unwrap_or(&[])
                .iter()
                .filter_map(|x| x.as_str().map(|s| leak_str(s.to_string())))
                .collect();
            let extensions: &'static [&'static str] = Box::leak(extensions.into_boxed_slice());
            Ok(ParameterKind::File { extensions })
        }
        "song_name" => Ok(ParameterKind::SongName),
        other => Err(format!("unknown parameter kind: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: ModuleDescriptorTemplate = ModuleDescriptorTemplate {
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

    #[test]
    fn template_round_trip() {
        let blob = serialize_module_descriptor_template(&SAMPLE);
        let back = deserialize_module_descriptor_template(&blob).expect("deserialize");

        assert_eq!(back.name, "Mixer");
        assert_eq!(back.axes.len(), 1);
        assert_eq!(back.axes[0].name, "channels");
        assert_eq!(back.global_inputs.len(), 1);
        assert_eq!(back.global_inputs[0].name, "cv");
        assert_eq!(back.per_axis_inputs.len(), 1);
        assert_eq!(back.per_axis_inputs[0].0, AxisId::CHANNELS);
        assert_eq!(back.per_axis_inputs[0].1.name, "in");
        assert_eq!(back.per_axis_outputs[0].1.name, "out");
        assert_eq!(back.structural_params.len(), 1);
        assert!(matches!(back.structural_params[0].kind, ParameterKind::Bool { default: false }));
        assert_eq!(back.per_axis_realtime_params.len(), 1);

        // Build descriptors from both and compare.
        let a = SAMPLE.build_channels(2);
        let b = back.build_channels(2);
        assert_eq!(a.module_name, b.module_name);
        assert_eq!(a.inputs.len(), b.inputs.len());
        assert_eq!(a.outputs.len(), b.outputs.len());
        assert_eq!(a.realtime_params.len(), b.realtime_params.len());
        assert_eq!(a.structural_params.len(), b.structural_params.len());
    }
}
