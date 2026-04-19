use patches_core::{
    CableKind, ModuleDescriptor, ModuleShape, ParameterDescriptor, ParameterKind,
    ParameterMap, ParameterValue, PortDescriptor,
};

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
    let poly_layout = match val.get("poly_layout").and_then(|v| v.as_str()) {
        Some("transport") => patches_core::cables::PolyLayout::Transport,
        Some("midi") => patches_core::cables::PolyLayout::Midi,
        _ => patches_core::cables::PolyLayout::Audio,
    };
    Ok(PortDescriptor { name, index, kind, poly_layout })
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
        "song_name" => Ok(ParameterKind::SongName),
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
            let v = val
                .get("v")
                .and_then(|v| v.as_i64())
                .ok_or("enum missing numeric v (ADR 0045 Spike 0: u32 variant index)")?;
            if v < 0 {
                return Err(format!("enum index must be non-negative, got {v}"));
            }
            Ok(ParameterValue::Enum(v as u32))
        }
        "file" => {
            let v = val.get("v").and_then(|v| v.as_str()).ok_or("file missing v")?.to_string();
            Ok(ParameterValue::File(v))
        }
        other => Err(format!("unknown value type: {other}")),
    }
}

// ── BuildError deserialization ───────────────────────────────────────────────

pub fn deserialize_error(data: &[u8]) -> String {
    String::from_utf8_lossy(data).into_owned()
}
