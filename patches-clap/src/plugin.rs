//! Core plugin struct and CLAP plugin vtable.
//!
//! `PatchesClapPlugin` holds the Patches engine state and implements
//! the CLAP plugin callbacks: init, activate, start/stop processing,
//! process, and extension queries.

use std::ffi::{c_char, c_void};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

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
    clap_event_midi, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI,
};
use clap_sys::host::clap_host;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::{
    clap_process, clap_process_status, CLAP_PROCESS_CONTINUE,
};

use patches_core::{
    AudioEnvironment, MidiEvent, ModuleGraph, Registry, BASE_PERIODIC_UPDATE_INTERVAL,
};
use patches_engine::builder::ExecutionPlan;
use patches_engine::{CleanupAction, PatchProcessor, Planner};

use crate::extensions;
use crate::gui::{GuiState, PatchSnapshot};

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
    pub(crate) processor: Option<PatchProcessor>,
    pub(crate) plan_rx: Option<rtrb::Consumer<ExecutionPlan>>,

    // ── Main-thread state ───────────────────────────────────────────
    pub(crate) plan_tx: Option<rtrb::Producer<ExecutionPlan>>,
    pub(crate) cleanup_thread: Option<JoinHandle<()>>,
    pub(crate) planner: Planner,
    pub(crate) registry: Registry,
    pub(crate) env: Option<AudioEnvironment>,

    // ── DSL state ───────────────────────────────────────────────────
    pub(crate) dsl_source: String,
    pub(crate) graph: Option<ModuleGraph>,

    // ── GUI ─────────────────────────────────────────────────────────
    pub(crate) gui_state: Arc<Mutex<GuiState>>,
    pub(crate) gui_handle: Option<crate::gui_vizia::ViziaGuiHandle>,
    pub(crate) gui_scale: f64,

    pub(crate) sample_rate: f64,
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
    pub(crate) fn compile_and_push_plan(&mut self) -> Result<(), String> {
        let env = self.env.as_ref().ok_or("not activated")?;
        let file = patches_dsl::parse(&self.dsl_source).map_err(|e| e.to_string())?;
        let result = patches_dsl::expand(&file).map_err(|e| e.to_string())?;
        let graph = patches_interpreter::build(&result.patch, &self.registry, env)
            .map_err(|e| e.to_string())?;
        let plan = self
            .planner
            .build(&graph, &self.registry, env)
            .map_err(|e| e.to_string())?;
        // Snapshot the graph for the GUI before storing it.
        let snapshot = PatchSnapshot::from_graph(&graph);
        {
            let mut gui = self.gui_state.lock().unwrap_or_else(|e| e.into_inner());
            gui.patch_snapshot = Some(snapshot);
        }
        self.graph = Some(graph);
        if let Some(tx) = &mut self.plan_tx {
            let _ = tx.push(plan);
        }
        Ok(())
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
    let p = plugin_mut(plugin);

    // If already active (e.g. sample-rate change), deactivate first.
    if p.processor.is_some() {
        dlog!("activate: already active, deactivating first");
        plugin_deactivate(plugin);
        let p = plugin_mut(plugin);
        let _ = p; // reborrow after deactivate
    }
    let p = plugin_mut(plugin);

    p.sample_rate = sample_rate;

    let env = AudioEnvironment {
        sample_rate: sample_rate as f32,
        poly_voices: 16,
        periodic_update_interval: BASE_PERIODIC_UPDATE_INTERVAL,
    };
    p.env = Some(env);

    // Cleanup ring buffer.
    let (cleanup_tx, cleanup_rx) = rtrb::RingBuffer::<CleanupAction>::new(1024);

    // Spawn cleanup thread.
    match patches_engine::kernel::spawn_cleanup_thread(cleanup_rx) {
        Ok(handle) => p.cleanup_thread = Some(handle),
        Err(e) => {
            eprintln!("patches-clap: failed to spawn cleanup thread: {e}");
            return false;
        }
    }

    // Create the processor.
    p.processor = Some(PatchProcessor::new(4096, 1024, 1, cleanup_tx));

    // Plan delivery channel.
    let (plan_tx, plan_rx) = rtrb::RingBuffer::<ExecutionPlan>::new(1);
    p.plan_tx = Some(plan_tx);
    p.plan_rx = Some(plan_rx);

    // If we already have DSL source (e.g. from state_load), compile now.
    if !p.dsl_source.is_empty() {
        if let Err(e) = p.compile_and_push_plan() {
            eprintln!("patches-clap: initial compile failed: {e}");
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

    // Drop plan channel.
    p.plan_tx.take();
    p.plan_rx.take();

    // Drop processor — this drops the cleanup_tx, signalling the
    // cleanup thread to exit.
    p.processor.take();

    // Join cleanup thread.
    if let Some(handle) = p.cleanup_thread.take() {
        let _ = handle.join();
    }

    p.env = None;
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
static PROCESS_LOGGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Count process calls to log diagnostics after a few buffers.
static PROCESS_COUNT: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

unsafe extern "C" fn plugin_process(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> clap_process_status {
    if !PROCESS_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
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
                    proc.deliver_midi(MidiEvent { bytes: midi.data });
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

    // Refresh the GUI labels if anything changed.
    if gui_dirty {
        if let Some(handle) = &p.gui_handle {
            handle.update(&p.gui_state);
        }
    }
}

/// Read from GuiState under the lock.
fn lock_gui<T>(state: &Mutex<GuiState>, f: impl FnOnce(&GuiState) -> T) -> T {
    let gui = state.lock().unwrap_or_else(|e| e.into_inner());
    f(&gui)
}

/// Mutate GuiState under the lock.
fn lock_gui_mut(state: &Mutex<GuiState>, f: impl FnOnce(&mut GuiState)) {
    let mut gui = state.lock().unwrap_or_else(|e| e.into_inner());
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
                    g.status = success_msg.into();
                }),
                Err(e) => lock_gui_mut(&p.gui_state, |g| {
                    g.status = format!("Error: {e}");
                }),
            }
        }
        Err(e) => lock_gui_mut(&p.gui_state, |g| {
            g.status = format!("Read error: {e}");
        }),
    }
}
