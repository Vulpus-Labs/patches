pub mod approximate;
pub mod delay_buffer;
pub mod frequency;
pub mod param_access;
pub mod phase_accumulator;

pub use delay_buffer::{DelayBuffer, PolyDelayBuffer, PolyThiranInterp, ThiranInterp};
pub use approximate::fast_exp2;
pub use patches_dsp::TapFeedbackFilter;
pub use patches_dsp::ToneFilter;
pub use patches_dsp::MonoBiquad;
#[allow(unused_imports)]
pub(crate) use patches_dsp::PolyBiquad;
