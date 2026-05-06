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
    clap_event_midi, clap_event_param_value, clap_event_transport, CLAP_CORE_EVENT_SPACE_ID,
    CLAP_EVENT_MIDI, CLAP_EVENT_PARAM_VALUE, CLAP_TRANSPORT_HAS_BEATS_TIMELINE,
    CLAP_TRANSPORT_HAS_TEMPO, CLAP_TRANSPORT_HAS_TIME_SIGNATURE, CLAP_TRANSPORT_IS_PLAYING,
};
use clap_sys::fixedpoint::CLAP_BEATTIME_FACTOR;
use clap_sys::host::clap_host;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::{
    clap_process, clap_process_status, CLAP_PROCESS_CONTINUE,
};

use patches_core::{
    AudioEnvironment, HostControlEvent, MidiEvent, MAX_HOST_CONTROLS,
    BASE_PERIODIC_UPDATE_INTERVAL,
};
use patches_observation::{spawn_observer, tap_ring, ObserverHandle};
use patches_observation::subscribers::{DiagnosticReader, SubscribersHandle};
use patches_registry::Registry;
use patches_host::AdoptionMessage;
use patches_engine::PatchProcessor;
use patches_host::{HostBuilder, HostRuntime, InMemorySource};

use crate::extensions;
use patches_dsl::manifest::{Manifest, TapDescriptor};
use patches_plugin_common::host_control_registry::{
    HostControlRegistry, RescanLevel, StandardHostControlRegistry,
};
use patches_plugin_common::{
    xdg_list_presets, xdg_load_preset, xdg_preset_path, xdg_save_preset, Action, CompileFailure,
    CompileSuccess, Controller, DiagnosticView, Env, MeterTap, PersistedSettings, RescanProbe,
    ScanDetails, StateDelta, TapSummary,
};

/// CLAP cap on published host-control parameters (ADR 0057 §6 ties this
/// to the host-control backplane lane count).
pub(crate) const HOST_CONTROL_CAP: usize = 64;


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

    /// Name → CLAP-param-ID lifecycle registry. Lives on the main /
    /// control thread; reconciled from `controller.host_control_manifest`
    /// on each successful compile (ADR 0057 §6, ticket 0811).
    pub(crate) host_control_registry: StandardHostControlRegistry,

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

    /// Ask the host to rescan our parameter list (`CLAP_PARAM_RESCAN_*`).
    /// Used after the host-control manifest changes (ADR 0057 §6,
    /// ticket 0811).
    #[allow(dead_code)] // retained for callers outside the env path
    pub(crate) fn request_param_rescan(&self, flags: u32) {
        unsafe { request_param_rescan_raw(self.host, flags) }
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
            if let Some(proc) = p.processor.as_mut() {
                try_adopt_pending(Some(rx), proc);
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

unsafe extern "C" fn plugin_process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    // Tag the host's audio thread on first entry (ADR 0045 spike 4).
    // Idempotent; no-op when the allocator-trap feature is off.
    patches_alloc_trap::mark_audio_thread();
    // Flush denormals on the hardware. Per-callback because some hosts
    // reset MXCSR between calls. See E134.
    patches_engine::enable_flush_to_zero();

    if process.is_null() {
        dlog!("process: null process ptr");
        return CLAP_PROCESS_CONTINUE;
    }
    let p = plugin_mut(plugin);
    let Some(proc) = p.processor.as_mut() else {
        dlog!("process: no processor");
        return CLAP_PROCESS_CONTINUE;
    };

    try_adopt_pending(p.plan_rx.as_mut(), proc);

    let pr = &*process;
    let frames = pr.frames_count as usize;
    if frames == 0 {
        return CLAP_PROCESS_CONTINUE;
    }
    let Some(out_buf) = output_buffer(pr) else {
        return CLAP_PROCESS_CONTINUE;
    };
    let (in_l, in_r) = read_input_ptrs(pr);

    ingest_events(proc, pr.in_events, frames);
    write_transport_state(&mut p.prev_beat, &mut p.prev_bar, proc, pr.transport);
    run_sample_loop(proc, &p.meter, in_l, in_r, out_buf, frames);

    CLAP_PROCESS_CONTINUE
}

/// Resolve the primary output buffer if the host provided a usable one.
unsafe fn output_buffer(
    pr: &clap_process,
) -> Option<&mut clap_sys::audio_buffer::clap_audio_buffer> {
    if pr.audio_outputs_count == 0 || pr.audio_outputs.is_null() {
        return None;
    }
    let buf = &mut *pr.audio_outputs;
    if buf.data32.is_null() || buf.channel_count < 1 {
        return None;
    }
    Some(buf)
}

/// Ingest CLAP_EVENT_PARAM_VALUE and CLAP_EVENT_MIDI events into the
/// processor's host-control + MIDI scratches, then drive the per-block
/// step-fill / transpose / smooth pipeline once for `frames` (ticket
/// 0817, ADR 0069). The sample loop is pure DSP after this.
unsafe fn ingest_events(
    proc: &mut PatchProcessor,
    in_events: *const clap_sys::events::clap_input_events,
    frames: usize,
) {
    let (size_fn, get_fn) = if in_events.is_null() {
        (None, None)
    } else {
        match ((*in_events).size, (*in_events).get) {
            (Some(s), Some(g)) => (Some(s), Some(g)),
            _ => (None, None),
        }
    };
    // MIDI scratch must be sized + cleared before stamping rows.
    // Host-control scratch buffers events in a Vec and drains in
    // prepare_block, so it accepts events written before *or* after
    // prepare. Calling prepare_midi_block first prevents the prior
    // block's count rows from being zeroed *after* this block's
    // events are stamped (which silently dropped every MIDI event).
    proc.prepare_midi_block(frames);
    if let (Some(size_fn), Some(get_fn)) = (size_fn, get_fn) {
        let count = size_fn(in_events);
        for i in 0..count {
            let header = get_fn(in_events, i);
            if header.is_null() || (*header).space_id != CLAP_CORE_EVENT_SPACE_ID {
                continue;
            }
            let frame_offset = (*header).time.min(u16::MAX as u32) as u16;
            match (*header).type_ {
                CLAP_EVENT_PARAM_VALUE => {
                    let pv = &*(header as *const clap_event_param_value);
                    let Some(channel) = proc.host_control_channel_of(pv.param_id) else {
                        continue;
                    };
                    proc.write_host_control_event(HostControlEvent {
                        channel,
                        sample_offset: frame_offset,
                        value: pv.value as f32,
                    });
                }
                CLAP_EVENT_MIDI => {
                    let midi = &*(header as *const clap_event_midi);
                    proc.write_midi_event(frame_offset, MidiEvent { bytes: midi.data });
                }
                _ => {}
            }
        }
    }
    proc.prepare_host_control_block(frames);
}

/// Read the host transport (if present) and write the GLOBAL_TRANSPORT
/// row. Resets `prev_beat`/`prev_bar` on stop or when the beats
/// timeline drops, so the first frame after play resumes does not
/// synthesise a spurious beat/bar trigger from stale state.
unsafe fn write_transport_state(
    prev_beat: &mut f64,
    prev_bar: &mut i32,
    proc: &mut PatchProcessor,
    transport: *const clap_event_transport,
) {
    if transport.is_null() {
        return;
    }
    let t: &clap_event_transport = &*transport;
    let playing = if t.flags & CLAP_TRANSPORT_IS_PLAYING != 0 { 1.0 } else { 0.0 };
    let tempo = if t.flags & CLAP_TRANSPORT_HAS_TEMPO != 0 { t.tempo as f32 } else { 0.0 };
    let has_beats = t.flags & CLAP_TRANSPORT_HAS_BEATS_TIMELINE != 0;
    if playing == 0.0 || !has_beats {
        *prev_beat = -1.0;
        *prev_bar = -1;
    }
    let (beat, bar, beat_trigger, bar_trigger) = if has_beats {
        let beat_f = t.song_pos_beats as f64 / CLAP_BEATTIME_FACTOR as f64;
        let bar_num = t.bar_number;
        let beat_trig = if beat_f.floor() != prev_beat.floor() && *prev_beat >= 0.0 {
            1.0
        } else {
            0.0
        };
        let bar_trig = if bar_num != *prev_bar && *prev_bar >= 0 { 1.0 } else { 0.0 };
        *prev_beat = beat_f;
        *prev_bar = bar_num;
        (beat_f as f32, bar_num as f32, beat_trig, bar_trig)
    } else {
        (0.0, 0.0, 0.0, 0.0)
    };
    let (tsig_num, tsig_denom) = if t.flags & CLAP_TRANSPORT_HAS_TIME_SIGNATURE != 0 {
        (t.tsig_num as f32, t.tsig_denom as f32)
    } else {
        (0.0, 0.0)
    };
    proc.write_transport(
        playing, tempo, beat, bar, beat_trigger, bar_trigger, tsig_num, tsig_denom,
    );
}

/// Pure-DSP sample loop: feed input, tick the engine, accumulate meter
/// peaks/squared-sums, write output. Publishes meter snapshot at the
/// end of the block.
unsafe fn run_sample_loop(
    proc: &mut PatchProcessor,
    meter: &MeterTap,
    in_l: *const f32,
    in_r: *const f32,
    out_buf: &mut clap_sys::audio_buffer::clap_audio_buffer,
    frames: usize,
) {
    let (mut peak_l, mut peak_r) = meter.decayed_peaks();
    let mut sq_l = 0.0f32;
    let mut sq_r = 0.0f32;
    let ch0 = *out_buf.data32;
    let ch1 = if out_buf.channel_count >= 2 { *out_buf.data32.add(1) } else { std::ptr::null_mut() };
    for f in 0..frames {
        let il = if in_l.is_null() { 0.0 } else { *in_l.add(f) };
        let ir = if in_r.is_null() { 0.0 } else { *in_r.add(f) };
        proc.write_input(il, ir);
        let (ol, or) = proc.tick();
        let ola = ol.abs();
        let ora = or.abs();
        if ola > peak_l { peak_l = ola; }
        if ora > peak_r { peak_r = ora; }
        sq_l += ol * ol;
        sq_r += or * or;
        if !ch0.is_null() {
            *ch0.add(f) = ol;
        }
        if !ch1.is_null() {
            *ch1.add(f) = or;
        }
    }
    meter.publish(peak_l, peak_r, sq_l, sq_r, frames);
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
        let mut q = p
            .action_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        synthesised.into_iter().chain(q.drain(..)).collect()
    };
    let mut needs_restart = false;
    let mut needs_dirty = false;
    let mut compiled = false;
    for action in actions {
        let delta = apply_action(p, action);
        if delta.persistable_changed {
            needs_dirty = true;
        }
        if delta.requires_restart {
            needs_restart = true;
        }
        if delta.plan_recompile {
            compiled = true;
        }
    }
    if compiled {
        // Registry.apply + rescan now happen inside ClapEnv's
        // compile_and_push_plan so PlanMeta can ship the
        // (param_id → channel) table atomically with the plan
        // (ticket 0817). Here we only handle the bookkeeping that
        // depends on the post-apply registry state: seed last_value
        // for freshly-added entries, restore persisted values, and
        // surface evicted tombstones via the status log.
        let live_names: Vec<(String, u32, _, _)> = p
            .host_control_registry
            .live_sorted_by_id()
            .into_iter()
            .map(|(n, e)| (n.to_string(), e.id, e.kind, e.params.clone()))
            .collect();
        for (_name, id, kind, params) in &live_names {
            let (_, _, dflt) = crate::extensions::param_range(*kind, params);
            // Seed the default *only* when the registry's last_value is
            // still 0.0 (freshly added or never recorded). Reborn
            // entries already carry their tombstoned value; we don't
            // want to overwrite it.
            let needs_seed = p
                .host_control_registry
                .live_by_id(*id)
                .map(|(_, e)| e.last_value == 0.0)
                .unwrap_or(false);
            if needs_seed {
                p.host_control_registry.record_value(*id, dflt as f32);
            }
        }
        // Restore persisted host-control values (project / preset reload).
        let restored: Vec<(u32, f32)> = p
            .controller
            .host_controls
            .iter()
            .filter_map(|(name, v)| {
                p.host_control_registry.live(name).map(|e| (e.id, *v))
            })
            .collect();
        for (id, v) in restored {
            p.host_control_registry.record_value(id, v);
        }
        // Note: evicted-tombstones reporting moved here would require
        // re-running apply; the rescan-driving apply already happened
        // in env. Skipped — evictions are visible via the host's own
        // param-set diff after rescan.
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
    // Split borrow: ClapEnv only holds the side-effect fields the
    // controller's handlers touch (runtime + the host-control registry
    // for the compile→apply→push sequence in 0817), so `controller`
    // can be borrowed mutably alongside it.
    let PatchesClapPlugin {
        runtime,
        host_control_registry,
        host,
        controller,
        ..
    } = p;
    let host_ptr: *const clap_host = *host;
    let mut env = ClapEnv {
        runtime,
        host_control_registry,
        host: host_ptr,
    };
    controller.apply(action, &mut env)
}

/// Pop one pending adoption message (if any) and apply it to the
/// processor. Returns `true` if a plan was adopted. Shared by the
/// activate fast-path, the process-tick adoption check, and tests.
pub(crate) fn try_adopt_pending(
    plan_rx: Option<&mut rtrb::Consumer<AdoptionMessage>>,
    proc: &mut PatchProcessor,
) -> bool {
    let Some(rx) = plan_rx else { return false };
    let Ok(msg) = rx.pop() else { return false };
    let patches_host::AdoptionMessage { common, host_control } = msg;
    proc.adopt_plan_with_meta(common.plan, common.monitor, host_control);
    true
}

/// Free-function rescan dispatcher used from `ClapEnv` (which holds the
/// raw `clap_host` pointer instead of `&PatchesClapPlugin`).
unsafe fn request_param_rescan_raw(host: *const clap_host, flags: u32) {
    use clap_sys::ext::params::{clap_host_params, CLAP_EXT_PARAMS};
    if host.is_null() {
        return;
    }
    let get_ext = match (*host).get_extension {
        Some(f) => f,
        None => return,
    };
    let raw = get_ext(host, CLAP_EXT_PARAMS.as_ptr());
    if raw.is_null() {
        return;
    }
    let host_params = &*(raw as *const clap_host_params);
    if let Some(rescan) = host_params.rescan {
        rescan(host, flags);
    }
}

/// Build a `(param_id, channel)` routing table from the registry's
/// current live set (ticket 0817). Channel == backplane slot. The
/// lane-kind table is populated upstream by `patches-host` from the
/// manifest — this helper only emits the CLAP-specific column.
fn build_routing(registry: &StandardHostControlRegistry) -> Vec<(u32, u8)> {
    let mut routing = Vec::with_capacity(MAX_HOST_CONTROLS);
    for (_name, entry) in registry.live_sorted_by_id() {
        if entry.slot >= MAX_HOST_CONTROLS {
            continue;
        }
        routing.push((entry.id, entry.slot as u8));
    }
    routing
}

/// CLAP-side `Env` impl. Holds the side-effect surface the controller
/// reaches into during action handling: runtime (for compile + push),
/// host-control registry (for ID minting between compile and push —
/// ticket 0817), and the raw `clap_host` pointer used to request param
/// rescans after a manifest change.
struct ClapEnv<'a> {
    runtime: &'a mut Option<HostRuntime>,
    host_control_registry: &'a mut StandardHostControlRegistry,
    host: *const clap_host,
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
        match runtime.compile_only(&src, registry) {
            Ok(out) => {
                // Apply the new host-control manifest to the registry
                // (mints/recycles IDs, marks rescans). Must happen
                // *before* the plan is pushed so the audio-thread
                // message carrying `(param_id → channel)` lands
                // atomically with the plan (ticket 0817).
                let outcome = self
                    .host_control_registry
                    .apply(&out.loaded.host_control_manifest);

                let routing = build_routing(self.host_control_registry);
                let (loaded, msg) = out.split_with_routing(routing);
                runtime.push_blocking(&loaded, msg);

                // Rescan request follows the push; the host can pick
                // up the new param set whenever it next pumps params.
                match outcome.rescan {
                    RescanLevel::None => {}
                    RescanLevel::Info => unsafe {
                        request_param_rescan_raw(
                            self.host,
                            clap_sys::ext::params::CLAP_PARAM_RESCAN_INFO,
                        )
                    },
                    RescanLevel::All => unsafe {
                        request_param_rescan_raw(
                            self.host,
                            clap_sys::ext::params::CLAP_PARAM_RESCAN_ALL,
                        )
                    },
                }

                let taps = project_manifest(&loaded.manifest);
                let warnings: Vec<_> = loaded
                    .layering_warnings
                    .iter()
                    .map(patches_diagnostics::RenderedDiagnostic::from_layering_warning)
                    .collect();
                Ok(CompileSuccess {
                    taps,
                    warnings,
                    host_control_manifest: Arc::new(loaded.host_control_manifest.clone()),
                })
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
            host_control_registry: StandardHostControlRegistry::new(HOST_CONTROL_CAP),
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
            host_control_registry: StandardHostControlRegistry::new(HOST_CONTROL_CAP),
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
                if let Some(proc) = p.processor.as_mut() {
                    try_adopt_pending(p.plan_rx.as_mut(), proc);
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
