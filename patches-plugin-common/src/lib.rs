//! GUI-toolkit-agnostic state shared by Patches plugin crates.
//!
//! [`Controller`] owns the persistable + derived plugin model;
//! [`GuiSnapshot`] is the webview projection produced by
//! [`Controller::snapshot`].

pub mod controller;
pub mod gui;
pub mod host_control_registry;
pub mod meter;

pub use controller::{
    default_preset_library_dir, xdg_list_presets, xdg_load_preset, xdg_preset_path,
    xdg_save_preset, Action, CompileFailure, CompileSuccess, Controller, Env, PatchIdentity,
    PersistedSettings, PresetEnvelope, RescanProbe, ScanDetails, SidecarEnvelope, StateDelta,
    PRESET_SCHEMA_VERSION, SIDECAR_SCHEMA_VERSION,
};

pub use gui::{
    DiagnosticView, GuiSnapshot, Intent, TapDisplayOpts, TapFrame, TapSlotFrame, TapSummary,
    STATUS_LOG_CAPACITY,
};
pub use meter::MeterTap;
