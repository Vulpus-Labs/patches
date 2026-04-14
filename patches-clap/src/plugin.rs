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

use patches_core::source_map::SourceMap;
use patches_core::{
    AudioEnvironment, MidiEvent, ModuleGraph, Registry, BASE_PERIODIC_UPDATE_INTERVAL,
};
use patches_diagnostics::{RenderedDiagnostic, Severity, Snippet, SnippetKind};
use patches_engine::builder::ExecutionPlan;
use patches_engine::{CleanupAction, PatchProcessor, Planner};

use crate::error::CompileError;
use crate::extensions;
use crate::gui::{DiagnosticView, GuiState};

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
    /// Parent directory of the loaded `.patches` file, used to resolve
    /// relative asset paths (e.g. IR files).
    pub(crate) base_dir: Option<std::path::PathBuf>,
    pub(crate) graph: Option<ModuleGraph>,

    // ── GUI ─────────────────────────────────────────────────────────
    pub(crate) gui_state: Arc<Mutex<GuiState>>,
    pub(crate) gui_handle: Option<crate::gui_vizia::ViziaGuiHandle>,
    pub(crate) gui_scale: f64,

    pub(crate) sample_rate: f64,

    /// Source map from the most recent successful `load_or_parse` — retained
    /// so downstream `CompileError`s can be rendered as structured diagnostics
    /// in the GUI. Reset to `None` in `deactivate`.
    pub(crate) last_source_map: Option<SourceMap>,

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
        let env = self.env.as_ref().ok_or(CompileError::NotActivated)?;
        let (file, source_map) = self.load_or_parse()?;
        self.last_source_map = Some(source_map);
        let result = patches_dsl::expand(&file)?;
        let build_result = patches_interpreter::build_with_base_dir(
            &result.patch,
            &self.registry,
            env,
            self.base_dir.as_deref(),
        )?;
        let graph = build_result.graph;
        let tracker_data = build_result.tracker_data;
        let plan = self
            .planner
            .build_with_tracker_data(&graph, &self.registry, env, tracker_data)?;
        self.graph = Some(graph);
        if let Some(tx) = &mut self.plan_tx {
            let _ = tx.push(plan);
        }
        Ok(())
    }

    /// Most recently built source map — held so that a `CompileError` can be
    /// converted into structured [`RenderedDiagnostic`]s for GUI rendering.
    pub(crate) fn take_diagnostic_view(&mut self, err: &CompileError) -> DiagnosticView {
        let source_map = self.last_source_map.clone().unwrap_or_default();
        let diagnostics = compile_error_to_diagnostics(err, &source_map);
        DiagnosticView { diagnostics, source_map: Some(source_map) }
    }

    /// Load the master file using the include loader (resolving includes) when
    /// a file path is available on disk, or fall back to parsing `dsl_source`
    /// directly (e.g. state restored without original files).
    ///
    /// The master file is read from `self.dsl_source` (already loaded by the
    /// caller) to avoid a redundant disk read and TOCTOU inconsistency.
    fn load_or_parse(&self) -> Result<(patches_dsl::File, SourceMap), CompileError> {
        let file_path = lock_gui(&self.gui_state, |g| g.file_path.clone());
        if let Some(path) = file_path {
            if path.exists() {
                let master_source = self.dsl_source.clone();
                let master_canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
                let load_result = patches_dsl::load_with(&path, |p| {
                    // For the master file, return the already-read source;
                    // for included files, read from disk.
                    let canonical = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
                    if canonical == master_canonical {
                        Ok(master_source.clone())
                    } else {
                        std::fs::read_to_string(p)
                    }
                })?;
                return Ok((load_result.file, load_result.source_map));
            }
        }
        // Fallback: parse the in-memory source (no include resolution) — emit
        // an empty SourceMap; diagnostics from this path have no resolvable
        // paths but still render with line/column derived from the span text.
        Ok((patches_dsl::parse(&self.dsl_source)?, SourceMap::new()))
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

// ── Diagnostic construction ─────────────────────────────────────────

fn compile_error_to_diagnostics(err: &CompileError, source_map: &SourceMap) -> Vec<RenderedDiagnostic> {
    let d = match err {
        CompileError::NotActivated => synthetic_diagnostic("not activated", "not-activated"),
        CompileError::Load(e) => synthetic_diagnostic(&e.to_string(), "load"),
        CompileError::Parse(e) => span_diagnostic(e.span, &e.message, "parse"),
        CompileError::Expand(e) => RenderedDiagnostic::from_expand_error(e, source_map),
        CompileError::Interpret(e) => interpret_diagnostic(e),
        CompileError::Plan(e) => plan_diagnostic(e),
    };
    vec![d]
}

fn synthetic_diagnostic(message: &str, code: &str) -> RenderedDiagnostic {
    use patches_core::source_span::SourceId;
    RenderedDiagnostic {
        severity: Severity::Error,
        code: Some(code.to_string()),
        message: message.to_string(),
        primary: Snippet {
            source: SourceId::SYNTHETIC,
            range: 0..0,
            label: "here".to_string(),
            kind: SnippetKind::Primary,
        },
        related: Vec::new(),
    }
}

fn span_diagnostic(span: patches_dsl::ast::Span, message: &str, code: &str) -> RenderedDiagnostic {
    RenderedDiagnostic {
        severity: Severity::Error,
        code: Some(code.to_string()),
        message: message.to_string(),
        primary: Snippet {
            source: span.source,
            range: span.start..span.end,
            label: "here".to_string(),
            kind: SnippetKind::Primary,
        },
        related: Vec::new(),
    }
}

fn interpret_diagnostic(err: &patches_interpreter::InterpretError) -> RenderedDiagnostic {
    let primary = Snippet {
        source: err.provenance.site.source,
        range: err.provenance.site.start..err.provenance.site.end,
        label: "here".to_string(),
        kind: SnippetKind::Primary,
    };
    let related = err
        .provenance
        .expansion
        .iter()
        .map(|s| Snippet {
            source: s.source,
            range: s.start..s.end,
            label: "expanded from here".to_string(),
            kind: SnippetKind::Expansion,
        })
        .collect();
    RenderedDiagnostic {
        severity: Severity::Error,
        code: Some("interpret".to_string()),
        message: err.message.clone(),
        primary,
        related,
    }
}

fn plan_diagnostic(err: &patches_engine::builder::BuildError) -> RenderedDiagnostic {
    let message = err.to_string();
    match &err.origin {
        Some(prov) => {
            let primary = Snippet {
                source: prov.site.source,
                range: prov.site.start..prov.site.end,
                label: "here".to_string(),
                kind: SnippetKind::Primary,
            };
            let related = prov
                .expansion
                .iter()
                .map(|s| Snippet {
                    source: s.source,
                    range: s.start..s.end,
                    label: "expanded from here".to_string(),
                    kind: SnippetKind::Expansion,
                })
                .collect();
            RenderedDiagnostic {
                severity: Severity::Error,
                code: Some("plan".to_string()),
                message,
                primary,
                related,
            }
        }
        None => synthetic_diagnostic(&message, "plan"),
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
            p.base_dir = path.parent().map(|d| d.to_path_buf());
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
