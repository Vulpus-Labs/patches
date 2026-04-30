//! GUI-toolkit-agnostic state shared by Patches plugin crates.
//!
//! [`Controller`] owns the persistable + derived plugin model;
//! [`GuiSnapshot`] is the webview projection produced by
//! [`Controller::snapshot`].

pub mod controller;
pub mod gui;
pub mod meter;

pub use controller::{
    Action, CompileFailure, CompileSuccess, Controller, Env, RescanProbe, ScanDetails,
    SerializedState, StateDelta,
};

pub use gui::{
    DiagnosticView, GuiSnapshot, Intent, TapDisplayOpts, TapFrame, TapSlotFrame, TapSummary,
    STATUS_LOG_CAPACITY,
};
pub use meter::MeterTap;
