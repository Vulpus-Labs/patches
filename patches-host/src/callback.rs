//! Marker trait for a host's audio callback.
//!
//! Two very different shapes need to plug in here:
//! - **patches-player**: pushes pulled samples to a cpal output stream;
//!   the callback structure lives in `patches-cpal::PatchEngine`.
//! - **patches-clap**: a sample-accurate loop that walks the host's
//!   transport and event lists alongside ticking the processor.
//!
//! Their only common surface is "owns a [`PatchProcessor`] and a plan
//! consumer; pulls plans before each buffer". This trait records that
//! contract so the runtime knows what it is handing audio endpoints to,
//! but does not (yet) prescribe a uniform `process` signature — that
//! would freeze a shape we know is going to bend under 0517 / 0518.

use patches_engine::PatchProcessor;
use patches_planner::ExecutionPlan;
use rtrb::Consumer;

/// Implemented by a host's audio callback.
///
/// Hosts call [`install`](Self::install) once at activation time with
/// the endpoints obtained from
/// [`HostRuntime::take_audio_endpoints`](crate::HostRuntime::take_audio_endpoints).
pub trait HostAudioCallback {
    fn install(
        &mut self,
        processor: PatchProcessor,
        plan_rx: Consumer<ExecutionPlan>,
    );
}
