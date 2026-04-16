use patches_core::{
    CableKind, ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind,
    ParameterMap, ParameterValue, PortDescriptor,
};

// ── JSON writing helpers ─────────────────────────────────────────────────────

pub(super) fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub(super) fn write_f32(out: &mut String, v: f32) {
    if v.is_nan() {
        out.push_str("null");
    } else if v.is_infinite() {
        if v > 0.0 { out.push_str("1e38"); } else { out.push_str("-1e38"); }
    } else {
        out.push_str(&format!("{v}"));
        // Ensure it looks like a float
        if !out.ends_with('.') && !out.contains('.') && !out.contains('e') && !out.contains('E') {
            out.push_str(".0");
        }
    }
}

// ── ModuleDescriptor serialization ───────────────────────────────────────────

pub fn serialize_module_descriptor(desc: &ModuleDescriptor) -> Vec<u8> {
    let mut out = String::with_capacity(512);
    out.push('{');

    out.push_str("\"module_name\":");
    write_string(&mut out, desc.module_name);

    out.push_str(",\"shape\":");
    write_shape(&mut out, &desc.shape);

    out.push_str(",\"inputs\":[");
    for (i, port) in desc.inputs.iter().enumerate() {
        if i > 0 { out.push(','); }
        write_port_descriptor(&mut out, port);
    }
    out.push(']');

    out.push_str(",\"outputs\":[");
    for (i, port) in desc.outputs.iter().enumerate() {
        if i > 0 { out.push(','); }
        write_port_descriptor(&mut out, port);
    }
    out.push(']');

    out.push_str(",\"parameters\":[");
    for (i, param) in desc.parameters.iter().enumerate() {
        if i > 0 { out.push(','); }
        write_parameter_descriptor(&mut out, param);
    }
    out.push(']');

    out.push('}');
    out.into_bytes()
}

fn write_shape(out: &mut String, shape: &ModuleShape) {
    out.push_str(&format!(
        "{{\"channels\":{},\"length\":{},\"high_quality\":{}}}",
        shape.channels,
        shape.length,
        shape.high_quality,
    ));
}

fn write_port_descriptor(out: &mut String, port: &PortDescriptor) {
    out.push('{');
    out.push_str("\"name\":");
    write_string(out, port.name);
    out.push_str(&format!(",\"index\":{}", port.index));
    out.push_str(",\"kind\":");
    match port.kind {
        CableKind::Mono => write_string(out, "mono"),
        CableKind::Poly => write_string(out, "poly"),
    }
    out.push('}');
}

fn write_parameter_descriptor(out: &mut String, param: &ParameterDescriptor) {
    out.push('{');
    out.push_str("\"name\":");
    write_string(out, param.name);
    out.push_str(&format!(",\"index\":{}", param.index));
    out.push_str(",\"parameter_type\":");
    write_parameter_kind(out, &param.parameter_type);
    out.push('}');
}

fn write_parameter_kind(out: &mut String, kind: &ParameterKind) {
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
        ParameterKind::String { default } => {
            out.push_str("{\"type\":\"string\",\"default\":");
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

// ── ParameterMap serialization ───────────────────────────────────────────────

pub fn serialize_parameter_map(params: &ParameterMap) -> Vec<u8> {
    let mut out = String::with_capacity(256);
    out.push('[');
    let mut first = true;
    for (name, index, value) in params.iter() {
        if !first { out.push(','); }
        first = false;
        out.push('{');
        out.push_str("\"name\":");
        write_string(&mut out, name);
        out.push_str(&format!(",\"index\":{index}"));
        out.push_str(",\"value\":");
        write_parameter_value(&mut out, value);
        out.push('}');
    }
    out.push(']');
    out.into_bytes()
}

fn write_parameter_value(out: &mut String, value: &ParameterValue) {
    match value {
        ParameterValue::Float(v) => {
            out.push_str("{\"type\":\"float\",\"v\":");
            write_f32(out, *v);
            out.push('}');
        }
        ParameterValue::Int(v) => {
            out.push_str(&format!("{{\"type\":\"int\",\"v\":{v}}}"));
        }
        ParameterValue::Bool(v) => {
            out.push_str(&format!("{{\"type\":\"bool\",\"v\":{v}}}"));
        }
        ParameterValue::Enum(v) => {
            out.push_str("{\"type\":\"enum\",\"v\":");
            write_string(out, v);
            out.push('}');
        }
        ParameterValue::String(v) => {
            out.push_str("{\"type\":\"string\",\"v\":");
            write_string(out, v);
            out.push('}');
        }
        ParameterValue::File(v) => {
            out.push_str("{\"type\":\"file\",\"v\":");
            write_string(out, v);
            out.push('}');
        }
        ParameterValue::FloatBuffer(_) => {
            // FloatBuffer is not serialized via JSON; it uses a binary sideband.
            // Write a placeholder so the JSON is valid.
            out.push_str("{\"type\":\"float_buffer\",\"v\":null}");
        }
    }
}

// ── BuildError serialization ─────────────────────────────────────────────────

pub fn serialize_error(msg: &str) -> Vec<u8> {
    msg.as_bytes().to_vec()
}
