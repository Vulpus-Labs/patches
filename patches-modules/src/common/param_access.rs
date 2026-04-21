//! Shared accessors for `ParamView` with typed fallback signature.
//!
//! Post-Spike-5 the underlying frame is always fully packed (defaults already
//! filled on the control thread), so the `default` argument is no longer
//! used at runtime — the accessor just forwards to the typed `ParamView`
//! method. The signature is preserved so call sites don't need mechanical
//! rewrites beyond the trait flip.

use patches_core::param_frame::ParamView;
use patches_core::parameter_map::ParameterKey;

#[inline]
pub fn get_float(params: &ParamView<'_>, name: &str, index: usize, _default: f32) -> f32 {
    params.float(ParameterKey::new(name, index))
}

#[inline]
pub fn get_int(params: &ParamView<'_>, name: &str, index: usize, _default: i64) -> i64 {
    params.int(ParameterKey::new(name, index))
}

#[inline]
pub fn get_bool(params: &ParamView<'_>, name: &str, index: usize, _default: bool) -> bool {
    params.bool(ParameterKey::new(name, index))
}

#[inline]
pub fn get_enum(params: &ParamView<'_>, name: &str, index: usize) -> u32 {
    params.enum_variant(ParameterKey::new(name, index))
}
