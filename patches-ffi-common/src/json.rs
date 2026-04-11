//! Hand-rolled JSON serialization for types that cross the FFI boundary on the
//! control thread. This avoids adding serde as a dependency to patches-core.
//!
//! Deserialized `&'static str` fields are produced by leaking `String`s via
//! `Box::leak`. This is intentional and bounded: one set of leaked strings per
//! module type per library load.

use std::sync::Arc;
use patches_core::{
    CableKind, ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind,
    ParameterMap, ParameterValue, PortDescriptor,
};

// ── JSON writing helpers ─────────────────────────────────────────────────────

fn write_string(out: &mut String, s: &str) {
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

fn write_f32(out: &mut String, v: f32) {
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
        ParameterKind::Array { default, length } => {
            out.push_str(&format!("{{\"type\":\"array\",\"length\":{length},\"default\":["));
            for (i, v) in default.iter().enumerate() {
                if i > 0 { out.push(','); }
                write_string(out, v);
            }
            out.push_str("]}");
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
        ParameterValue::Array(v) => {
            out.push_str("{\"type\":\"array\",\"v\":[");
            for (i, s) in v.iter().enumerate() {
                if i > 0 { out.push(','); }
                write_string(out, s);
            }
            out.push_str("]}");
        }
    }
}

// ── JSON parsing (minimal, hand-rolled) ──────────────────────────────────────

/// A minimal JSON value type sufficient for our deserialization needs.
#[derive(Debug, Clone)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    fn as_str(&self) -> Option<&str> {
        if let JsonValue::Str(s) = self { Some(s) } else { None }
    }
    fn as_f64(&self) -> Option<f64> {
        if let JsonValue::Number(n) = self { Some(*n) } else { None }
    }
    fn as_i64(&self) -> Option<i64> {
        if let JsonValue::Number(n) = self { Some(*n as i64) } else { None }
    }
    fn as_bool(&self) -> Option<bool> {
        if let JsonValue::Bool(b) = self { Some(*b) } else { None }
    }
    fn as_array(&self) -> Option<&[JsonValue]> {
        if let JsonValue::Array(a) = self { Some(a) } else { None }
    }
    fn get(&self, key: &str) -> Option<&JsonValue> {
        if let JsonValue::Object(pairs) = self {
            pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        } else {
            None
        }
    }
    fn as_usize(&self) -> Option<usize> {
        self.as_f64().map(|n| n as usize)
    }
}

struct JsonParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, pos: 0 }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() && matches!(self.input[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.input.get(self.pos).copied()?;
        self.pos += 1;
        Some(b)
    }

    fn expect(&mut self, b: u8) -> Result<(), String> {
        self.skip_whitespace();
        match self.advance() {
            Some(c) if c == b => Ok(()),
            Some(c) => Err(format!("expected '{}', found '{}' at pos {}", b as char, c as char, self.pos - 1)),
            None => Err(format!("expected '{}', found EOF", b as char)),
        }
    }

    fn parse_value(&mut self) -> Result<JsonValue, String> {
        self.skip_whitespace();
        match self.peek() {
            Some(b'"') => self.parse_string().map(JsonValue::Str),
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b't') => self.parse_literal(b"true", JsonValue::Bool(true)),
            Some(b'f') => self.parse_literal(b"false", JsonValue::Bool(false)),
            Some(b'n') => self.parse_literal(b"null", JsonValue::Null),
            Some(b'-') | Some(b'0'..=b'9') => self.parse_number(),
            Some(c) => Err(format!("unexpected char '{}' at pos {}", c as char, self.pos)),
            None => Err("unexpected EOF".to_string()),
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut s = String::new();
        loop {
            match self.advance() {
                Some(b'"') => return Ok(s),
                Some(b'\\') => {
                    match self.advance() {
                        Some(b'"') => s.push('"'),
                        Some(b'\\') => s.push('\\'),
                        Some(b'/') => s.push('/'),
                        Some(b'n') => s.push('\n'),
                        Some(b'r') => s.push('\r'),
                        Some(b't') => s.push('\t'),
                        Some(b'u') => {
                            let mut hex = String::with_capacity(4);
                            for _ in 0..4 {
                                hex.push(self.advance().ok_or("unexpected EOF in \\u escape")? as char);
                            }
                            let code = u32::from_str_radix(&hex, 16)
                                .map_err(|e| format!("bad unicode escape: {e}"))?;
                            if let Some(c) = char::from_u32(code) {
                                s.push(c);
                            }
                        }
                        Some(c) => s.push(c as char),
                        None => return Err("unexpected EOF in string escape".to_string()),
                    }
                }
                Some(c) => s.push(c as char),
                None => return Err("unexpected EOF in string".to_string()),
            }
        }
    }

    fn parse_number(&mut self) -> Result<JsonValue, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') { self.pos += 1; }
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        if self.pos < self.input.len() && self.input[self.pos] == b'.' {
            self.pos += 1;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos < self.input.len() && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E') {
            self.pos += 1;
            if self.pos < self.input.len() && (self.input[self.pos] == b'+' || self.input[self.pos] == b'-') {
                self.pos += 1;
            }
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let s = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|e| format!("bad number bytes: {e}"))?;
        let n: f64 = s.parse().map_err(|e| format!("bad number '{s}': {e}"))?;
        Ok(JsonValue::Number(n))
    }

    fn parse_object(&mut self) -> Result<JsonValue, String> {
        self.expect(b'{')?;
        let mut pairs = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') { self.pos += 1; return Ok(JsonValue::Object(pairs)); }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.expect(b':')?;
            let value = self.parse_value()?;
            pairs.push((key, value));
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b'}') => { self.pos += 1; return Ok(JsonValue::Object(pairs)); }
                _ => return Err(format!("expected ',' or '}}' at pos {}", self.pos)),
            }
        }
    }

    fn parse_array(&mut self) -> Result<JsonValue, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') { self.pos += 1; return Ok(JsonValue::Array(items)); }
        loop {
            items.push(self.parse_value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b']') => { self.pos += 1; return Ok(JsonValue::Array(items)); }
                _ => return Err(format!("expected ',' or ']' at pos {}", self.pos)),
            }
        }
    }

    fn parse_literal(&mut self, literal: &[u8], value: JsonValue) -> Result<JsonValue, String> {
        for &expected in literal {
            match self.advance() {
                Some(c) if c == expected => {}
                _ => return Err(format!("bad literal at pos {}", self.pos)),
            }
        }
        Ok(value)
    }
}

fn parse_json(input: &[u8]) -> Result<JsonValue, String> {
    let mut parser = JsonParser::new(input);
    let value = parser.parse_value()?;
    Ok(value)
}

// ── Leak helper ──────────────────────────────────────────────────────────────

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn leak_static_slice(strings: Vec<&'static str>) -> &'static [&'static str] {
    Box::leak(strings.into_boxed_slice())
}

// ── ModuleDescriptor deserialization ─────────────────────────────────────────

pub fn deserialize_module_descriptor(data: &[u8]) -> Result<ModuleDescriptor, String> {
    let root = parse_json(data)?;

    let module_name = leak_str(
        root.get("module_name")
            .and_then(|v| v.as_str())
            .ok_or("missing module_name")?
            .to_string(),
    );

    let shape_val = root.get("shape").ok_or("missing shape")?;
    let shape = ModuleShape {
        channels: shape_val.get("channels").and_then(|v| v.as_usize()).unwrap_or(0),
        length: shape_val.get("length").and_then(|v| v.as_usize()).unwrap_or(0),
        high_quality: shape_val.get("high_quality").and_then(|v| v.as_bool()).unwrap_or(false),
    };

    let inputs = root.get("inputs")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(deserialize_port_descriptor).collect::<Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();

    let outputs = root.get("outputs")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(deserialize_port_descriptor).collect::<Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();

    let parameters = root.get("parameters")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(deserialize_parameter_descriptor).collect::<Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();

    Ok(ModuleDescriptor {
        module_name,
        shape,
        inputs,
        outputs,
        parameters,
    })
}

fn deserialize_port_descriptor(val: &JsonValue) -> Result<PortDescriptor, String> {
    let name = leak_str(
        val.get("name").and_then(|v| v.as_str()).ok_or("port missing name")?.to_string(),
    );
    let index = val.get("index").and_then(|v| v.as_usize()).unwrap_or(0);
    let kind = match val.get("kind").and_then(|v| v.as_str()) {
        Some("poly") => CableKind::Poly,
        _ => CableKind::Mono,
    };
    Ok(PortDescriptor { name, index, kind })
}

fn deserialize_parameter_descriptor(val: &JsonValue) -> Result<ParameterDescriptor, String> {
    let name = leak_str(
        val.get("name").and_then(|v| v.as_str()).ok_or("param missing name")?.to_string(),
    );
    let index = val.get("index").and_then(|v| v.as_usize()).unwrap_or(0);
    let pt = val.get("parameter_type").ok_or("param missing parameter_type")?;
    let parameter_type = deserialize_parameter_kind(pt)?;
    Ok(ParameterDescriptor { name, index, parameter_type })
}

fn deserialize_parameter_kind(val: &JsonValue) -> Result<ParameterKind, String> {
    let ty = val.get("type").and_then(|v| v.as_str()).ok_or("kind missing type")?;
    match ty {
        "float" => {
            let min = val.get("min").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let max = val.get("max").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let default = val.get("default").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            Ok(ParameterKind::Float { min, max, default })
        }
        "int" => {
            let min = val.get("min").and_then(|v| v.as_i64()).unwrap_or(0);
            let max = val.get("max").and_then(|v| v.as_i64()).unwrap_or(100);
            let default = val.get("default").and_then(|v| v.as_i64()).unwrap_or(0);
            Ok(ParameterKind::Int { min, max, default })
        }
        "bool" => {
            let default = val.get("default").and_then(|v| v.as_bool()).unwrap_or(false);
            Ok(ParameterKind::Bool { default })
        }
        "enum" => {
            let variants_arr = val.get("variants")
                .and_then(|v| v.as_array())
                .ok_or("enum kind missing variants")?;
            let variants: Vec<&'static str> = variants_arr.iter()
                .filter_map(|v| v.as_str().map(|s| leak_str(s.to_string())))
                .collect();
            let variants = leak_static_slice(variants);
            let default = leak_str(
                val.get("default").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            );
            Ok(ParameterKind::Enum { variants, default })
        }
        "string" => {
            let default = leak_str(
                val.get("default").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            );
            Ok(ParameterKind::String { default })
        }
        "file" => {
            let empty = vec![];
            let exts_json = val.get("extensions").and_then(|v| v.as_array()).unwrap_or(&empty);
            let extensions: &'static [&'static str] = Box::leak(
                exts_json.iter()
                    .filter_map(|v| v.as_str().map(|s| leak_str(s.to_string())))
                    .collect::<Vec<&'static str>>()
                    .into_boxed_slice(),
            );
            Ok(ParameterKind::File { extensions })
        }
        "array" => {
            let length = val.get("length").and_then(|v| v.as_usize()).unwrap_or(0);
            let default_arr = val.get("default")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| leak_str(s.to_string())))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let default = leak_static_slice(default_arr);
            Ok(ParameterKind::Array { default, length })
        }
        other => Err(format!("unknown parameter kind: {other}")),
    }
}

// ── ParameterMap deserialization ─────────────────────────────────────────────

pub fn deserialize_parameter_map(data: &[u8]) -> Result<ParameterMap, String> {
    let root = parse_json(data)?;
    let entries = root.as_array().ok_or("expected JSON array for ParameterMap")?;
    let mut map = ParameterMap::new();
    for entry in entries {
        let name = entry.get("name").and_then(|v| v.as_str()).ok_or("entry missing name")?;
        let index = entry.get("index").and_then(|v| v.as_usize()).unwrap_or(0);
        let value_obj = entry.get("value").ok_or("entry missing value")?;
        let value = deserialize_parameter_value(value_obj)?;
        map.insert_param(name.to_string(), index, value);
    }
    Ok(map)
}

fn deserialize_parameter_value(val: &JsonValue) -> Result<ParameterValue, String> {
    let ty = val.get("type").and_then(|v| v.as_str()).ok_or("value missing type")?;
    match ty {
        "float" => {
            let v = val.get("v").and_then(|v| v.as_f64()).ok_or("float missing v")? as f32;
            Ok(ParameterValue::Float(v))
        }
        "int" => {
            let v = val.get("v").and_then(|v| v.as_i64()).ok_or("int missing v")?;
            Ok(ParameterValue::Int(v))
        }
        "bool" => {
            let v = val.get("v").and_then(|v| v.as_bool()).ok_or("bool missing v")?;
            Ok(ParameterValue::Bool(v))
        }
        "enum" => {
            let v = leak_str(
                val.get("v").and_then(|v| v.as_str()).ok_or("enum missing v")?.to_string(),
            );
            Ok(ParameterValue::Enum(v))
        }
        "string" => {
            let v = val.get("v").and_then(|v| v.as_str()).ok_or("string missing v")?.to_string();
            Ok(ParameterValue::String(v))
        }
        "file" => {
            let v = val.get("v").and_then(|v| v.as_str()).ok_or("file missing v")?.to_string();
            Ok(ParameterValue::File(v))
        }
        "array" => {
            let arr = val.get("v").and_then(|v| v.as_array()).ok_or("array missing v")?;
            let strings: Vec<String> = arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            Ok(ParameterValue::Array(Arc::from(strings)))
        }
        other => Err(format!("unknown value type: {other}")),
    }
}

// ── BuildError serialization ─────────────────────────────────────────────────

pub fn serialize_error(msg: &str) -> Vec<u8> {
    msg.as_bytes().to_vec()
}

pub fn deserialize_error(data: &[u8]) -> String {
    String::from_utf8_lossy(data).into_owned()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patches_core::{CableKind, ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind, ParameterMap, ParameterValue, PortDescriptor};

    #[test]
    fn module_descriptor_round_trip() {
        let desc = ModuleDescriptor {
            module_name: "TestGain",
            shape: ModuleShape { channels: 2, length: 8, high_quality: true },
            inputs: vec![
                PortDescriptor { name: "in", index: 0, kind: CableKind::Mono },
                PortDescriptor { name: "sidechain", index: 0, kind: CableKind::Poly },
            ],
            outputs: vec![
                PortDescriptor { name: "out", index: 0, kind: CableKind::Mono },
            ],
            parameters: vec![
                ParameterDescriptor { name: "gain", index: 0, parameter_type: ParameterKind::Float { min: 0.0, max: 2.0, default: 1.0 } },
                ParameterDescriptor { name: "mode", index: 0, parameter_type: ParameterKind::Enum { variants: &["linear", "log"], default: "linear" } },
                ParameterDescriptor { name: "active", index: 0, parameter_type: ParameterKind::Bool { default: true } },
                ParameterDescriptor { name: "voices", index: 0, parameter_type: ParameterKind::Int { min: 1, max: 8, default: 4 } },
                ParameterDescriptor { name: "label", index: 0, parameter_type: ParameterKind::String { default: "default" } },
                ParameterDescriptor { name: "steps", index: 0, parameter_type: ParameterKind::Array { default: &["C4", "E4"], length: 16 } },
            ],
        };

        let json = serialize_module_descriptor(&desc);
        let back = deserialize_module_descriptor(&json).expect("deserialize failed");

        assert_eq!(back.module_name, "TestGain");
        assert_eq!(back.shape.channels, 2);
        assert_eq!(back.shape.length, 8);
        assert!(back.shape.high_quality);
        assert_eq!(back.inputs.len(), 2);
        assert_eq!(back.inputs[0].name, "in");
        assert_eq!(back.inputs[0].kind, CableKind::Mono);
        assert_eq!(back.inputs[1].name, "sidechain");
        assert_eq!(back.inputs[1].kind, CableKind::Poly);
        assert_eq!(back.outputs.len(), 1);
        assert_eq!(back.outputs[0].name, "out");
        assert_eq!(back.parameters.len(), 6);

        // Float param
        match &back.parameters[0].parameter_type {
            ParameterKind::Float { min, max, default } => {
                assert_eq!(*min, 0.0);
                assert_eq!(*max, 2.0);
                assert_eq!(*default, 1.0);
            }
            other => panic!("expected Float, got {other:?}"),
        }

        // Enum param
        match &back.parameters[1].parameter_type {
            ParameterKind::Enum { variants, default } => {
                assert_eq!(variants.len(), 2);
                assert_eq!(variants[0], "linear");
                assert_eq!(variants[1], "log");
                assert_eq!(*default, "linear");
            }
            other => panic!("expected Enum, got {other:?}"),
        }

        // Bool param
        match &back.parameters[2].parameter_type {
            ParameterKind::Bool { default } => assert!(*default),
            other => panic!("expected Bool, got {other:?}"),
        }

        // Int param
        match &back.parameters[3].parameter_type {
            ParameterKind::Int { min, max, default } => {
                assert_eq!(*min, 1);
                assert_eq!(*max, 8);
                assert_eq!(*default, 4);
            }
            other => panic!("expected Int, got {other:?}"),
        }

        // String param
        match &back.parameters[4].parameter_type {
            ParameterKind::String { default } => assert_eq!(*default, "default"),
            other => panic!("expected String, got {other:?}"),
        }

        // Array param
        match &back.parameters[5].parameter_type {
            ParameterKind::Array { default, length } => {
                assert_eq!(*length, 16);
                assert_eq!(default.len(), 2);
                assert_eq!(default[0], "C4");
                assert_eq!(default[1], "E4");
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn parameter_map_round_trip() {
        let mut params = ParameterMap::new();
        params.insert("gain".to_string(), ParameterValue::Float(0.75));
        params.insert_param("pan".to_string(), 1, ParameterValue::Float(-0.5));
        params.insert("active".to_string(), ParameterValue::Bool(true));
        params.insert("voices".to_string(), ParameterValue::Int(6));
        params.insert("mode".to_string(), ParameterValue::Enum("log"));
        params.insert("path".to_string(), ParameterValue::String("/tmp/test.wav".to_string()));
        params.insert("steps".to_string(), ParameterValue::Array(
            vec!["C4".to_string(), "E4".to_string()].into(),
        ));

        let json = serialize_parameter_map(&params);
        let back = deserialize_parameter_map(&json).expect("deserialize failed");

        assert_eq!(back.get_scalar("gain"), Some(&ParameterValue::Float(0.75)));
        assert_eq!(back.get("pan", 1), Some(&ParameterValue::Float(-0.5)));
        assert_eq!(back.get_scalar("active"), Some(&ParameterValue::Bool(true)));
        assert_eq!(back.get_scalar("voices"), Some(&ParameterValue::Int(6)));
        // Enum: the deserialized variant is a leaked &'static str, compare by value
        match back.get_scalar("mode") {
            Some(ParameterValue::Enum(v)) => assert_eq!(*v, "log"),
            other => panic!("expected Enum(\"log\"), got {other:?}"),
        }
        assert_eq!(back.get_scalar("path"), Some(&ParameterValue::String("/tmp/test.wav".to_string())));
        match back.get_scalar("steps") {
            Some(ParameterValue::Array(arr)) => {
                assert_eq!(arr.len(), 2);
                assert_eq!(arr[0], "C4");
                assert_eq!(arr[1], "E4");
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn empty_parameter_map_round_trip() {
        let params = ParameterMap::new();
        let json = serialize_parameter_map(&params);
        let back = deserialize_parameter_map(&json).expect("deserialize failed");
        assert!(back.is_empty());
    }

    #[test]
    fn error_round_trip() {
        let msg = "parameter 'gain' out of range";
        let bytes = serialize_error(msg);
        let back = deserialize_error(&bytes);
        assert_eq!(back, msg);
    }
}
