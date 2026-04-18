//! `patches-vintage` — vintage-style BBD effects.
//!
//! Houses a reusable BBD primitive (Holters & Parker, DAFx-18) plus
//! modules built on top of it (currently [`vchorus::VChorus`]). Also
//! ships an NE570-style compander primitive for future BBD-delay and
//! Dimension-D-style modules.
//!
//! `patches_modules::default_registry()` calls [`register`] at the end,
//! so consumers pick up every module in this crate through the default
//! registry with no DSL-surface change. A later epic converts this crate
//! into an FFI plugin bundle per ADR 0039 / E088.

pub mod bbd;
pub mod compander;
pub mod vchorus;

pub use vchorus::VChorus;

/// Register every module in this crate with the supplied registry.
pub fn register(r: &mut patches_registry::Registry) {
    r.register::<VChorus>();
}
