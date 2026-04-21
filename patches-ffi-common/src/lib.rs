pub mod arc_table;
pub mod json;
pub mod types;

// Re-exports of the audio-thread parameter data plane, now owned by
// `patches-core` so the `Module` trait can name `ParamView` in its
// signature without a crate cycle.
pub use patches_core::ids;
pub use patches_core::param_frame;
pub use patches_core::param_layout;
pub use patches_core::ids::{FloatBufferId, SongDataId};
pub use types::*;
