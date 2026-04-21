//! Passing a `*ParamArray` directly (no `.at(i)`) must not compile.

use patches_core::param_frame::ParamView;
use patches_core::params::FloatParamArray;

mod params {
    use super::FloatParamArray;
    pub const GAIN: FloatParamArray = FloatParamArray::new("gain");
}

fn probe(view: &ParamView<'_>) {
    let _ = view.get(params::GAIN);
}

fn main() {}
