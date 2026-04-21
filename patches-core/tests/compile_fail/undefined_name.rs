//! Referencing a `params::` const that does not exist must not compile.

use patches_core::param_frame::ParamView;

mod params {
    // Intentionally empty — `not_a_real_param` is undefined.
}

fn probe(view: &ParamView<'_>) {
    let _ = view.get(params::not_a_real_param);
}

fn main() {}
