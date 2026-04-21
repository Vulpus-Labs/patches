//! Reading a `Float` slot as `i64` must not compile.

use patches_core::param_frame::ParamView;
use patches_core::params::FloatParamName;

mod params {
    use super::FloatParamName;
    pub const DRY_WET: FloatParamName = FloatParamName::new("dry_wet");
}

fn probe(view: &ParamView<'_>) {
    let _: i64 = view.get(params::DRY_WET);
}

fn main() {}
