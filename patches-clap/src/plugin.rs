//! Core plugin struct and CLAP plugin vtable.
//!
//! `PatchesClapPlugin` holds the Patches engine state and implements
//! the CLAP plugin callbacks: init, activate, start/stop processing,
//! process, and extension queries.

use std::collections::VecDeque;
use std::ffi::{c_char, c_void};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Log a message to ~/patches-clap-debug.log for crash diagnosis.
macro_rules! dlog {
    ($($arg:tt)*) => {{
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(concat!(env!("HOME"), "/patches-clap-debug.log"))
        {
            let _ = writeln!(f, $($arg)*);
        }
    }};
}

use clap_sys::events::{
    clap_event_midi, clap_event_transport, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI,
    CLAP_TRANSPORT_HAS_BEATS_TIMELINE, CLAP_TRANSPORT_HAS_TEMPO,
    CLAP_TRANSPORT_HAS_TIME_SIGNATURE, CLAP_TRANSPORT_IS_PLAYING,
};
use clap_sys::fixedpoint::CLAP_BEATTIME_FACTOR;
use clap_sys::host::clap_host;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::{
    clap_process, clap_process_status, CLAP_PROCESS_CONTINUE,
};

use patches_core::{AudioEnvironment, MidiEvent, BASE_PERIODIC_UPDATE_INTERVAL};
use patches_observation::{spawn_observer, tap_ring, ObserverHandle};
use patches_observation::subscribers::{DiagnosticReader, SubscribersHandle};
use patches_registry::Registry;
use patches_host::AdoptionMessage;
use patches_engine::PatchProcessor;
use patches_host::{HostBuilder, HostRuntime, InMemorySource};

use crate::extensions;
use patches_dsl::manifest::{Manifest, TapDescriptor};
use patches_plugin_common::{
    xdg_list_presets, xdg_load_preset, xdg_preset_path, xdg_save_preset, Action, CompileFailure,
    CompileSuccess, Controller, DiagnosticView, Env, MeterTap, PersistedSettings, RescanProbe,
    ScanDetails, StateDelta, TapSummary,
};

/// The runtime state of a single plugin instance.
///
/// Allocated in `create_plugin`, freed in `destroy`.
/// The `clap_plugin` struct lives in a separate heap allocation whose
/// `plugin_data` field points here.
#[allow(dead_code)] // host + request_callback used once GUI triggers callbacks
pub struct PatchesClapPlugin {
    /// Host reference — used for `request_callback`.
    pub(crate) host: *const clap_host,

    // ── Audio-thread state ──────────────────────────────────────────
    /// Taken out of [`HostRuntime`] at activate time so the CLAP audio
    /// callback can drive it.
    pub(crate) processor: Option<PatchProcessor>,
    pub(crate) plan_rx: Option<rtrb::Consumer<AdoptionMessage>>,

    // ── Main-thread state ───────────────────────────────────────────
    /// Owns the planner, plan-tx producer, cleanup thread and audio env.
    /// `None` until [`activate`](plugin_activate); reset on `deactivate`.
    pub(crate) runtime: Option<HostRuntime>,

    // ── DSL state ───────────────────────────────────────────────────
    /// Canonical owner of `dsl_source`, `module_paths`, `file_path`,
    /// `tap_opts`, `module_names`, `taps`, `status_log`,
    /// `diagnostic_view`, and the live `Registry` (ADR 0061, tickets
    /// 0758–0761). Webview reads via `Controller::snapshot`.
    pub(crate) controller: Controller,
    /// Action queue drained on each `on_main_thread` tick. Webview IPC
    /// pushes; main thread drains under `Controller::apply` (ADR 0061).
    pub(crate) action_queue: Arc<Mutex<VecDeque<Action>>>,

    // ── GUI ─────────────────────────────────────────────────────────
    /// Clonable handle for polling engine halt state (ADR 0051). Populated
    /// in `activate` from the processor.
    pub(crate) halt_handle: Option<patches_engine::HaltHandle>,
    /// Observer thread handle. Started in `activate`, joined in `deactivate`.
    pub(crate) observer: Option<ObserverHandle>,
    /// Reader handle into the observer's atomic-scalar tap surface
    /// (ADR 0053 §7). Cloned for the GUI's main-thread tap pump.
    pub(crate) subscribers: Option<SubscribersHandle>,
    /// Observer-side diagnostic ring reader. Drained on `on_main_thread`
    /// and surfaced through the controller status log (ticket 0725).
    pub(crate) diagnostics: Option<DiagnosticReader>,
    pub(crate) gui_handle: Option<crate::gui::WebviewGuiHandle>,
    pub(crate) gui_scale: f64,
    /// Lock-free master-output meter tap. Audio thread writes, GUI reads.
    pub(crate) meter: Arc<MeterTap>,

    pub(crate) sample_rate: f64,

    // ── Transport edge detection ───────────────────────────────────
    /// Previous beat position, used to detect beat boundary crossings.
    pub(crate) prev_beat: f64,
    /// Previous bar number, used to detect bar boundary crossings.
    pub(crate) prev_bar: i32,
}

// Safety: PatchesClapPlugin is only accessed according to CLAP's
// threading rules — main-thread fields on the main thread, audio-thread
// fields on the audio thread. The only cross-thread shared state is
// `action_queue` (behind Arc<Mutex>).
unsafe impl Send for PatchesClapPlugin {}

impl PatchesClapPlugin {
    /// Request the host to call `on_main_thread` at its earliest convenience.
    #[allow(dead_code)] // will be used by the GUI to trigger main-thread work
    pub(crate) fn request_callback(&self) {
        unsafe {
            if let Some(f) = (*self.host).request_callback {
                f(self.host);
            }
        }
    }

    /// Ask the host to deactivate + reactivate this plugin. Used to
    /// trigger the hard-stop rescan flow (ADR 0044 §3): host drives the
    /// stop, and `activate` rebuilds the registry from `module_paths`
    /// and recompiles `dsl_source`.
    pub(crate) fn request_restart(&self) {
        if self.host.is_null() {
            return;
        }
        unsafe {
            if let Some(f) = (*self.host).request_restart {
                f(self.host);
            }
        }
    }

    /// Tell the host the plugin's persistable state has changed since the
    /// last `state.save`. Hosts use this to enable Save / prompt-on-close
    /// (ADR-relevant: ticket 0566 persists `module_paths`; loading a new
    /// patch and editing module paths both flip persistable state).
    pub(crate) fn mark_state_dirty(&self) {
        use clap_sys::ext::state::{clap_host_state, CLAP_EXT_STATE};
        if self.host.is_null() {
            return;
        }
        unsafe {
            let get_ext = match (*self.host).get_extension {
                Some(f) => f,
                None => return,
            };
            let raw = get_ext(self.host, CLAP_EXT_STATE.as_ptr());
            if raw.is_null() {
                return;
            }
            let ext = &*(raw as *const clap_host_state);
            if let Some(f) = ext.mark_dirty {
                f(self.host);
            }
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Recover a shared reference to the plugin from a raw `clap_plugin` pointer.
///
/// # Safety
/// The caller must ensure `plugin` is non-null and that `plugin_data`
/// points to a valid `PatchesClapPlugin`.
unsafe fn plugin_ref<'a>(plugin: *const clap_plugin) -> &'a PatchesClapPlugin {
    &*((*plugin).plugin_data as *const PatchesClapPlugin)
}

/// Recover an exclusive reference to the plugin from a raw `clap_plugin` pointer.
///
/// # Safety
/// The caller must ensure `plugin` is non-null, that `plugin_data`
/// points to a valid `PatchesClapPlugin`, and that no other reference
/// to the plugin is live.
unsafe fn plugin_mut<'a>(plugin: *const clap_plugin) -> &'a mut PatchesClapPlugin {
    &mut *((*plugin).plugin_data as *mut PatchesClapPlugin)
}

// ── Public accessors for use by extensions ──────────────────────────

/// Recover a shared reference to the plugin — for use by extension callbacks.
///
/// # Safety
/// Same as `plugin_ref`.
pub(crate) unsafe fn plugin_ref_pub<'a>(plugin: *const clap_plugin) -> &'a PatchesClapPlugin {
    plugin_ref(plugin)
}

/// Recover an exclusive reference to the plugin — for use by extension callbacks.
///
/// # Safety
/// Same as `plugin_mut`.
pub(crate) unsafe fn plugin_mut_pub<'a>(plugin: *const clap_plugin) -> &'a mut PatchesClapPlugin {
    plugin_mut(plugin)
}

// ── Vtable constructor ──────────────────────────────────────────────

/// Build a `clap_plugin` struct populated with our vtable function pointers.
pub(crate) fn make_clap_plugin(
    desc: *const clap_plugin_descriptor,
    _host: *const clap_host,
    data: *mut PatchesClapPlugin,
) -> clap_plugin {
    clap_plugin {
        desc,
        plugin_data: data as *mut c_void,
        init: Some(plugin_init),
        destroy: Some(plugin_destroy),
        activate: Some(plugin_activate),
        deactivate: Some(plugin_deactivate),
        start_processing: Some(plugin_start_processing),
        stop_processing: Some(plugin_stop_processing),
        reset: Some(plugin_reset),
        process: Some(plugin_process),
        get_extension: Some(plugin_get_extension),
        on_main_thread: Some(plugin_on_main_thread),
    }
}

// ── Vtable callbacks ────────────────────────────────────────────────

unsafe extern "C" fn plugin_init(_plugin: *const clap_plugin) -> bool {
    dlog!("init");
    true
}

unsafe extern "C" fn plugin_destroy(plugin: *const clap_plugin) {
    dlog!("destroy");
    let data = (*plugin).plugin_data as *mut PatchesClapPlugin;
    // Drop the plugin data first.
    drop(Box::from_raw(data));
    // Then drop the clap_plugin struct itself.
    drop(Box::from_raw(plugin as *mut clap_plugin));
}

unsafe extern "C" fn plugin_activate(
    plugin: *const clap_plugin,
    sample_rate: f64,
    _min_frames_count: u32,
    _max_frames_count: u32,
) -> bool {
    dlog!("activate: sr={sample_rate}");

    // If already active (e.g. sample-rate change), deactivate first.
    // Check into a bool before calling deactivate so we don't hold
    // two &mut references simultaneously.
    let already_active = plugin_mut(plugin).processor.is_some();
    if already_active {
        dlog!("activate: already active, deactivating first");
        plugin_deactivate(plugin);
    }
    let p = plugin_mut(plugin);

    p.sample_rate = sample_rate;

    let env = AudioEnvironment {
        sample_rate: sample_rate as f32,
        poly_voices: 16,
        periodic_update_interval: BASE_PERIODIC_UPDATE_INTERVAL,
        hosted: true,
    };

    let mut runtime = match HostBuilder::new().build(env) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("patches-clap: failed to build host runtime: {e}");
            return false;
        }
    };

    // Stand up the observer thread + tap ring before taking the audio
    // endpoints, so the planner's manifest publication reaches the
    // observer (ADR 0056). Mirrors `patches-player::run_tui`.
    let (tap_tx, tap_rx) = tap_ring(64);
    let (mut observer, diag_rx) = spawn_observer(tap_rx, std::time::Duration::from_millis(2));
    if let Some(replans) = observer.take_replans() {
        runtime.attach_observer(replans);
    } else {
        eprintln!("patches-clap: observer replan producer missing");
    }
    let subs_handle = observer.subscribers.clone();

    let (mut processor, plan_rx) = match runtime.take_audio_endpoints() {
        Some(pair) => pair,
        None => {
            eprintln!("patches-clap: host runtime missing audio endpoints");
            return false;
        }
    };
    processor.set_tap_producer(Some(tap_tx));
    p.halt_handle = Some(processor.halt_handle());
    p.processor = Some(processor);
    p.plan_rx = Some(plan_rx);
    p.runtime = Some(runtime);
    p.observer = Some(observer);
    p.subscribers = Some(subs_handle);
    p.diagnostics = Some(diag_rx);

    // Rebuild registry + recompile via the controller. Action::Activate
    // performs the scan and (if dsl_source non-empty) compile.
    apply_action(p, Action::Activate);
    dlog!("activate: module scan {}", p
        .controller
        .status_log
        .iter()
        .rev()
        .find(|s| s.starts_with("Module scan:"))
        .cloned()
        .unwrap_or_default());

    // Immediately adopt any pending plan so audio starts right away.
    if !p.controller.dsl_source.is_empty() {
        if let Some(rx) = &mut p.plan_rx {
            if let Ok((plan, meta)) = rx.pop() {
                if let Some(proc) = &mut p.processor {
                    proc.adopt_plan_with_meta(plan, meta.map(|b| *b));
                }
            }
        }
    }

    true
}

unsafe extern "C" fn plugin_deactivate(plugin: *const clap_plugin) {
    dlog!("deactivate");
    let p = plugin_mut(plugin);

    // Drop the audio-thread endpoints first (releasing the cleanup_tx
    // producer the processor holds), then the runtime — its `Drop` joins
    // the cleanup thread.
    p.plan_rx.take();
    p.processor.take();
    p.halt_handle = None;
    p.runtime.take();
    p.subscribers = None;
    p.diagnostics = None;
    if let Some(obs) = p.observer.take() {
        obs.stop();
    }

    // Lower to controller after audio-side teardown so the derived
    // model (taps, registry, halt) matches the now-inert engine.
    apply_action(p, Action::Deactivate);
}

unsafe extern "C" fn plugin_start_processing(_plugin: *const clap_plugin) -> bool {
    dlog!("start_processing");
    true
}

unsafe extern "C" fn plugin_stop_processing(_plugin: *const clap_plugin) {
    dlog!("stop_processing");
}

unsafe extern "C" fn plugin_reset(_plugin: *const clap_plugin) {
    dlog!("reset");
    // reset is called on the audio thread — must not block or allocate.
    // The processor's internal state (cable buffers, module pool) is
    // already valid; the next plan adoption will bring it up to date.
    // Nothing to do here.
}

/// Logged once so we know process was reached without flooding the log.
/// `OnceLock` makes the "first call" semantics explicit.
static PROCESS_LOGGED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
/// Count process calls to log diagnostics after a few buffers. `Relaxed`
/// ordering is sufficient — this is a diagnostic counter with no
/// happens-before dependency on other state.
static PROCESS_COUNT: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

unsafe extern "C" fn plugin_process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    // Tag the host's audio thread on first entry (ADR 0045 spike 4).
    // Idempotent; no-op when the allocator-trap feature is off.
    patches_alloc_trap::mark_audio_thread();

    if PROCESS_LOGGED.set(()).is_ok() {
        dlog!("process: first call");
    }
    if process.is_null() {
        dlog!("process: null process ptr");
        return CLAP_PROCESS_CONTINUE;
    }
    let p = plugin_mut(plugin);
    let proc = match &mut p.processor {
        Some(proc) => proc,
        None => {
            dlog!("process: no processor");
            return CLAP_PROCESS_CONTINUE;
        }
    };

    // Adopt any pending plan.
    if let Some(rx) = &mut p.plan_rx {
        if let Ok((plan, meta)) = rx.pop() {
            dlog!("process: adopting plan, {} active modules", plan.active_indices.len());
            proc.adopt_plan_with_meta(plan, meta.map(|b| *b));
            // Reset counter so we log output levels after the new plan.
            PROCESS_COUNT.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }

    let pr = &*process;
    let frames = pr.frames_count as usize;
    if frames == 0 {
        return CLAP_PROCESS_CONTINUE;
    }

    // Read input buffer pointers (may be null if not connected).
    let (in_l, in_r) = read_input_ptrs(pr);

    // Output buffer — get the raw clap_audio_buffer and write through
    // data32 each sample (don't cache the inner pointers).
    if pr.audio_outputs_count == 0 || pr.audio_outputs.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    let out_buf = &mut *pr.audio_outputs;
    if out_buf.data32.is_null() || out_buf.channel_count < 1 {
        return CLAP_PROCESS_CONTINUE;
    }

    // Event iteration — guard against missing vtable functions.
    let in_events = pr.in_events;
    let (event_size_fn, event_get_fn) = if !in_events.is_null() {
        match ((*in_events).size, (*in_events).get) {
            (Some(s), Some(g)) => (Some(s), Some(g)),
            _ => (None, None),
        }
    } else {
        (None, None)
    };
    let event_count = event_size_fn.map_or(0, |f| f(in_events));
    let mut event_idx: u32 = 0;

    // Read host transport and write it to the processor's GLOBAL_TRANSPORT slot.
    if !pr.transport.is_null() {
        let t: &clap_event_transport = &*pr.transport;
        let playing = if t.flags & CLAP_TRANSPORT_IS_PLAYING != 0 {
            1.0
        } else {
            0.0
        };
        let tempo = if t.flags & CLAP_TRANSPORT_HAS_TEMPO != 0 {
            t.tempo as f32
        } else {
            0.0
        };
        let (beat, bar, beat_trigger, bar_trigger) =
            if t.flags & CLAP_TRANSPORT_HAS_BEATS_TIMELINE != 0 {
                let beat_f = t.song_pos_beats as f64 / CLAP_BEATTIME_FACTOR as f64;
                let bar_num = t.bar_number;
                // Detect beat boundary: integer part of beat changed.
                let beat_trig = if beat_f.floor() != p.prev_beat.floor()
                    && p.prev_beat >= 0.0
                {
                    1.0
                } else {
                    0.0
                };
                // Detect bar boundary: bar number changed.
                let bar_trig = if bar_num != p.prev_bar && p.prev_bar >= 0 {
                    1.0
                } else {
                    0.0
                };
                p.prev_beat = beat_f;
                p.prev_bar = bar_num;
                (beat_f as f32, bar_num as f32, beat_trig, bar_trig)
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };
        let (tsig_num, tsig_denom) =
            if t.flags & CLAP_TRANSPORT_HAS_TIME_SIGNATURE != 0 {
                (t.tsig_num as f32, t.tsig_denom as f32)
            } else {
                (0.0, 0.0)
            };
        proc.write_transport(
            playing, tempo, beat, bar, beat_trigger, bar_trigger, tsig_num, tsig_denom,
        );
    }

    // Meter accumulators — seed peaks from the previous block decayed.
    let (mut meter_pl, mut meter_pr) = p.meter.decayed_peaks();
    let mut meter_sq_l = 0.0f32;
    let mut meter_sq_r = 0.0f32;

    // Sample-accurate processing loop.
    for f in 0..frames {
        // Deliver MIDI events at this sample offset.
        if let Some(get_fn) = event_get_fn {
            while event_idx < event_count {
                let header = get_fn(in_events, event_idx);
                if header.is_null() {
                    event_idx += 1;
                    continue;
                }
                if (*header).time > f as u32 {
                    break;
                }
                if (*header).space_id == CLAP_CORE_EVENT_SPACE_ID
                    && (*header).type_ == CLAP_EVENT_MIDI
                {
                    let midi = &*(header as *const clap_event_midi);
                    proc.write_midi(&[MidiEvent { bytes: midi.data }]);
                }
                event_idx += 1;
            }
        }

        // Feed input.
        let il = if in_l.is_null() { 0.0 } else { *in_l.add(f) };
        let ir = if in_r.is_null() { 0.0 } else { *in_r.add(f) };
        proc.write_input(il, ir);

        // Tick the engine.
        let (ol, or) = proc.tick();

        // Meter accumulation.
        let ola = ol.abs();
        let ora = or.abs();
        if ola > meter_pl { meter_pl = ola; }
        if ora > meter_pr { meter_pr = ora; }
        meter_sq_l += ol * ol;
        meter_sq_r += or * or;

        // Write to the output buffer.
        if !out_buf.data32.is_null() {
            let ch0 = *out_buf.data32;
            if !ch0.is_null() {
                *ch0.add(f) = ol;
            }
            if out_buf.channel_count >= 2 {
                let ch1 = *out_buf.data32.add(1);
                if !ch1.is_null() {
                    *ch1.add(f) = or;
                }
            }
        }
    }

    p.meter.publish(meter_pl, meter_pr, meter_sq_l, meter_sq_r, frames);

    // Log diagnostics on the 10th buffer so we can see output levels.
    let count = PROCESS_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if count == 10 {
        let sample = if !out_buf.data32.is_null() {
            let ch0 = *out_buf.data32;
            if ch0.is_null() { 0.0 } else { *ch0 }
        } else {
            0.0
        };
        dlog!("process diag: frames={frames} out[0]={sample}");
    }

    CLAP_PROCESS_CONTINUE
}

/// Extract input f32 channel pointers, returning null for missing/invalid buffers.
unsafe fn read_input_ptrs(pr: &clap_process) -> (*const f32, *const f32) {
    if pr.audio_inputs_count == 0 || pr.audio_inputs.is_null() {
        return (std::ptr::null(), std::ptr::null());
    }
    let buf = &*pr.audio_inputs;
    if buf.data32.is_null() {
        return (std::ptr::null(), std::ptr::null());
    }
    let ch = buf.channel_count as usize;
    let l = *buf.data32;
    let r = if ch > 1 { *buf.data32.add(1) } else { l };
    (l as *const f32, r as *const f32)
}


unsafe extern "C" fn plugin_get_extension(
    _plugin: *const clap_plugin,
    id: *const c_char,
) -> *const c_void {
    extensions::get_extension(id)
}

unsafe extern "C" fn plugin_on_main_thread(plugin: *const clap_plugin) {
    dlog!("on_main_thread");
    let p = plugin_mut(plugin);

    // Poll-and-synthesise audio-thread events as actions (ADR 0061).
    // No SPSC ring back from audio: we read existing channels each tick
    // and emit actions only when something changed.
    let mut synthesised: Vec<Action> = Vec::new();
    if let Some(handle) = &p.halt_handle {
        let observed = handle.halt_info();
        let same = match (&observed, &p.controller.halt) {
            (None, None) => true,
            (Some(a), Some(b)) => a.slot == b.slot && a.module_name == b.module_name,
            _ => false,
        };
        if !same {
            synthesised.push(Action::HaltObserved(observed));
        }
    }
    if let Some(reader) = p.diagnostics.as_mut() {
        let lines: Vec<String> = reader.drain().iter().map(|d| d.render()).collect();
        if !lines.is_empty() {
            synthesised.push(Action::DiagnosticsDrained(lines));
        }
    }

    // Drain action queue and apply each through the controller. Single
    // mutation entry point per ADR 0061. Synthesised audio-thread
    // actions go first so a halt is visible before the action that
    // triggered it (e.g. Reload after a panic) tries to recover.
    let actions: Vec<Action> = {
        let mut q = p.action_queue.lock().expect("action_queue mutex poisoned");
        synthesised.into_iter().chain(q.drain(..)).collect()
    };
    let mut needs_restart = false;
    let mut needs_dirty = false;
    for action in actions {
        let delta = apply_action(p, action);
        if delta.persistable_changed {
            needs_dirty = true;
        }
        if delta.requires_restart {
            needs_restart = true;
        }
    }
    if needs_dirty {
        p.mark_state_dirty();
    }
    if needs_restart {
        p.request_restart();
    }

    if let Some(handle) = &p.gui_handle {
        let snap = p.controller.snapshot();
        handle.update(&snap);
    }

    // Push a TapFrame at most once per `TAP_PUSH_INTERVAL`. Frames flow
    // through a separate channel from `applyState` so snapshot dedupe
    // doesn't suppress live tap updates.
    if let (Some(handle), Some(subs)) = (&p.gui_handle, &p.subscribers) {
        handle.push_taps(subs, &p.controller.taps, &p.controller.tap_opts);
    }
}

/// Render per-entry detail from a [`ScanReport`] as status-log lines so
/// the user sees *why* a module failed to load (ABI mismatch, dlopen
/// error, etc.) rather than just an error count.
fn scan_detail_lines(report: &patches_ffi::ScanReport) -> Vec<String> {
    use patches_ffi::SkipReason;
    let mut out = Vec::new();
    for (path, err) in &report.errors {
        out.push(format!("  error {}: {err}", path.display()));
    }
    for skip in &report.skipped {
        match skip {
            SkipReason::AbiMismatch { expected, found, path } => out.push(format!(
                "  skip {}: ABI mismatch (host {expected}, plugin {found})",
                path.display()
            )),
            SkipReason::LowerVersion { name, existing, candidate, path } => out.push(
                format!(
                    "  skip {}: {name} v{candidate} <= existing v{existing}",
                    path.display()
                ),
            ),
            SkipReason::DuplicateInBundle { name, path } => {
                out.push(format!("  skip {}: duplicate {name} in bundle", path.display()))
            }
        }
    }
    out
}

/// Apply one [`Action`] by constructing a fresh [`ClapEnv`] view of the
/// plugin's audio-side fields and dispatching to `Controller::apply`.
pub(crate) fn apply_action(p: &mut PatchesClapPlugin, action: Action) -> StateDelta {
    // Split borrow: ClapEnv only holds the audio-side fields, so the
    // controller can be borrowed mutably alongside it.
    let PatchesClapPlugin {
        runtime,
        controller,
        ..
    } = p;
    let mut env = ClapEnv { runtime };
    controller.apply(action, &mut env)
}

/// CLAP-side `Env` impl. Holds only what the controller's handlers
/// touch — file dialogs, file I/O, and the audio-thread plan-push path.
struct ClapEnv<'a> {
    runtime: &'a mut Option<HostRuntime>,
}

impl<'a> Env for ClapEnv<'a> {
    fn pick_file(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .add_filter("Patches", &["patches"])
            .pick_file()
    }
    fn pick_folder(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }
    fn read_file(&mut self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
    fn compile_and_push_plan(
        &mut self,
        source: &str,
        file_path: Option<&Path>,
        registry: &Registry,
    ) -> Result<CompileSuccess, CompileFailure> {
        let runtime = match self.runtime.as_mut() {
            Some(r) => r,
            None => {
                return Err(CompileFailure {
                    message: "engine not activated".into(),
                    view: DiagnosticView::default(),
                })
            }
        };
        let mut src = InMemorySource::new(source.to_string());
        if let Some(path) = file_path {
            src = src.with_master_path(path.to_path_buf());
        }
        match runtime.compile_and_push(&src, registry) {
            Ok(loaded) => {
                let taps = project_manifest(&loaded.manifest);
                let warnings: Vec<_> = loaded
                    .layering_warnings
                    .iter()
                    .map(patches_diagnostics::RenderedDiagnostic::from_layering_warning)
                    .collect();
                Ok(CompileSuccess { taps, warnings })
            }
            Err(e) => {
                let view = DiagnosticView {
                    diagnostics: e.to_rendered_diagnostics(),
                    source_map: Some(e.source_map.clone()),
                };
                Err(CompileFailure {
                    message: format!("{e}"),
                    view,
                })
            }
        }
    }
    fn probe_paths(&mut self, paths: &[PathBuf], registry: &Registry) -> RescanProbe {
        let mut probe = RescanProbe::default();
        if paths.is_empty() {
            return probe;
        }
        // Scan into a throwaway registry so we read manifests without
        // perturbing the live one. The Arc<Library> handles drop with
        // the throwaway registry, releasing the dylibs.
        let mut throwaway = Registry::new();
        let report = patches_ffi::PluginScanner::new(paths.to_vec()).scan(&mut throwaway);
        let names: Vec<String> = throwaway.module_names().map(|s| s.to_string()).collect();
        for name in names {
            let candidate = throwaway.module_version(&name).unwrap_or(0);
            match registry.module_version(&name) {
                None => probe.added.push(name),
                Some(live) if live < candidate => probe.replaced.push(name),
                _ => probe.unchanged.push(name),
            }
        }
        for (path, err) in &report.errors {
            probe.errors.push(format!("  error {}: {err}", path.display()));
        }
        for skip in &report.skipped {
            if let patches_ffi::SkipReason::AbiMismatch { expected, found, path } = skip {
                probe.errors.push(format!(
                    "  skip {}: ABI mismatch (host {expected}, plugin {found})",
                    path.display()
                ));
            }
        }
        probe
    }
    fn scan_into(&mut self, paths: &[PathBuf], registry: &mut Registry) -> ScanDetails {
        scan_into_registry(paths, registry, "")
    }
    fn reset_and_scan(&mut self, paths: &[PathBuf]) -> (Registry, ScanDetails) {
        let mut registry = patches_modules::default_registry();
        let details = scan_into_registry(paths, &mut registry, "");
        (registry, details)
    }
    fn preset_path(&self, patch_stem: &str, preset_name: &str) -> Option<PathBuf> {
        xdg_preset_path(patch_stem, preset_name)
    }
    fn list_presets(&mut self, patch_stem: &str) -> Vec<String> {
        xdg_list_presets(patch_stem)
    }
    fn load_preset(&mut self, path: &Path) -> std::io::Result<Option<PersistedSettings>> {
        xdg_load_preset(path)
    }
    fn save_preset(&mut self, path: &Path, settings: &PersistedSettings) -> std::io::Result<()> {
        xdg_save_preset(path, settings)
    }
}

fn scan_into_registry(
    paths: &[PathBuf],
    registry: &mut Registry,
    empty_summary: &str,
) -> ScanDetails {
    let (summary, details) = if paths.is_empty() {
        (empty_summary.to_string(), Vec::new())
    } else {
        let scanner = patches_ffi::PluginScanner::new(paths.to_vec());
        let report = scanner.scan(registry);
        (report.summary(), scan_detail_lines(&report))
    };
    let mut module_names: Vec<String> =
        registry.module_names().map(|s| s.to_string()).collect();
    module_names.sort();
    ScanDetails {
        summary,
        details,
        module_names,
    }
}

/// Project the DSL tap manifest into the webview-facing summary list,
/// preserving slot order. `kind` collapses to the single component name
/// for simple taps and to `"compound"` for multi-component taps.
fn project_manifest(manifest: &Manifest) -> Vec<TapSummary> {
    manifest.iter().map(tap_summary).collect()
}

fn tap_summary(d: &TapDescriptor) -> TapSummary {
    let kind = if d.components.len() == 1 {
        d.components[0].as_str().to_string()
    } else {
        "compound".to_string()
    };
    TapSummary {
        name: d.name.clone(),
        slot: d.slot,
        kind,
        components: d.components.iter().map(|c| c.as_str().to_string()).collect(),
    }
}


#[cfg(test)]
mod activate_scan_tests {
    //! Ticket 0566: end-to-end — craft a saved state pointing at a
    //! module plugin dir, load it into a freshly created plugin, call
    //! `activate`, and verify the scanned module appears in the
    //! activated runtime's registry.
    use super::*;
    use crate::factory::PLUGIN_DESCRIPTOR;
    use clap_sys::ext::state::{clap_plugin_state, CLAP_EXT_STATE};
    use clap_sys::stream::clap_istream;
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use patches_registry::Registry;
    use patches_modules::default_registry;
    use crate::extensions::get_extension;
    use clap_sys::plugin::clap_plugin;

    fn gain_dylib_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.push("target");
        p.push("debug");
        #[cfg(target_os = "macos")]
        p.push("libtest_gain_plugin.dylib");
        #[cfg(target_os = "linux")]
        p.push("libtest_gain_plugin.so");
        #[cfg(target_os = "windows")]
        p.push("test_gain_plugin.dll");
        p
    }

    struct InCtx { buf: Vec<u8>, pos: RefCell<usize> }
    unsafe extern "C" fn istream_read(
        stream: *const clap_istream, data: *mut c_void, size: u64,
    ) -> i64 {
        let ctx = &*((*stream).ctx as *const InCtx);
        let mut pos = ctx.pos.borrow_mut();
        let avail = ctx.buf.len() - *pos;
        let n = avail.min(size as usize);
        if n == 0 { return 0; }
        std::ptr::copy_nonoverlapping(ctx.buf[*pos..].as_ptr(), data as *mut u8, n);
        *pos += n;
        n as i64
    }

    fn craft_state_bytes(module_paths: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        // empty file_path
        out.extend_from_slice(&0u32.to_le_bytes());
        // empty dsl_source
        out.extend_from_slice(&0u32.to_le_bytes());
        // module_paths
        out.extend_from_slice(&(module_paths.len() as u32).to_le_bytes());
        for p in module_paths {
            let bytes = p.as_bytes();
            out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(bytes);
        }
        out
    }

    #[test]
    fn state_load_plus_activate_scans_module_paths() {
        let dylib = gain_dylib_path();
        assert!(dylib.exists(), "gain dylib missing at {}", dylib.display());

        // Fresh plugin instance.
        let data = Box::new(PatchesClapPlugin {
            host: std::ptr::null(),
            processor: None,
            plan_rx: None,
            runtime: None,
            controller: Controller {
                registry: default_registry(),
                ..Controller::default()
            },
            action_queue: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            gui_handle: None,
            gui_scale: 1.0,
            sample_rate: 0.0,
            prev_beat: -1.0,
            prev_bar: -1,
            halt_handle: None,
            observer: None,
            subscribers: None,
            diagnostics: None,
            meter: Arc::new(MeterTap::new()),
        });
        let data_ptr = Box::into_raw(data);
        let clap_plugin_box = Box::new(make_clap_plugin(
            &PLUGIN_DESCRIPTOR,
            std::ptr::null(),
            data_ptr,
        ));
        let plugin_ptr: *const clap_plugin = Box::into_raw(clap_plugin_box);

        unsafe {
            // Load crafted state.
            let dylib_str = dylib.to_string_lossy().into_owned();
            let bytes = craft_state_bytes(&[&dylib_str]);
            let in_ctx = InCtx { buf: bytes, pos: RefCell::new(0) };
            let stream = clap_istream {
                ctx: &in_ctx as *const InCtx as *mut c_void,
                read: Some(istream_read),
            };

            let ext = get_extension(CLAP_EXT_STATE.as_ptr());
            assert!(!ext.is_null());
            let state_ext = &*(ext as *const clap_plugin_state);
            let load_fn = state_ext.load.expect("state.load vtable");
            assert!(load_fn(plugin_ptr, &stream), "state_load failed");

            // module_paths populated from the saved state.
            assert_eq!(
                (*data_ptr).controller.module_paths,
                vec![PathBuf::from(&dylib_str)],
            );

            // Activate — should rescan and register Gain.
            let activate = (*plugin_ptr).activate.expect("activate vtable");
            assert!(activate(plugin_ptr, 48_000.0, 32, 1024));

            let registry: &Registry = &(*data_ptr).controller.registry;
            let names: Vec<&str> = registry.module_names().collect();
            assert!(
                names.contains(&"Gain"),
                "Gain not in activated registry: {names:?}",
            );

            // Clean shutdown.
            let deactivate = (*plugin_ptr).deactivate.expect("deactivate vtable");
            deactivate(plugin_ptr);
            let destroy = (*plugin_ptr).destroy.expect("destroy vtable");
            destroy(plugin_ptr);
        }
    }

    /// Ticket 0631: perform a hard-stop rescan while the plugin is
    /// active — add a module path, cycle deactivate/activate (what the
    /// host does in response to `request_restart`), and verify the new
    /// module is registered and the engine keeps processing output.
    #[test]
    fn rescan_cycle_adds_module_and_preserves_audio() {
        let dylib = gain_dylib_path();
        assert!(dylib.exists(), "gain dylib missing at {}", dylib.display());

        let data = Box::new(PatchesClapPlugin {
            host: std::ptr::null(),
            processor: None,
            plan_rx: None,
            runtime: None,
            // Minimal patch that exercises the engine without needing
            // the Gain module — we only verify audio continuity, not
            // that the Gain module is in use.
            controller: Controller {
                registry: default_registry(),
                dsl_source: "out_left = 0\nout_right = 0\n".to_string(),
                ..Controller::default()
            },
            action_queue: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            gui_handle: None,
            gui_scale: 1.0,
            sample_rate: 0.0,
            prev_beat: -1.0,
            prev_bar: -1,
            halt_handle: None,
            observer: None,
            subscribers: None,
            diagnostics: None,
            meter: Arc::new(MeterTap::new()),
        });
        let data_ptr = Box::into_raw(data);
        let clap_plugin_box = Box::new(make_clap_plugin(
            &PLUGIN_DESCRIPTOR,
            std::ptr::null(),
            data_ptr,
        ));
        let plugin_ptr: *const clap_plugin = Box::into_raw(clap_plugin_box);

        unsafe {
            let activate = (*plugin_ptr).activate.expect("activate vtable");
            let deactivate = (*plugin_ptr).deactivate.expect("deactivate vtable");

            // Initial activate with no module paths — registry is just
            // the default set, Gain not present.
            assert!(activate(plugin_ptr, 48_000.0, 32, 1024));
            {
                let names: Vec<&str> =
                    (*data_ptr).controller.registry.module_names().collect();
                assert!(!names.contains(&"Gain"));
            }

            // Confirm the engine is live by ticking the processor —
            // adopt the pending plan first.
            let tick_once = |p: &mut PatchesClapPlugin| {
                if let Some(rx) = &mut p.plan_rx {
                    if let Ok((plan, meta)) = rx.pop() {
                        if let Some(proc) = &mut p.processor {
                            proc.adopt_plan_with_meta(plan, meta.map(|b| *b));
                        }
                    }
                }
                let proc = p.processor.as_mut().expect("processor");
                proc.write_input(0.0, 0.0);
                proc.tick()
            };
            let _before = tick_once(&mut *data_ptr);

            // Simulate a GUI rescan: add a module path and run the
            // host-side deactivate → activate cycle that `request_restart`
            // would drive.
            (*data_ptr).controller.module_paths.push(dylib.clone());
            deactivate(plugin_ptr);
            assert!((*data_ptr).processor.is_none());
            assert!(activate(plugin_ptr, 48_000.0, 32, 1024));

            // Gain now in the registry.
            let names: Vec<&str> =
                (*data_ptr).controller.registry.module_names().collect();
            assert!(
                names.contains(&"Gain"),
                "Gain not in post-rescan registry: {names:?}",
            );

            assert_eq!((*data_ptr).controller.module_paths, vec![dylib.clone()]);

            // Engine still ticks — dsl_source was recompiled and a plan
            // was pushed.
            let _after = tick_once(&mut *data_ptr);

            deactivate(plugin_ptr);
            let destroy = (*plugin_ptr).destroy.expect("destroy vtable");
            destroy(plugin_ptr);
        }
    }
}
