/// Test support utilities for module unit tests.
///
/// Available under the `test-support` Cargo feature, and always available in
/// `#[cfg(test)]` builds.
pub mod harness;
pub mod macros;

pub use harness::ModuleHarness;
pub use macros::IntoParameterValue;
pub use macros::{assert_attenuated, assert_nearly, assert_passes, assert_within, params};
