use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use super::module_descriptor::{ModuleDescriptor, ParameterKind};

/// Identifies a single parameter by name and index.
///
/// For modules with only one parameter of a given name, `index` is `0` and
/// the key serialises as just `"name"`. For multi-channel modules that expose
/// several parameters with the same conceptual name (e.g. `"gain/0"`,
/// `"gain/1"`), `index` distinguishes them.
///
/// This mirrors [`crate::modules::module_descriptor::PortRef`] for ports.
///
/// `ParameterKey` is retained as a value type for `param_layout` slot keys;
/// it is not the internal storage key of [`ParameterMap`].
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
    fn from(s: &str) -> Self {
        Self { name: s.to_string(), index: 0 }
    }
}

impl From<String> for ParameterKey {
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.index == 0 {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{}/{}", self.name, self.index)
        }
    }
}

// ── ParameterValue ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum ParameterValue {
    Float(f32),
    Int(i64),
    Bool(bool),
    /// Enum variant index matching the order of
    /// [`ParameterKind::Enum::variants`](super::module_descriptor::ParameterKind::Enum).
    Enum(u32),
    /// A resolved absolute file path. Produced by the interpreter from
    /// `file("path")` DSL syntax. The planner replaces this with `FloatBuffer`
    /// after calling the module's `FileProcessor::process_file`.
    File(String),
    /// Pre-processed file data as a flat float buffer.
    FloatBuffer(Arc<[f32]>),
}

impl From<f32> for ParameterValue {
    fn from(v: f32) -> Self { ParameterValue::Float(v) }
}
impl From<bool> for ParameterValue {
    fn from(v: bool) -> Self { ParameterValue::Bool(v) }
}
impl From<i64> for ParameterValue {
    fn from(v: i64) -> Self { ParameterValue::Int(v) }
}

impl ParameterValue {
    pub fn kind_name(&self) -> &'static str {
        match self {
            ParameterValue::Float(_) => "float",
            ParameterValue::Int(_) => "int",
            ParameterValue::Bool(_) => "bool",
            ParameterValue::Enum(_) => "enum",
            ParameterValue::File(_) => "file",
            ParameterValue::FloatBuffer(_) => "float_buffer",
        }
    }
}

// ── ParameterMap ──────────────────────────────────────────────────────────────

/// An immutable map from `(name, index)` to [`ParameterValue`].
///
/// Conceptually a **partial or complete assignment** over a module's declared
/// parameter slots. Construction is funnelled through two constructors:
///
/// - [`ParameterMap::defaults`] produces a map covering every slot declared
///   by a [`ModuleDescriptor`].
/// - [`ParameterMap::with_overrides`] layers sparse overrides on top of a
///   base map (typically a defaults map).
///
/// A sparse map (e.g. the result of diffing two assignments) is built via
/// [`FromIterator`] and is expected to be merged into a complete map before
/// consumption.
///
/// There is no mutation API — no `insert`, `set`, `remove`, or builder. If you
/// find yourself reaching for one, you are probably trying to express one of
/// the two construction patterns above.
///
/// ## Storage layout
///
/// Internal storage is `HashMap<String, Vec<(usize, ParameterValue)>>`.
/// The outer lookup uses `Borrow<str>` (zero heap allocation); the inner
/// linear scan over indices is O(n_indices), which is almost always 1.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParameterMap(HashMap<String, Vec<(usize, ParameterValue)>>);

impl ParameterMap {
    /// Construct an empty map. Transitional — production paths use
    /// [`ParameterMap::defaults`] / [`ParameterMap::with_overrides`];
    /// tests should prefer [`FromIterator`]. Retained so the migration
    /// from the old accessor-heavy API (see ticket 0693) can be phased.
    #[doc(hidden)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Scalar-index convenience for [`insert_param`]. Transitional.
    #[doc(hidden)]
    pub fn insert(&mut self, name: String, value: ParameterValue) {
        self.insert_param(name, 0, value);
    }

    /// Insert or overwrite `(name, index) → value`. Transitional — see
    /// [`ParameterMap::new`]. Production paths must not reach for this;
    /// it exists to keep legacy test construction working during the
    /// migration tracked by ticket 0693.
    #[doc(hidden)]
    pub fn insert_param(
        &mut self,
        name: impl Into<String>,
        index: usize,
        value: ParameterValue,
    ) {
        let entries = self.0.entry(name.into()).or_default();
        if let Some(existing) = entries.iter_mut().find(|(i, _)| *i == index) {
            existing.1 = value;
        } else {
            entries.push((index, value));
        }
    }

    /// Construct a fully-populated map using each parameter's
    /// [`ParameterKind::default_value`]. This is the **user-facing** default
    /// — `File` parameters appear as `File(empty-path)`, `SongName` as
    /// `Int(-1)` — suitable as the base for filling a partial user-supplied
    /// map before the planner resolves file contents.
    pub fn declared_defaults(descriptor: &ModuleDescriptor) -> Self {
        descriptor
            .parameters
            .iter()
            .map(|p| (p.name.to_string(), p.index, p.parameter_type.default_value()))
            .collect()
    }

    /// Construct a fully-populated map from a descriptor's declared defaults,
    /// in the **post-resolution** shape the frame packer expects. `File`
    /// parameters appear as an empty `FloatBuffer` stand-in (the planner
    /// replaces these with real file contents before frame pack, ticket
    /// 0599); `SongName` is `Int(0)`.
    pub fn defaults(descriptor: &ModuleDescriptor) -> Self {
        let mut inner: HashMap<String, Vec<(usize, ParameterValue)>> = HashMap::new();
        for p in &descriptor.parameters {
            let v = match &p.parameter_type {
                ParameterKind::Float { default, .. } => ParameterValue::Float(*default),
                ParameterKind::Int { default, .. } => ParameterValue::Int(*default),
                ParameterKind::Bool { default } => ParameterValue::Bool(*default),
                ParameterKind::Enum { variants, default, .. } => {
                    let idx = variants.iter().position(|v| v == default).unwrap_or(0);
                    ParameterValue::Enum(idx as u32)
                }
                ParameterKind::File { .. } => ParameterValue::FloatBuffer(
                    Arc::<[f32]>::from(Vec::<f32>::new().into_boxed_slice()),
                ),
                ParameterKind::SongName => ParameterValue::Int(0),
            };
            inner.entry(p.name.to_string()).or_default().push((p.index, v));
        }
        Self(inner)
    }

    /// Layer sparse `overrides` on top of `base`, producing a new map.
    ///
    /// Any `(name, index)` present in `overrides` replaces the corresponding
    /// entry in `base`. Entries in `base` not covered by `overrides` are
    /// carried over unchanged. Entries in `overrides` with keys not present
    /// in `base` are added.
    ///
    /// Typical use: `with_overrides(&ParameterMap::defaults(desc), dsl_values)`
    /// to produce a complete assignment from user-supplied overrides.
    pub fn with_overrides(
        base: &Self,
        overrides: impl IntoIterator<Item = (String, usize, ParameterValue)>,
    ) -> Self {
        let mut out = base.clone();
        for (name, index, value) in overrides {
            let entries = out.0.entry(name).or_default();
            if let Some(existing) = entries.iter_mut().find(|(i, _)| *i == index) {
                existing.1 = value;
            } else {
                entries.push((index, value));
            }
        }
        out
    }

    /// Look up the value for `name` at `index`. Zero heap allocation.
    pub fn get(&self, name: &str, index: usize) -> Option<&ParameterValue> {
        self.0.get(name)?.iter().find(|(i, _)| *i == index).map(|(_, v)| v)
    }

    /// Iterate over all `(name, index, value)` triples.
    pub fn iter(&self) -> impl Iterator<Item = (&str, usize, &ParameterValue)> {
        self.0.iter().flat_map(|(name, entries)| {
            entries.iter().map(move |(idx, v)| (name.as_str(), *idx, v))
        })
    }

    /// `true` when no entries are present. One production caller
    /// (`planner::builder::diff_non_empty`) uses this to skip empty diffs.
    pub fn is_empty(&self) -> bool {
        self.0.values().all(|v| v.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::module_descriptor::{ModuleDescriptor, ParameterDescriptor, ParameterKind};

    fn pv_f(x: f32) -> ParameterValue { ParameterValue::Float(x) }

    fn descriptor_with_two_floats() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "test",
            inputs: Vec::new(),
            outputs: Vec::new(),
            parameters: vec![
                ParameterDescriptor {
                    name: "cutoff",
                    index: 0,
                    parameter_type: ParameterKind::Float { default: 1000.0, min: 0.0, max: 20000.0 },
                },
                ParameterDescriptor {
                    name: "q",
                    index: 0,
                    parameter_type: ParameterKind::Float { default: 0.7, min: 0.1, max: 10.0 },
                },
            ],
            shape: crate::modules::module_descriptor::ModuleShape::default(),
        }
    }

    fn descriptor_with_indexed() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "test",
            inputs: Vec::new(),
            outputs: Vec::new(),
            parameters: vec![
                ParameterDescriptor {
                    name: "gain",
                    index: 0,
                    parameter_type: ParameterKind::Float { default: 1.0, min: 0.0, max: 2.0 },
                },
                ParameterDescriptor {
                    name: "gain",
                    index: 1,
                    parameter_type: ParameterKind::Float { default: 0.5, min: 0.0, max: 2.0 },
                },
            ],
            shape: crate::modules::module_descriptor::ModuleShape::default(),
        }
    }

    fn descriptor_with_enum() -> ModuleDescriptor {
        ModuleDescriptor {
            module_name: "test",
            inputs: Vec::new(),
            outputs: Vec::new(),
            parameters: vec![ParameterDescriptor {
                name: "mode",
                index: 0,
                parameter_type: ParameterKind::Enum {
                    variants: &["a", "b", "c"],
                    default: "b",
                },
            }],
            shape: crate::modules::module_descriptor::ModuleShape::default(),
        }
    }

    #[test]
    fn defaults_covers_every_declared_slot() {
        let d = descriptor_with_two_floats();
        let m = ParameterMap::defaults(&d);
        assert_eq!(m.get("cutoff", 0), Some(&pv_f(1000.0)));
        assert_eq!(m.get("q", 0), Some(&pv_f(0.7)));
        assert_eq!(m.iter().count(), 2);
    }

    #[test]
    fn defaults_preserves_indexed_slots() {
        let d = descriptor_with_indexed();
        let m = ParameterMap::defaults(&d);
        assert_eq!(m.get("gain", 0), Some(&pv_f(1.0)));
        assert_eq!(m.get("gain", 1), Some(&pv_f(0.5)));
        assert_eq!(m.iter().count(), 2);
    }

    #[test]
    fn defaults_resolves_enum_default_to_variant_index() {
        let d = descriptor_with_enum();
        let m = ParameterMap::defaults(&d);
        assert_eq!(m.get("mode", 0), Some(&ParameterValue::Enum(1)));
    }

    #[test]
    fn defaults_unknown_enum_default_falls_back_to_zero() {
        let d = ModuleDescriptor {
            module_name: "t",
            inputs: Vec::new(),
            outputs: Vec::new(),
            parameters: vec![ParameterDescriptor {
                name: "mode",
                index: 0,
                parameter_type: ParameterKind::Enum { variants: &["x", "y"], default: "missing" },
            }],
            shape: crate::modules::module_descriptor::ModuleShape::default(),
        };
        let m = ParameterMap::defaults(&d);
        assert_eq!(m.get("mode", 0), Some(&ParameterValue::Enum(0)));
    }

    #[test]
    fn declared_defaults_uses_default_value_semantics() {
        let d = descriptor_with_two_floats();
        let m = ParameterMap::declared_defaults(&d);
        assert_eq!(m.get("cutoff", 0), Some(&pv_f(1000.0)));
        assert_eq!(m.get("q", 0), Some(&pv_f(0.7)));
        assert_eq!(m.iter().count(), 2);
    }

    #[test]
    fn with_overrides_replaces_existing_keys() {
        let d = descriptor_with_two_floats();
        let base = ParameterMap::defaults(&d);
        let over = ParameterMap::with_overrides(
            &base,
            [("cutoff".to_string(), 0, pv_f(440.0))],
        );
        assert_eq!(over.get("cutoff", 0), Some(&pv_f(440.0)));
        assert_eq!(over.get("q", 0), Some(&pv_f(0.7)));
    }

    #[test]
    fn with_overrides_adds_new_keys() {
        let base: ParameterMap = ParameterMap::default();
        let m = ParameterMap::with_overrides(
            &base,
            [
                ("a".to_string(), 0, pv_f(1.0)),
                ("b".to_string(), 0, pv_f(2.0)),
            ],
        );
        assert_eq!(m.get("a", 0), Some(&pv_f(1.0)));
        assert_eq!(m.get("b", 0), Some(&pv_f(2.0)));
        assert_eq!(m.iter().count(), 2);
    }

    #[test]
    fn with_overrides_distinguishes_indices_under_same_name() {
        let base: ParameterMap = [
            ("gain".to_string(), 0, pv_f(0.0)),
            ("gain".to_string(), 1, pv_f(0.0)),
        ].into_iter().collect();
        let m = ParameterMap::with_overrides(
            &base,
            [("gain".to_string(), 1, pv_f(0.9))],
        );
        assert_eq!(m.get("gain", 0), Some(&pv_f(0.0)));
        assert_eq!(m.get("gain", 1), Some(&pv_f(0.9)));
    }

    #[test]
    fn iter_yields_all_entries() {
        let m: ParameterMap = [
            ("a".to_string(), 0, pv_f(1.0)),
            ("a".to_string(), 1, pv_f(2.0)),
            ("b".to_string(), 0, pv_f(3.0)),
        ].into_iter().collect();
        let mut entries: Vec<_> = m.iter().map(|(n, i, v)| (n.to_string(), i, v.clone())).collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        assert_eq!(entries, vec![
            ("a".to_string(), 0, pv_f(1.0)),
            ("a".to_string(), 1, pv_f(2.0)),
            ("b".to_string(), 0, pv_f(3.0)),
        ]);
    }

    #[test]
    fn is_empty_distinguishes_empty_and_non_empty() {
        let empty = ParameterMap::default();
        assert!(empty.is_empty());
        let nonempty: ParameterMap = [("x".to_string(), 0, pv_f(1.0))].into_iter().collect();
        assert!(!nonempty.is_empty());
    }

    #[test]
    fn from_iter_collects_distinct_entries() {
        let m: ParameterMap = [
            ("a".to_string(), 0, pv_f(1.0)),
            ("b".to_string(), 0, pv_f(2.0)),
        ].into_iter().collect();
        assert_eq!(m.get("a", 0), Some(&pv_f(1.0)));
        assert_eq!(m.get("b", 0), Some(&pv_f(2.0)));
        assert_eq!(m.iter().count(), 2);
    }

    #[test]
    fn from_iter_later_entry_wins_for_duplicate_key() {
        let m: ParameterMap = [
            ("k".to_string(), 0, pv_f(1.0)),
            ("k".to_string(), 0, pv_f(2.0)),
        ].into_iter().collect();
        assert_eq!(m.get("k", 0), Some(&pv_f(2.0)));
        assert_eq!(m.iter().count(), 1);
    }

    #[test]
    fn insert_param_overwrites_existing_index() {
        let mut m = ParameterMap::new();
        m.insert_param("k", 0, pv_f(1.0));
        m.insert_param("k", 0, pv_f(2.0));
        assert_eq!(m.get("k", 0), Some(&pv_f(2.0)));
        assert_eq!(m.iter().count(), 1);
    }

    #[test]
    fn insert_param_distinct_indices_coexist() {
        let mut m = ParameterMap::new();
        m.insert_param("k", 0, pv_f(1.0));
        m.insert_param("k", 1, pv_f(2.0));
        assert_eq!(m.get("k", 0), Some(&pv_f(1.0)));
        assert_eq!(m.get("k", 1), Some(&pv_f(2.0)));
    }
}

/// Collect a sparse map from an iterator of `(name, index, value)`.
///
/// Produces a possibly-sparse map. Use [`ParameterMap::defaults`] +
/// [`ParameterMap::with_overrides`] when a descriptor-complete map is needed.
/// This impl exists for planner diffs and test construction.
impl FromIterator<(String, usize, ParameterValue)> for ParameterMap {
    fn from_iter<I: IntoIterator<Item = (String, usize, ParameterValue)>>(iter: I) -> Self {
        let mut inner: HashMap<String, Vec<(usize, ParameterValue)>> = HashMap::new();
        for (name, index, value) in iter {
            let entries = inner.entry(name).or_default();
            if let Some(existing) = entries.iter_mut().find(|(i, _)| *i == index) {
                existing.1 = value;
            } else {
                entries.push((index, value));
            }
        }
        Self(inner)
    }
}
