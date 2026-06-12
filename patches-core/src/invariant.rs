//! Sanctioned mechanism for documented-invariant panics (ticket 1000).
//!
//! CLAUDE.md bans plain `unwrap()` / `expect()` in library code. A small
//! residue of call sites unwrap a value that a *documented invariant*
//! guarantees is present — a planner-time type check, a just-verified
//! non-emptiness, a counter that cannot realistically overflow. Those use
//! [`ExpectInvariant::expect_invariant`] instead, so:
//!
//! - a grep for `\.unwrap(` / `\.expect(` returns only genuine policy
//!   violations, and
//! - the sanctioned sites stay greppable as `expect_invariant`, each
//!   carrying a message explaining why the invariant holds.
//!
//! Use it only where the `None` / `Err` branch is genuinely unreachable
//! given an upstream guarantee. Where the failure is actually reachable
//! (I/O, OS resources, user input), propagate an error instead.

use std::fmt::Debug;

/// Unwrap a value guaranteed by a documented invariant, panicking with a
/// descriptive message if that invariant is violated (which would be a
/// bug, not an environmental failure).
pub trait ExpectInvariant<T> {
    /// Unwrap `self`, panicking with `msg` if the invariant does not hold.
    fn expect_invariant(self, msg: &str) -> T;
}

impl<T> ExpectInvariant<T> for Option<T> {
    #[inline]
    #[track_caller]
    fn expect_invariant(self, msg: &str) -> T {
        match self {
            Some(v) => v,
            None => panic!("invariant violated: {msg}"),
        }
    }
}

impl<T, E: Debug> ExpectInvariant<T> for Result<T, E> {
    #[inline]
    #[track_caller]
    fn expect_invariant(self, msg: &str) -> T {
        match self {
            Ok(v) => v,
            Err(e) => panic!("invariant violated: {msg}: {e:?}"),
        }
    }
}
