//! The [`Expander`] struct and its methods.
//!
//! Methods are spread across sibling files by concern (see ADR 0041):
//! - [`frame`] — [`frame::BodyFrame`] bundle threaded through per-body passes.
//! - [`passes`] — body walker and the four free translator passes.
//! - [`template`] — template instantiation and argument binding.
//! - [`emit`] — connection flattening and scale composition at emit time.
//!
//! Stateless substitution helpers live next door in `super::substitute`.

use std::collections::{HashMap, HashSet};

use crate::ast::{Span, Template};
use crate::structural::StructuralCode as Code;

use super::ExpandError;

mod emit;
mod frame;
mod passes;
mod template;

/// Carries the immutable template table and the recursion guard used across
/// the recursive descent. Per-body state (alias maps, body accumulators)
/// lives on the stack frame of `expand_body`, not on the expander.
pub(super) struct Expander<'a> {
    pub(super) templates: &'a HashMap<&'a str, &'a Template>,
    pub(super) call_stack: HashSet<String>,
}

impl<'a> Expander<'a> {
    pub(super) fn new(templates: &'a HashMap<&'a str, &'a Template>) -> Self {
        Self {
            templates,
            call_stack: HashSet::new(),
        }
    }
}

/// Recursion-guard for template instantiation.
///
/// On `push`: errors if `type_name` is already on the call stack, otherwise
/// inserts it. On drop: removes it. Combines the contains-check, insert, and
/// remove around the recursive body expansion into one RAII object so `?`
/// inside the guarded region unwinds correctly.
pub(super) struct CallGuard<'e, 'a> {
    pub(super) expander: &'e mut Expander<'a>,
    type_name: String,
}

impl<'e, 'a> CallGuard<'e, 'a> {
    pub(super) fn push(
        expander: &'e mut Expander<'a>,
        type_name: &str,
        span: Span,
    ) -> Result<Self, ExpandError> {
        if expander.call_stack.contains(type_name) {
            return Err(ExpandError::new(
                Code::RecursiveTemplate,
                span,
                format!("recursive template instantiation: '{}'", type_name),
            ));
        }
        expander.call_stack.insert(type_name.to_owned());
        Ok(Self { expander, type_name: type_name.to_owned() })
    }
}

impl Drop for CallGuard<'_, '_> {
    fn drop(&mut self) {
        self.expander.call_stack.remove(self.type_name.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_expander() -> Expander<'static> {
        // Leak a fresh empty template table so the returned Expander is 'static
        // for the test's scope. Tests don't instantiate templates; the table
        // just needs to exist.
        let templates: &'static HashMap<&'static str, &'static Template> =
            Box::leak(Box::new(HashMap::new()));
        Expander::new(templates)
    }

    // ── CallGuard ─────────────────────────────────────────────────────────────

    #[test]
    fn call_guard_inserts_on_push_and_removes_on_drop() {
        let mut exp = mk_expander();
        {
            let guard = CallGuard::push(&mut exp, "Osc", Span::synthetic()).unwrap();
            assert!(guard.expander.call_stack.contains("Osc"));
        }
        assert!(!exp.call_stack.contains("Osc"));
    }

    #[test]
    fn call_guard_rejects_recursive_template_without_mutating_stack() {
        let mut exp = mk_expander();
        exp.call_stack.insert("Osc".to_owned());
        let before: HashSet<String> = exp.call_stack.clone();

        let err = match CallGuard::push(&mut exp, "Osc", Span::synthetic()) {
            Ok(_) => panic!("recursive push should fail"),
            Err(e) => e,
        };
        assert_eq!(err.code, Code::RecursiveTemplate);
        assert_eq!(exp.call_stack, before, "failed push must not alter call_stack");
    }

    #[test]
    fn call_guard_unwinds_on_early_return_path() {
        fn body(exp: &mut Expander) -> Result<(), ExpandError> {
            let _guard = CallGuard::push(exp, "Filter", Span::synthetic())?;
            Err(ExpandError::other(Span::synthetic(), "boom"))
        }

        let mut exp = mk_expander();
        let err = body(&mut exp).unwrap_err();
        assert_eq!(err.message, "boom");
        assert!(
            !exp.call_stack.contains("Filter"),
            "guard must drop and remove the type even on error return"
        );
    }

    #[test]
    fn call_guard_nested_pushes_and_pops_in_order() {
        let mut exp = mk_expander();
        let g1 = CallGuard::push(&mut exp, "A", Span::synthetic()).unwrap();
        {
            let g2 = CallGuard::push(g1.expander, "B", Span::synthetic()).unwrap();
            assert!(g2.expander.call_stack.contains("A"));
            assert!(g2.expander.call_stack.contains("B"));
        }
        assert!(g1.expander.call_stack.contains("A"));
        assert!(!g1.expander.call_stack.contains("B"));
    }
}
