use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// Identifies a single parameter by name and index.
///
/// For modules with only one parameter of a given name, `index` is `0` and
/// the key serialises as just `"name"`. For multi-channel modules that expose
/// several parameters with the same conceptual name (e.g. `"gain/0"`,
/// `"gain/1"`), `index` distinguishes them.
///
/// This mirrors [`crate::modules::module_descriptor::PortRef`] for ports.
///
/// `ParameterKey` is retained as a convenience value type (e.g. for display /
/// serialisation) but is no longer used as the internal storage key inside
/// [`ParameterMap`] — see the two-level storage layout described there.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParameterKey {
    pub name: String,
    pub index: usize,
}

impl ParameterKey {
    pub fn new(name: impl Into<String>, index: usize) -> Self {
        Self { name: name.into(), index }
    }
}

impl From<&str> for ParameterKey {
    /// Converts a bare name to a key with index 0.
    fn from(s: &str) -> Self {
        Self { name: s.to_string(), index: 0 }
    }
}

impl From<String> for ParameterKey {
    /// Converts a bare name to a key with index 0.
    fn from(s: String) -> Self {
        Self { name: s, index: 0 }
    }
}

impl From<super::module_descriptor::ParameterRef> for ParameterKey {
    fn from(r: super::module_descriptor::ParameterRef) -> Self {
        Self { name: r.name.to_string(), index: r.index }
    }
}

impl fmt::Display for ParameterKey {
    /// Formats as `"name"` when index is 0, or `"name/N"` for N > 0.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.index == 0 {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{}/{}", self.name, self.index)
        }
    }
}

// ── ParameterValue ────────────────────────────────────────────────────────────

/// An `Array` value stores its strings in an `Arc<[String]>` so that cloning
/// the value is O(1) — the data is shared, not copied.  Mutation requires
/// cloning the inner slice (copy-on-write pattern), which is only needed when
/// the DSL hot-reloads a new pattern, not on every audio tick.
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterValue {
    Float(f32),
    Int(i64),
    Bool(bool),
    Enum(&'static str),
    /// A single runtime string (e.g. a file path). Owned because values come
    /// from the DSL at runtime.
    String(String),
    // Array parameter values own their strings via Arc so that cloning is O(1).
    // Patterns come from the DSL at runtime and cannot be required to be 'static
    // (unlike Enum variants, which are a closed compile-time set declared in the
    // descriptor).
    Array(Arc<[String]>),
    /// A resolved absolute file path. Produced by the interpreter from
    /// `file("path")` DSL syntax. The planner replaces this with `FloatBuffer`
    /// after calling the module's `FileProcessor::process_file`.
    File(String),
    /// Pre-processed file data as a flat float buffer. Produced by the planner
    /// after calling `FileProcessor::process_file`. The `Arc` makes cloning
    /// O(1) — important because `ParameterMap::clone()` is called in the
    /// default `Module::update_parameters` implementation.
    FloatBuffer(Arc<[f32]>),
}

impl ParameterValue {
    pub fn kind_name(&self) -> &'static str {
        match self {
            ParameterValue::Float(_) => "float",
            ParameterValue::Int(_) => "int",
            ParameterValue::Bool(_) => "bool",
            ParameterValue::Enum(_) => "enum",
            ParameterValue::String(_) => "string",
            ParameterValue::Array(_) => "array",
            ParameterValue::File(_) => "file",
            ParameterValue::FloatBuffer(_) => "float_buffer",
        }
    }
}

// ── ParameterMap ──────────────────────────────────────────────────────────────

/// A map from parameter `(name, index)` pairs to [`ParameterValue`].
///
/// ## Storage layout
///
/// Internal storage is `HashMap<String, Vec<(usize, ParameterValue)>>`.
/// The outer lookup uses `Borrow<str>` (zero heap allocation); the inner
/// linear scan over indices is O(n_indices), which is almost always 1
/// (the vast majority of parameters are scalar and live at index 0).
///
/// This restores the zero-allocation lookup property that was lost when the
/// map was changed from `HashMap<String, ParameterValue>` to
/// `HashMap<ParameterKey, ParameterValue>` (T-0131).  There is no stable-Rust
/// way to implement `Borrow<Q>` for a composite borrowed key without `unsafe`.
#[derive(Debug, Clone, Default)]
pub struct ParameterMap(HashMap<String, Vec<(usize, ParameterValue)>>);

impl ParameterMap {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    // --- primary lookup -------------------------------------------------------

    /// Look up the value for `name` at `index`. Zero heap allocation.
    ///
    /// `index` distinguishes parameters that share a name but differ by position
    /// (e.g. `"gain/0"`, `"gain/1"`).  Pass `0` for scalar parameters that have
    /// only one value — or use [`get_scalar`](Self::get_scalar) as a convenience
    /// shorthand when `index` is always `0`.
    pub fn get(&self, name: &str, index: usize) -> Option<&ParameterValue> {
        self.0.get(name)?.iter().find(|(i, _)| *i == index).map(|(_, v)| v)
    }

    /// Look up the value for `name` at index 0. Zero heap allocation.
    ///
    /// Equivalent to `self.get(name, 0)`.  Use this for scalar parameters that
    /// are never indexed; use [`get`](Self::get) when `index` may be > 0.
    pub fn get_scalar(&self, name: &str) -> Option<&ParameterValue> {
        self.get(name, 0)
    }

    /// Remove and return the value for `name` at `index`, if present.
    pub fn take(&mut self, name: &str, index: usize) -> Option<ParameterValue> {
        let entries = self.0.get_mut(name)?;
        let pos = entries.iter().position(|(i, _)| *i == index)?;
        Some(entries.swap_remove(pos).1)
    }

    /// Remove and return the value for `name` at index 0.
    ///
    /// Equivalent to `self.take(name, 0)`.
    pub fn take_scalar(&mut self, name: &str) -> Option<ParameterValue> {
        self.take(name, 0)
    }

    // --- zero-index aliases ---------------------------------------------------

    /// Insert `value` for `name` at index 0.  Returns the previous value if any.
    pub fn insert(&mut self, name: String, value: ParameterValue) -> Option<ParameterValue> {
        self.insert_param(name, 0, value)
    }

    /// Return `true` if a value for `name` at index 0 is present.
    pub fn contains_key(&self, name: &str) -> bool {
        self.0.get(name).is_some_and(|v| v.iter().any(|(i, _)| *i == 0))
    }

    // --- explicit-index access ------------------------------------------------

    /// Insert `value` for `name` at `index`.  Returns the previous value if any.
    pub fn insert_param(
        &mut self,
        name: impl Into<String>,
        index: usize,
        value: ParameterValue,
    ) -> Option<ParameterValue> {
        let entries = self.0.entry(name.into()).or_default();
        if let Some(existing) = entries.iter_mut().find(|(i, _)| *i == index) {
            Some(std::mem::replace(&mut existing.1, value))
        } else {
            entries.push((index, value));
            None
        }
    }

    /// Insert the default value produced by `f` for `name` at `index` only if
    /// no value is currently present. Does nothing if the entry already exists.
    ///
    /// Replaces the `entry()/entry_param()` + `or_insert_with()` pattern used
    /// in `Module::build` to fill missing parameters with descriptor defaults.
    pub fn get_or_insert(&mut self, name: &str, index: usize, f: impl FnOnce() -> ParameterValue) {
        if self.get(name, index).is_none() {
            self.insert_param(name.to_string(), index, f());
        }
    }

    // --- standard plumbing ---------------------------------------------------

    pub fn is_empty(&self) -> bool {
        self.0.values().all(|v| v.is_empty())
    }

    pub fn len(&self) -> usize {
        self.0.values().map(|v| v.len()).sum()
    }

    /// Iterate over all `(name, index, value)` triples. No allocation.
    pub fn iter(&self) -> impl Iterator<Item = (&str, usize, &ParameterValue)> {
        self.0.iter().flat_map(|(name, entries)| {
            entries.iter().map(move |(idx, v)| (name.as_str(), *idx, v))
        })
    }

    /// Iterate over all `(name, index)` key pairs. No allocation.
    pub fn keys(&self) -> impl Iterator<Item = (&str, usize)> {
        self.0.iter().flat_map(|(name, entries)| {
            entries.iter().map(move |(idx, _)| (name.as_str(), *idx))
        })
    }
}

impl FromIterator<(String, usize, ParameterValue)> for ParameterMap {
    fn from_iter<I: IntoIterator<Item = (String, usize, ParameterValue)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (name, index, value) in iter {
            map.insert_param(name, index, value);
        }
        map
    }
}

impl FromIterator<(String, ParameterValue)> for ParameterMap {
    /// Collects string-keyed pairs as index-0 entries.
    fn from_iter<I: IntoIterator<Item = (String, ParameterValue)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (name, value) in iter {
            map.insert(name, value);
        }
        map
    }
}
