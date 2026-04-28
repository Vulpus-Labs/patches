use std::collections::HashMap;

/// A control-thread-only value carried by a structural parameter (ADR 0060).
///
/// Structural params never reach the audio thread, so the variant set is
/// free-form: it covers the realtime numeric types plus `String` for file
/// paths and other text-shaped construction inputs.
#[derive(Debug, Clone, PartialEq)]
pub enum StructuralValue {
    Bool(bool),
    Int(i64),
    Float(f32),
    String(String),
}

impl StructuralValue {
    pub fn kind_name(&self) -> &'static str {
        match self {
            StructuralValue::Bool(_) => "bool",
            StructuralValue::Int(_) => "int",
            StructuralValue::Float(_) => "float",
            StructuralValue::String(_) => "string",
        }
    }
}

/// Construction-time, control-thread carrier for structural parameter values
/// (ADR 0060). Allocation OK; never accessed from the audio thread.
///
/// Module impls read structural values inside [`Module::prepare`](crate::modules::Module::prepare)
/// to size internal buffers, decode file-backed inputs, or set quality
/// flags. Editing a structural value triggers a fresh `prepare` call and
/// an instance swap via the planner — there is no audio-thread structural
/// update path.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StructuralParams {
    inner: HashMap<String, Vec<(usize, StructuralValue)>>,
}

impl StructuralParams {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Insert or overwrite `(name, index) → value`.
    pub fn insert(&mut self, name: impl Into<String>, index: usize, value: StructuralValue) {
        let entries = self.inner.entry(name.into()).or_default();
        if let Some(slot) = entries.iter_mut().find(|(i, _)| *i == index) {
            slot.1 = value;
        } else {
            entries.push((index, value));
        }
    }

    pub fn get(&self, name: &str, index: usize) -> Option<&StructuralValue> {
        self.inner
            .get(name)
            .and_then(|v| v.iter().find(|(i, _)| *i == index).map(|(_, val)| val))
    }

    pub fn get_bool(&self, name: &str, index: usize) -> Option<bool> {
        match self.get(name, index)? {
            StructuralValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn get_int(&self, name: &str, index: usize) -> Option<i64> {
        match self.get(name, index)? {
            StructuralValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn get_float(&self, name: &str, index: usize) -> Option<f32> {
        match self.get(name, index)? {
            StructuralValue::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn get_string(&self, name: &str, index: usize) -> Option<&str> {
        match self.get(name, index)? {
            StructuralValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, usize, &StructuralValue)> {
        self.inner
            .iter()
            .flat_map(|(name, entries)| entries.iter().map(move |(i, v)| (name.as_str(), *i, v)))
    }
}
