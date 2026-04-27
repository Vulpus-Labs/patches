//! Core plugin struct and CLAP plugin vtable.
//!
//! `PatchesClapPlugin` holds the Patches engine state and implements
//! the CLAP plugin callbacks: init, activate, start/stop processing,
//! process, and extension queries.

use std::ffi::{c_char, c_void};
use std::io::Write;
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
use patches_planner::ExecutionPlan;
use patches_engine::PatchProcessor;
use patches_host::{HostBuilder, HostRuntime, InMemorySource};

use crate::error::CompileError;
use crate::extensions;
use patches_dsl::manifest::{Manifest, TapDescriptor};
use patches_plugin_common::{DiagnosticView, GuiState, MeterTap, TapSummary};

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
    pub(crate) plan_rx: Option<rtrb::Consumer<ExecutionPlan>>,

    // ── Main-thread state ───────────────────────────────────────────
    /// Owns the planner, plan-tx producer, cleanup thread and audio env.
    /// `None` until [`activate`](plugin_activate); reset on `deactivate`.
    pub(crate) runtime: Option<HostRuntime>,
    pub(crate) registry: Registry,

    // ── DSL state ───────────────────────────────────────────────────
    pub(crate) dsl_source: String,

    /// Persisted module search paths. Populated at `create_plugin` (empty)
    /// or `state_load`. Rescanned at every `activate` into `registry`.
    pub(crate) module_paths: Vec<std::path::PathBuf>,

    // ── GUI ─────────────────────────────────────────────────────────
    pub(crate) gui_state: Arc<Mutex<GuiState>>,
    /// Clonable handle for polling engine halt state (ADR 0051). Populated
    /// in `activate` from the processor.
    pub(crate) halt_handle: Option<patches_engine::HaltHandle>,
    /// Observer thread handle. Started in `activate`, joined in `deactivate`.
    pub(crate) observer: Option<ObserverHandle>,
    /// Reader handle into the observer's atomic-scalar tap surface
    /// (ADR 0053 §7). Cloned for the GUI's main-thread tap pump.
    pub(crate) subscribers: Option<SubscribersHandle>,
    /// Observer-side diagnostic ring reader. Drained on `on_main_thread`
    /// and surfaced through the GUI status log (ticket 0725).
    pub(crate) diagnostics: Option<DiagnosticReader>,
    pub(crate) gui_handle: Option<crate::gui::WebviewGuiHandle>,
    pub(crate) gui_scale: f64,
    /// Current window size in logical pixels. Updated via
    /// `gui.set_size`; persisted between `gui_destroy` and `gui_create`
    /// so reopen restores the previous size.
    pub(crate) gui_width: u32,
    pub(crate) gui_height: u32,
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
// `gui_state` (behind Arc<Mutex>).
unsafe impl Send for PatchesClapPlugin {}

impl PatchesClapPlugin {
    /// Compile the current `dsl_source` and push the resulting plan.
    ///
    /// Called from `activate` (if source non-empty), `state_load`, and
    /// `on_main_thread`.
    pub(crate) fn compile_and_push_plan(&mut self) -> Result<(), CompileError> {
        let runtime = self
            .runtime
            .as_mut()
            .ok_or_else(|| CompileError::new(patches_host::CompileErrorKind::NotActivated))?;

        let file_path = lock_gui(&self.gui_state, |g| g.file_path.clone());
        let mut source = InMemorySource::new(self.dsl_source.clone());
        if let Some(path) = file_path {
            source = source.with_master_path(path);
        }

        let loaded = runtime.compile_and_push(&source, &self.registry)?;

        let taps = project_manifest(&loaded.manifest);
        lock_gui_mut(&self.gui_state, |g| g.taps = taps);

        if !loaded.layering_warnings.is_empty() {
            let rendered: Vec<_> = loaded
                .layering_warnings
                .iter()
                .map(patches_diagnostics::RenderedDiagnostic::from_layering_warning)
                .collect();
            lock_gui_mut(&self.gui_state, |g| g.diagnostic_view.diagnostics.extend(rendered));
        }

        Ok(())
    }

    /// Render a `CompileError` into a [`DiagnosticView`] using the
    /// source map the error itself carries.
    pub(crate) fn take_diagnostic_view(&mut self, err: &CompileError) -> DiagnosticView {
        let diagnostics = err.to_rendered_diagnostics();
        DiagnosticView {
            diagnostics,
            source_map: Some(err.source_map.clone()),
        }
    }

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
    fn request_restart(&self) {
        if self.host.is_null() {
            return;
        }
        unsafe {
            if let Some(f) = (*self.host).request_restart {
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

    // Rebuild the registry from scratch: default set plus a fresh scan
    // of any module paths the host persisted. Ticket 0566.
    p.registry = patches_modules::default_registry();
    if !p.module_paths.is_empty() {
        let scanner = patches_ffi::PluginScanner::new(p.module_paths.clone());
        let report = scanner.scan(&mut p.registry);
        let summary = report.summary();
        dlog!("activate: module scan {}", summary);
        lock_gui_mut(&p.gui_state, |g| {
            g.push_status(format!("Module scan: {summary}"));
        });
    }
    // Mirror persisted paths into the GUI so the editor reflects the
    // authoritative list after state_load or host-driven restart.
    lock_gui_mut(&p.gui_state, |g| {
        g.module_paths = p.module_paths.clone();
    });

    // If we already have DSL source (e.g. from state_load), compile now.
    if !p.dsl_source.is_empty() {
        if let Err(e) = p.compile_and_push_plan() {
            eprintln!("patches-clap: initial compile failed: {e}");
            let view = p.take_diagnostic_view(&e);
            lock_gui_mut(&p.gui_state, |g| {
                g.push_status(format!("Error: {e}"));
                g.diagnostic_view = view;
            });
            // Not fatal — plugin is still usable, just silent.
        }
        // Immediately adopt any pending plan so audio starts right away.
        if let Some(rx) = &mut p.plan_rx {
            if let Ok(plan) = rx.pop() {
                if let Some(proc) = &mut p.processor {
                    proc.adopt_plan(plan);
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
        if let Ok(plan) = rx.pop() {
            dlog!("process: adopting plan, {} active modules", plan.active_indices.len());
            proc.adopt_plan(plan);
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

    let mut gui_dirty = false;

    // Sync halt state to GUI (ADR 0051).
    if let Some(handle) = &p.halt_handle {
        let observed = handle.halt_info();
        let prev = lock_gui(&p.gui_state, |g| g.halt.clone());
        let changed = match (&observed, &prev) {
            (None, None) => false,
            (Some(a), Some(b)) => a.slot != b.slot || a.module_name != b.module_name,
            _ => true,
        };
        if changed {
            let status = observed.as_ref().map(|info| {
                let first = info.payload.lines().next().unwrap_or("");
                format!(
                    "Engine halted: module {:?} (slot {}) panicked: {} — reload the patch to recover.",
                    info.module_name, info.slot, first
                )
            });
            lock_gui_mut(&p.gui_state, |g| {
                g.halt = observed;
                if let Some(msg) = status {
                    g.push_status(msg);
                }
            });
            gui_dirty = true;
        }
    }

    // Handle GUI Browse request.
    let browse = lock_gui(&p.gui_state, |g| g.browse_requested);
    if browse {
        lock_gui_mut(&p.gui_state, |g| g.browse_requested = false);
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Patches", &["patches"])
            .pick_file()
        {
            lock_gui_mut(&p.gui_state, |g| g.file_path = Some(path.clone()));
            set_status_from_load(p, &path, "Loaded");
            gui_dirty = true;
        }
    }

    // Handle GUI Reload request.
    let (reload, path) = lock_gui(&p.gui_state, |g| {
        (g.reload_requested, g.file_path.clone())
    });
    if reload {
        lock_gui_mut(&p.gui_state, |g| g.reload_requested = false);
        if let Some(path) = path {
            set_status_from_load(p, &path, "Reloaded");
            gui_dirty = true;
        }
    }

    // Handle GUI "Add module path" request — opens a directory picker.
    let add_path = lock_gui(&p.gui_state, |g| g.add_path_requested);
    if add_path {
        lock_gui_mut(&p.gui_state, |g| g.add_path_requested = false);
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            if !p.module_paths.iter().any(|existing| existing == &dir) {
                p.module_paths.push(dir.clone());
                lock_gui_mut(&p.gui_state, |g| {
                    g.module_paths = p.module_paths.clone();
                    g.push_status(format!(
                        "Added module path: {} (press Rescan to load)",
                        dir.display()
                    ));
                });
                gui_dirty = true;
            }
        }
    }

    // Handle GUI "Remove module path" request.
    let remove_index = lock_gui_mut_take(&p.gui_state, |g| g.remove_path_index.take());
    if let Some(idx) = remove_index {
        if idx < p.module_paths.len() {
            let removed = p.module_paths.remove(idx);
            lock_gui_mut(&p.gui_state, |g| {
                g.module_paths = p.module_paths.clone();
                g.push_status(format!(
                    "Removed module path: {} (press Rescan to apply)",
                    removed.display()
                ));
            });
            gui_dirty = true;
        }
    }

    // Handle GUI Rescan request — hard-stop reload via host restart.
    let rescan = lock_gui(&p.gui_state, |g| g.rescan_requested);
    if rescan {
        lock_gui_mut(&p.gui_state, |g| {
            g.rescan_requested = false;
            g.push_status("Rescanning modules…");
            // Clear stale diagnostics so the post-restart compile starts
            // from a clean slate.
            g.diagnostic_view = DiagnosticView::default();
        });
        p.request_restart();
        gui_dirty = true;
    }

    // Drain observer diagnostics into the status log (ticket 0725).
    if let Some(reader) = p.diagnostics.as_mut() {
        let drained = reader.drain();
        if !drained.is_empty() {
            lock_gui_mut(&p.gui_state, |g| {
                for d in &drained {
                    g.push_status(d.render());
                }
            });
            gui_dirty = true;
        }
    }

    // Refresh the GUI labels if anything changed.
    if gui_dirty {
        if let Some(handle) = &p.gui_handle {
            handle.update(&p.gui_state);
        }
    }

    // Push a TapFrame at most once per `TAP_PUSH_INTERVAL`. Frames flow
    // through a separate channel from `applyState` so snapshot dedupe
    // doesn't suppress live tap updates.
    if let (Some(handle), Some(subs)) = (&p.gui_handle, &p.subscribers) {
        handle.push_taps(subs, &p.gui_state);
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

/// Mutate GuiState under the lock and return a value produced from it.
fn lock_gui_mut_take<T>(state: &Mutex<GuiState>, f: impl FnOnce(&mut GuiState) -> T) -> T {
    let mut gui = state.lock().expect("gui_state mutex poisoned");
    f(&mut gui)
}

/// Read from GuiState under the lock.
fn lock_gui<T>(state: &Mutex<GuiState>, f: impl FnOnce(&GuiState) -> T) -> T {
    let gui = state.lock().expect("gui_state mutex poisoned");
    f(&gui)
}

/// Mutate GuiState under the lock.
fn lock_gui_mut(state: &Mutex<GuiState>, f: impl FnOnce(&mut GuiState)) {
    let mut gui = state.lock().expect("gui_state mutex poisoned");
    f(&mut gui);
}

/// Read a file, compile it, push the plan, and update the GUI status.
fn set_status_from_load(
    p: &mut PatchesClapPlugin,
    path: &std::path::Path,
    success_msg: &str,
) {
    match std::fs::read_to_string(path) {
        Ok(source) => {
            p.dsl_source = source;
            match p.compile_and_push_plan() {
                Ok(()) => lock_gui_mut(&p.gui_state, |g| {
                    g.push_status(success_msg);
                    g.diagnostic_view = DiagnosticView::default();
                }),
                Err(e) => {
                    let view = p.take_diagnostic_view(&e);
                    lock_gui_mut(&p.gui_state, |g| {
                        g.push_status(format!("Error: {e}"));
                        g.diagnostic_view = view;
                    });
                }
            }
        }
        Err(e) => lock_gui_mut(&p.gui_state, |g| {
            g.push_status(format!("Read error: {e}"));
        }),
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
    use patches_plugin_common::GuiState;
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
            registry: default_registry(),
            dsl_source: String::new(),
            module_paths: Vec::new(),
            gui_state: Arc::new(Mutex::new(GuiState::default())),
            gui_handle: None,
            gui_scale: 1.0,
            gui_width: crate::extensions::GUI_WIDTH,
            gui_height: crate::extensions::GUI_HEIGHT,
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
                (*data_ptr).module_paths,
                vec![PathBuf::from(&dylib_str)],
            );

            // Activate — should rescan and register Gain.
            let activate = (*plugin_ptr).activate.expect("activate vtable");
            assert!(activate(plugin_ptr, 48_000.0, 32, 1024));

            let registry: &Registry = &(*data_ptr).registry;
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
            registry: default_registry(),
            // Minimal patch that exercises the engine without needing
            // the Gain module — we only verify audio continuity, not
            // that the Gain module is in use.
            dsl_source: "out_left = 0\nout_right = 0\n".to_string(),
            module_paths: Vec::new(),
            gui_state: Arc::new(Mutex::new(GuiState::default())),
            gui_handle: None,
            gui_scale: 1.0,
            gui_width: crate::extensions::GUI_WIDTH,
            gui_height: crate::extensions::GUI_HEIGHT,
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
                    (*data_ptr).registry.module_names().collect();
                assert!(!names.contains(&"Gain"));
            }

            // Confirm the engine is live by ticking the processor —
            // adopt the pending plan first.
            let tick_once = |p: &mut PatchesClapPlugin| {
                if let Some(rx) = &mut p.plan_rx {
                    if let Ok(plan) = rx.pop() {
                        if let Some(proc) = &mut p.processor {
                            proc.adopt_plan(plan);
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
            (*data_ptr).module_paths.push(dylib.clone());
            deactivate(plugin_ptr);
            assert!((*data_ptr).processor.is_none());
            assert!(activate(plugin_ptr, 48_000.0, 32, 1024));

            // Gain now in the registry.
            let names: Vec<&str> =
                (*data_ptr).registry.module_names().collect();
            assert!(
                names.contains(&"Gain"),
                "Gain not in post-rescan registry: {names:?}",
            );

            // GUI mirror updated.
            {
                let gui = (*data_ptr)
                    .gui_state
                    .lock()
                    .expect("gui_state mutex poisoned");
                assert_eq!(gui.module_paths, vec![dylib.clone()]);
            }

            // Engine still ticks — dsl_source was recompiled and a plan
            // was pushed.
            let _after = tick_once(&mut *data_ptr);

            deactivate(plugin_ptr);
            let destroy = (*plugin_ptr).destroy.expect("destroy vtable");
            destroy(plugin_ptr);
        }
    }
}
