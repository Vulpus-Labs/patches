//! Host-control event + lane-kind types (ADR 0057, ADR 0068 §2 amended
//! 2026-05-05; ticket 0817).
//!
//! Lifted out of the per-block scratch implementation so plugin / engine
//! code can construct events without depending on the engine crate.
//! The scratch pipeline itself lives on `PatchProcessor`.

/// One host-control automation event.
///
/// `channel` is the host-control lane (`0..MAX_HOST_CONTROLS`) — the
/// scratch row this event lands on, derived at plan time from the
/// CLAP `param_id → channel` map.
///
/// `sample_offset` is the in-block sample index where the event takes
/// effect (`0..block_size`).
///
/// `value` is the knob / slider / toggle target, or any non-zero
/// placeholder for trigger (the row is always stamped with `1.0`
/// regardless).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HostControlEvent {
    pub channel: u8,
    pub sample_offset: u16,
    pub value: f32,
}

/// Per-lane behaviour selector for the per-block scratch pipeline. The
/// engine's smoothing / fill code branches on this; the manifest enum
/// (`patches_dsl::host_control_manifest::HostControlKind`) maps to one
/// of these at plan time.
///
/// - `Smoothed`: knob, slider — step-fill from tail, then one-pole
///   smoothing in place.
/// - `Latched`: toggle — step-fill from tail, no smoothing.
/// - `Impulse`: trigger — zero-fill, stamp `1.0` at each event sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HostControlLaneKind {
    #[default]
    Smoothed,
    Latched,
    Impulse,
}

/// Audio-thread-bound bundle of host-control plan-side state (ticket
/// 0817). Held inside [`HostControlForAudio::Present`] when the patch
/// declares any host controls; absent (`HostControlForAudio::None`)
/// otherwise.
///
/// Constructed in one shot by the host plugin's `assemble` step: the
/// `kinds` table is moved out of [`crate::HostControlLanes`]-style
/// intermediate output from `patches-host`; `routing` is appended by the
/// host plugin's automation surface (CLAP, AU, etc.) — empty `Vec` for
/// hosts with no automation.
pub struct HostControlPlanMeta {
    /// Per-channel lane kind. Indexed by backplane slot. Channels not
    /// claimed by the manifest keep their `Smoothed` default and never
    /// receive events, so the value is benign.
    pub kinds: [HostControlLaneKind; super::MAX_HOST_CONTROLS],
    /// External-automation `param_id → channel` table. Linear-scanned
    /// per event on the audio thread; ≤ `MAX_HOST_CONTROLS` entries.
    /// Empty for hosts without an automation surface.
    pub routing: Vec<(u32, u8)>,
}

/// Host-control attachment carried inside the audio-thread `AdoptionMessage`.
/// `None` means the patch declared no host controls (and the unreachable
/// "routing without channels" state is structurally impossible).
pub enum HostControlForAudio {
    None,
    Present(Box<HostControlPlanMeta>),
}

impl HostControlLaneKind {
    #[inline]
    pub fn smoothed(self) -> bool {
        matches!(self, Self::Smoothed)
    }

    /// True when an event on this lane stamps a one-sample impulse
    /// rather than a carry-forward step.
    #[inline]
    pub fn impulse(self) -> bool {
        matches!(self, Self::Impulse)
    }
}
