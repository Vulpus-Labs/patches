//! The [`Expander`] struct and its methods.
//!
//! Methods are spread across sibling files by concern (see ADR 0041):
//! - [`substitute`] — stateless param/value substitution helpers.
//! - [`passes`] — body walker and the four per-body passes.
//! - [`template`] — template instantiation and argument binding.
//! - [`emit`] — connection flattening and scale composition at emit time.

use std::collections::{HashMap, HashSet};

use crate::ast::Template;

mod emit;
mod passes;
mod substitute;
mod template;

/// Carries the immutable template table, the mutable recursion guard, and the
/// alias-map scope used across the recursive descent.
pub(super) struct Expander<'a> {
    pub(super) templates: &'a HashMap<&'a str, &'a Template>,
    pub(super) call_stack: HashSet<String>,
    /// instance_name → { alias_name → integer index }
    ///
    /// Built during pass 1 of `expand_body` from `AliasList` shape args.
    /// Each `module M : Type(port: [a, b, c])` registers aliases a→0, b→1, c→2
    /// under the key "M" (or the qualified name for nested templates).
    pub(super) alias_maps: HashMap<String, HashMap<String, u32>>,
}

impl<'a> Expander<'a> {
    pub(super) fn new(templates: &'a HashMap<&'a str, &'a Template>) -> Self {
        Self {
            templates,
            call_stack: HashSet::new(),
            alias_maps: HashMap::new(),
        }
    }
}
