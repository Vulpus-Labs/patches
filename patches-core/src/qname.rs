//! Structured qualified identifier (see ADR 0034).
//!
//! A [`QName`] carries a namespace path (outermost → innermost) alongside a
//! bare name, replacing the former slash-joined `String` representation used
//! throughout the DSL pipeline.

use std::cmp::Ordering;
use std::fmt;

/// A qualified identifier: an optional chain of namespace segments plus a
/// final bare name.
///
/// Constructed via [`QName::bare`] for top-level names and extended via
/// [`QName::child`] when entering a nested scope (template expansion, song
/// section, …).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QName {
    pub path: Vec<String>,
    pub name: String,
}

impl QName {
    /// Build a bare (unqualified) name.
    pub fn bare(name: impl Into<String>) -> Self {
        Self { path: Vec::new(), name: name.into() }
    }

    /// Extend `self` with an inner name, producing a new [`QName`] whose
    /// path is `self.path ++ [self.name]` and whose bare name is `name`.
    pub fn child(&self, name: impl Into<String>) -> Self {
        let mut path = self.path.clone();
        path.push(self.name.clone());
        Self { path, name: name.into() }
    }

    /// True iff this name has no enclosing scope.
    pub fn is_bare(&self) -> bool {
        self.path.is_empty()
    }
}

impl From<&str> for QName {
    fn from(s: &str) -> Self {
        Self::bare(s)
    }
}

impl From<String> for QName {
    fn from(s: String) -> Self {
        Self::bare(s)
    }
}

impl fmt::Display for QName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for seg in &self.path {
            f.write_str(seg)?;
            f.write_str("/")?;
        }
        f.write_str(&self.name)
    }
}

impl PartialOrd for QName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QName {
    fn cmp(&self, other: &Self) -> Ordering {
        for (a, b) in self.path.iter().zip(other.path.iter()) {
            match a.as_str().cmp(b.as_str()) {
                Ordering::Equal => continue,
                ord => return ord,
            }
        }
        match self.path.len().cmp(&other.path.len()) {
            Ordering::Less => self.name.as_str().cmp(
                other.path.get(self.path.len()).map(String::as_str).unwrap_or(""),
            ).then(Ordering::Less),
            Ordering::Greater => self.path.get(other.path.len()).map(String::as_str).unwrap_or("")
                .cmp(other.name.as_str()).then(Ordering::Greater),
            Ordering::Equal => self.name.as_str().cmp(other.name.as_str()),
        }
    }
}

impl QName {
    /// Compare this `QName` to a slash-joined string without allocating.
    fn eq_display(&self, other: &str) -> bool {
        let mut rest = other;
        for seg in &self.path {
            if !rest.starts_with(seg.as_str()) {
                return false;
            }
            rest = &rest[seg.len()..];
            if !rest.starts_with('/') {
                return false;
            }
            rest = &rest[1..];
        }
        rest == self.name
    }
}

impl PartialEq<str> for QName {
    fn eq(&self, other: &str) -> bool {
        self.eq_display(other)
    }
}

impl PartialEq<&str> for QName {
    fn eq(&self, other: &&str) -> bool {
        self.eq_display(other)
    }
}

impl PartialEq<String> for QName {
    fn eq(&self, other: &String) -> bool {
        self.eq_display(other.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_has_empty_path() {
        let q = QName::bare("osc");
        assert!(q.is_bare());
        assert_eq!(q.path, Vec::<String>::new());
        assert_eq!(q.name, "osc");
        assert_eq!(q.to_string(), "osc");
    }

    #[test]
    fn child_extends_path() {
        let outer = QName::bare("v1");
        let inner = outer.child("osc");
        assert!(!inner.is_bare());
        assert_eq!(inner.path, vec!["v1".to_string()]);
        assert_eq!(inner.name, "osc");
        assert_eq!(inner.to_string(), "v1/osc");
    }

    #[test]
    fn nested_child() {
        let q = QName::bare("a").child("b").child("c");
        assert_eq!(q.path, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(q.name, "c");
        assert_eq!(q.to_string(), "a/b/c");
    }

    #[test]
    fn equality_is_structural() {
        assert_eq!(QName::bare("x"), QName::bare("x"));
        assert_ne!(QName::bare("x"), QName::bare("y"));
        assert_ne!(QName::bare("a").child("b"), QName::bare("a/b"));
    }

    #[test]
    fn ordering_matches_display() {
        let mut names = [
            QName::bare("zeta"),
            QName::bare("alpha").child("x"),
            QName::bare("alpha"),
        ];
        names.sort();
        let rendered: Vec<String> = names.iter().map(|q| q.to_string()).collect();
        assert_eq!(rendered, vec!["alpha", "alpha/x", "zeta"]);
    }

    #[test]
    fn string_equality() {
        let q = QName::bare("v").child("osc");
        assert!(q == *"v/osc");
        assert!(q == "v/osc");
    }
}
