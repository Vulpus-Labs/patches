//! CLAP extension implementations.
//!
//! Provides vtable statics for the extensions this plugin supports:
//! audio ports, note ports, state, and GUI.

use std::ffi::{c_char, c_void, CStr};

use clap_sys::ext::audio_ports::{
    clap_audio_port_info, clap_plugin_audio_ports, CLAP_AUDIO_PORT_IS_MAIN, CLAP_EXT_AUDIO_PORTS,
    CLAP_PORT_STEREO,
};
#[allow(unused_imports)]
use clap_sys::ext::gui::{
    clap_gui_resize_hints, clap_plugin_gui, clap_window, CLAP_EXT_GUI,
    CLAP_WINDOW_API_COCOA, CLAP_WINDOW_API_WIN32,
};
use clap_sys::ext::note_ports::{
    clap_note_port_info, clap_plugin_note_ports, CLAP_EXT_NOTE_PORTS, CLAP_NOTE_DIALECT_MIDI,
};
use clap_sys::ext::params::{
    clap_param_info, clap_plugin_params, CLAP_EXT_PARAMS, CLAP_PARAM_IS_AUTOMATABLE,
    CLAP_PARAM_IS_STEPPED,
};
use clap_sys::ext::state::{clap_plugin_state, CLAP_EXT_STATE};
use clap_sys::events::{
    clap_event_param_value, clap_input_events, clap_output_events,
    CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE,
};
use clap_sys::id::{clap_id, CLAP_INVALID_ID};
use clap_sys::plugin::clap_plugin;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::string_sizes::CLAP_NAME_SIZE;

use crate::plugin;

// ── Extension dispatch ──────────────────────────────────────────────

/// Called from `plugin_get_extension` — returns a pointer to the
/// matching extension vtable, or null.
pub(crate) unsafe fn get_extension(id: *const c_char) -> *const c_void {
    let id = CStr::from_ptr(id);
    if id == CLAP_EXT_AUDIO_PORTS {
        &AUDIO_PORTS as *const clap_plugin_audio_ports as *const c_void
    } else if id == CLAP_EXT_NOTE_PORTS {
        &NOTE_PORTS as *const clap_plugin_note_ports as *const c_void
    } else if id == CLAP_EXT_STATE {
        &STATE as *const clap_plugin_state as *const c_void
    } else if id == CLAP_EXT_GUI {
        &GUI as *const clap_plugin_gui as *const c_void
    } else if id == CLAP_EXT_PARAMS {
        &PARAMS as *const clap_plugin_params as *const c_void
    } else {
        std::ptr::null()
    }
}

// ── Audio ports ─────────────────────────────────────────────────────

static AUDIO_PORTS: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(audio_ports_count),
    get: Some(audio_ports_get),
};

unsafe extern "C" fn audio_ports_count(
    _plugin: *const clap_plugin,
    _is_input: bool,
) -> u32 {
    1
}

unsafe extern "C" fn audio_ports_get(
    _plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if index != 0 || info.is_null() {
        return false;
    }
    // Zero the struct first — hosts may inspect padding / unused fields.
    std::ptr::write_bytes(info, 0, 1);
    let info = &mut *info;
    info.id = if is_input { 0 } else { 1 };
    write_name(
        &mut info.name,
        if is_input { "Audio In" } else { "Audio Out" },
    );
    info.flags = CLAP_AUDIO_PORT_IS_MAIN;
    info.channel_count = 2;
    info.port_type = CLAP_PORT_STEREO.as_ptr();
    info.in_place_pair = CLAP_INVALID_ID;
    true
}

// ── Note ports ──────────────────────────────────────────────────────

static NOTE_PORTS: clap_plugin_note_ports = clap_plugin_note_ports {
    count: Some(note_ports_count),
    get: Some(note_ports_get),
};

unsafe extern "C" fn note_ports_count(
    _plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    if is_input { 1 } else { 0 }
}

unsafe extern "C" fn note_ports_get(
    _plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_note_port_info,
) -> bool {
    if !is_input || index != 0 || info.is_null() {
        return false;
    }
    std::ptr::write_bytes(info, 0, 1);
    let info = &mut *info;
    info.id = 0;
    info.supported_dialects = CLAP_NOTE_DIALECT_MIDI;
    info.preferred_dialect = CLAP_NOTE_DIALECT_MIDI;
    write_name(&mut info.name, "MIDI In");
    true
}

// ── State ───────────────────────────────────────────────────────────

static STATE: clap_plugin_state = clap_plugin_state {
    save: Some(state_save),
    load: Some(state_load),
};

/// State format:
/// ```text
/// [4 bytes LE: path_len] [path UTF-8 bytes]
/// [4 bytes LE: source_len] [source UTF-8 bytes]
/// [4 bytes LE: module_paths_count]         // optional trailing section
/// for each:
///   [4 bytes LE: len] [path UTF-8 bytes]
/// [4 bytes LE: tap_opts_count]             // optional (ticket 0753;
/// for each:                                 //  name-keyed since 0773)
///   [u32 name_len][name UTF-8][u32 fft][u32 dec][u32 win]
/// [u32 width][u32 height]                  // optional (ticket 0753)
/// [u32 tap_modes_count]                    // optional
/// for each:
///   [u32 name_len][name UTF-8][u32 snap][u32 heatmap]
/// ```
///
/// Each trailing section is independently EOF-tolerant: a clean EOF at
/// any section boundary is treated as "section absent" and the
/// remaining state keeps its defaults. Legacy states written before
/// 0566 / 0753 thus load cleanly.
/// Merge controller cache and live registry into a single name → value
/// map for persistence. Registry wins on collisions because
/// `record_value` runs every time the host or audio thread touches a
/// live param, while the cache is only refreshed on plan reload.
pub(crate) fn collect_host_controls_for_save(
    cache: &std::collections::HashMap<String, f32>,
    registry: &patches_plugin_common::host_control_registry::StandardHostControlRegistry,
) -> std::collections::HashMap<String, f32> {
    let mut hc = cache.clone();
    for (name, entry) in registry.live_sorted_by_id() {
        hc.insert(name.to_string(), entry.last_value);
    }
    hc
}

unsafe extern "C" fn state_save(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    let p = plugin::plugin_ref_pub(plugin);

    let path_bytes = p
        .controller
        .file_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let source_bytes = &p.controller.dsl_source;

    if !write_length_prefixed(stream, path_bytes.as_bytes()) {
        return false;
    }
    if !write_length_prefixed(stream, source_bytes.as_bytes()) {
        return false;
    }

    let count = p.controller.module_paths.len() as u32;
    if !stream_write_all(stream, &count.to_le_bytes()) {
        return false;
    }
    for mp in &p.controller.module_paths {
        let s = mp.to_string_lossy();
        if !write_length_prefixed(stream, s.as_bytes()) {
            return false;
        }
    }

    // Tap display opts (ticket 0753, name-keyed since 0773).
    let tap_opts: Vec<(&str, u32, u32, u32)> = p
        .controller
        .tap_opts
        .iter()
        .map(|(name, opts)| {
            (
                name.as_str(),
                opts.spectrum_fft_size as u32,
                opts.scope_decimation as u32,
                opts.scope_window_samples as u32,
            )
        })
        .collect();
    let tap_count = tap_opts.len() as u32;
    if !stream_write_all(stream, &tap_count.to_le_bytes()) {
        return false;
    }
    for (name, fft, dec, win) in &tap_opts {
        if !write_length_prefixed(stream, name.as_bytes()) { return false; }
        if !stream_write_all(stream, &fft.to_le_bytes()) { return false; }
        if !stream_write_all(stream, &dec.to_le_bytes()) { return false; }
        if !stream_write_all(stream, &win.to_le_bytes()) { return false; }
    }

    // Window size (ticket 0753).
    let (w, h) = p.controller.window_size.unwrap_or((GUI_WIDTH, GUI_HEIGHT));
    if !stream_write_all(stream, &w.to_le_bytes()) { return false; }
    if !stream_write_all(stream, &h.to_le_bytes()) { return false; }

    // Tap display modes (snap, heatmap) — separate EOF-tolerant section
    // so legacy tap_opts states (with no mode bools) still load.
    let tap_modes: Vec<(&str, u32, u32)> = p
        .controller
        .tap_opts
        .iter()
        .map(|(name, o)| (name.as_str(), o.scope_snap.to_wire(), o.spectrum_heatmap.to_wire()))
        .collect();
    let mode_count = tap_modes.len() as u32;
    if !stream_write_all(stream, &mode_count.to_le_bytes()) { return false; }
    for (name, snap, heat) in &tap_modes {
        if !write_length_prefixed(stream, name.as_bytes()) { return false; }
        if !stream_write_all(stream, &snap.to_le_bytes()) { return false; }
        if !stream_write_all(stream, &heat.to_le_bytes()) { return false; }
    }

    // Host-control values (ticket 0811). Name-keyed so cross-session
    // matching survives ID renumbering. The registry is authoritative
    // for live entries; the controller cache carries values for
    // entries that aren't currently live (e.g. tombstoned).
    let hc = collect_host_controls_for_save(
        &p.controller.host_controls,
        &p.host_control_registry,
    );
    let hc_count = hc.len() as u32;
    if !stream_write_all(stream, &hc_count.to_le_bytes()) { return false; }
    for (name, value) in &hc {
        if !write_length_prefixed(stream, name.as_bytes()) { return false; }
        if !stream_write_all(stream, &value.to_le_bytes()) { return false; }
    }

    true
}

unsafe extern "C" fn state_load(
    plugin: *const clap_plugin,
    stream: *const clap_istream,
) -> bool {
    let p = plugin::plugin_mut_pub(plugin);

    let (identity, mut settings) = match deserialize_state(stream) {
        Some(pair) => pair,
        None => return false,
    };

    // Clamp the saved window size before handing it to the controller,
    // so out-of-range values get coerced into the supported range.
    if let Some((w, h)) = settings.window_size {
        settings.window_size = Some(clamp_size(w, h));
    }

    plugin::apply_action(
        p,
        patches_plugin_common::Action::StateLoad { identity, settings },
    );
    if p.runtime.is_some() {
        plugin::apply_action(p, patches_plugin_common::Action::Activate);
    }
    true
}

/// Read the on-disk state stream into the persisted halves. Returns
/// `None` on a corrupt stream; an EOF at any optional-section boundary
/// is treated as "section absent" per the format docs above.
unsafe fn deserialize_state(
    stream: *const clap_istream,
) -> Option<(
    patches_plugin_common::PatchIdentity,
    patches_plugin_common::PersistedSettings,
)> {
    let path_bytes = read_length_prefixed(stream)?;
    let source_bytes = read_length_prefixed(stream)?;
    let path_str = String::from_utf8(path_bytes).ok()?;
    let source = String::from_utf8(source_bytes).ok()?;

    let file_path = if path_str.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(&path_str))
    };

    // Optional module_paths section.
    let module_paths = match try_read_u32(stream) {
        ReadU32::Ok(count) => {
            let mut out = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let bytes = read_length_prefixed(stream)?;
                let s = String::from_utf8(bytes).ok()?;
                out.push(std::path::PathBuf::from(s));
            }
            out
        }
        ReadU32::Eof => Vec::new(),
        ReadU32::Err => return None,
    };

    // Optional tap_opts section (name-keyed since 0773).
    let mut tap_opts: std::collections::HashMap<String, patches_plugin_common::TapDisplayOpts> =
        std::collections::HashMap::new();
    match try_read_u32(stream) {
        ReadU32::Ok(count) => {
            for _ in 0..count {
                let name_bytes = read_length_prefixed(stream)?;
                let name = String::from_utf8(name_bytes).ok()?;
                let fft = match try_read_u32(stream) {
                    ReadU32::Ok(n) => n as usize,
                    _ => return None,
                };
                let dec = match try_read_u32(stream) {
                    ReadU32::Ok(n) => n as usize,
                    _ => return None,
                };
                let win = match try_read_u32(stream) {
                    ReadU32::Ok(n) => n as usize,
                    _ => return None,
                };
                if name.is_empty() {
                    continue;
                }
                tap_opts.insert(
                    name,
                    patches_plugin_common::TapDisplayOpts {
                        spectrum_fft_size: fft,
                        scope_decimation: dec,
                        scope_window_samples: win,
                        ..patches_plugin_common::TapDisplayOpts::default()
                    },
                );
            }
        }
        ReadU32::Eof => {}
        ReadU32::Err => return None,
    }

    // Optional window size section.
    let window_size = match try_read_u32(stream) {
        ReadU32::Ok(width) => {
            let height = match try_read_u32(stream) {
                ReadU32::Ok(n) => n,
                _ => return None,
            };
            Some((width, height))
        }
        ReadU32::Eof => None,
        ReadU32::Err => return None,
    };

    // Optional tap-modes section (name-keyed since 0773).
    match try_read_u32(stream) {
        ReadU32::Ok(count) => {
            for _ in 0..count {
                let name_bytes = read_length_prefixed(stream)?;
                let name = String::from_utf8(name_bytes).ok()?;
                let snap = match try_read_u32(stream) {
                    ReadU32::Ok(n) => patches_plugin_common::ScopeMode::from_wire(n),
                    _ => return None,
                };
                let heat = match try_read_u32(stream) {
                    ReadU32::Ok(n) => patches_plugin_common::SpectrumRender::from_wire(n),
                    _ => return None,
                };
                if name.is_empty() {
                    continue;
                }
                let entry = tap_opts.entry(name).or_default();
                entry.scope_snap = snap;
                entry.spectrum_heatmap = heat;
            }
        }
        ReadU32::Eof => {}
        ReadU32::Err => return None,
    }

    // Optional host-control values section (ticket 0811).
    let mut host_controls: std::collections::HashMap<String, f32> =
        std::collections::HashMap::new();
    match try_read_u32(stream) {
        ReadU32::Ok(count) => {
            for _ in 0..count {
                let name_bytes = read_length_prefixed(stream)?;
                let name = String::from_utf8(name_bytes).ok()?;
                let mut buf = [0u8; 4];
                if !stream_read_all(stream, &mut buf) {
                    return None;
                }
                let value = f32::from_le_bytes(buf);
                if !name.is_empty() {
                    host_controls.insert(name, value);
                }
            }
        }
        ReadU32::Eof => {}
        ReadU32::Err => return None,
    }

    Some((
        patches_plugin_common::PatchIdentity {
            file_path,
            dsl_source: source,
        },
        patches_plugin_common::PersistedSettings {
            host_controls,
            tap_opts,
            window_size,
            module_paths,
        },
    ))
}

// ── GUI ─────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
const NATIVE_WINDOW_API: &CStr = CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
const NATIVE_WINDOW_API: &CStr = CLAP_WINDOW_API_WIN32;

pub(crate) const GUI_WIDTH: u32 = 800;
pub(crate) const GUI_HEIGHT: u32 = 600;
const GUI_MIN_WIDTH: u32 = 480;
const GUI_MIN_HEIGHT: u32 = 360;
const GUI_MAX_WIDTH: u32 = 3840;
const GUI_MAX_HEIGHT: u32 = 2400;

fn clamp_size(width: u32, height: u32) -> (u32, u32) {
    (
        width.clamp(GUI_MIN_WIDTH, GUI_MAX_WIDTH),
        height.clamp(GUI_MIN_HEIGHT, GUI_MAX_HEIGHT),
    )
}

static GUI: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(gui_is_api_supported),
    get_preferred_api: Some(gui_get_preferred_api),
    create: Some(gui_create),
    destroy: Some(gui_destroy),
    set_scale: Some(gui_set_scale),
    get_size: Some(gui_get_size),
    can_resize: Some(gui_can_resize),
    get_resize_hints: Some(gui_get_resize_hints),
    adjust_size: Some(gui_adjust_size),
    set_size: Some(gui_set_size),
    set_parent: Some(gui_set_parent),
    set_transient: Some(gui_set_transient),
    suggest_title: Some(gui_suggest_title),
    show: Some(gui_show),
    hide: Some(gui_hide),
};

unsafe extern "C" fn gui_is_api_supported(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if is_floating {
        return false;
    }
    CStr::from_ptr(api) == NATIVE_WINDOW_API
}

unsafe extern "C" fn gui_get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    *api = NATIVE_WINDOW_API.as_ptr();
    *is_floating = false;
    true
}

unsafe extern "C" fn gui_create(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    if is_floating {
        return false;
    }
    CStr::from_ptr(api) == NATIVE_WINDOW_API
}

unsafe extern "C" fn gui_destroy(plugin: *const clap_plugin) {
    let p = plugin::plugin_mut_pub(plugin);
    p.gui_handle.take(); // Drop closes the vizia window.
}

unsafe extern "C" fn gui_set_scale(
    plugin: *const clap_plugin,
    scale: f64,
) -> bool {
    let p = plugin::plugin_mut_pub(plugin);
    p.gui_scale = scale;
    true
}

unsafe extern "C" fn gui_get_size(
    plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    let p = plugin::plugin_ref_pub(plugin);
    let (w, h) = p.controller.window_size.unwrap_or((GUI_WIDTH, GUI_HEIGHT));
    *width = w;
    *height = h;
    true
}

unsafe extern "C" fn gui_can_resize(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn gui_get_resize_hints(
    _plugin: *const clap_plugin,
    hints: *mut clap_gui_resize_hints,
) -> bool {
    let hints = &mut *hints;
    hints.can_resize_horizontally = true;
    hints.can_resize_vertically = true;
    hints.preserve_aspect_ratio = false;
    hints.aspect_ratio_width = 0;
    hints.aspect_ratio_height = 0;
    true
}

unsafe extern "C" fn gui_adjust_size(
    _plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    let (w, h) = clamp_size(*width, *height);
    *width = w;
    *height = h;
    true
}

unsafe extern "C" fn gui_set_size(
    plugin: *const clap_plugin,
    width: u32,
    height: u32,
) -> bool {
    let (w, h) = clamp_size(width, height);
    let p = plugin::plugin_mut_pub(plugin);
    let delta = plugin::apply_action(p, patches_plugin_common::Action::SetWindowSize(w, h));
    if delta.persistable_changed {
        // Inline single-call site: gui_set_size runs on the main thread,
        // so we don't go through the queue.
        p.mark_state_dirty();
    }
    if let Some(handle) = &p.gui_handle {
        handle.set_bounds(w, h);
    }
    true
}

unsafe extern "C" fn gui_set_parent(
    plugin: *const clap_plugin,
    window: *const clap_window,
) -> bool {
    let p = plugin::plugin_mut_pub(plugin);
    // Drop any existing GUI first.
    p.gui_handle.take();

    #[cfg(target_os = "macos")]
    let parent = (*window).specific.cocoa;
    #[cfg(target_os = "windows")]
    let parent = (*window).specific.win32;

    let (w, h) = p.controller.window_size.unwrap_or((GUI_WIDTH, GUI_HEIGHT));
    match crate::gui::create_gui(
        parent,
        p.action_queue.clone(),
        p.host,
        w,
        h,
        p.gui_scale,
    ) {
        Some(handle) => {
            p.gui_handle = Some(handle);
            true
        }
        None => false,
    }
}

unsafe extern "C" fn gui_set_transient(
    _plugin: *const clap_plugin,
    _window: *const clap_window,
) -> bool {
    false
}

unsafe extern "C" fn gui_suggest_title(
    _plugin: *const clap_plugin,
    _title: *const c_char,
) {
    // Not a floating window — ignore.
}

unsafe extern "C" fn gui_show(plugin: *const clap_plugin) -> bool {
    let p = plugin::plugin_mut_pub(plugin);
    if let Some(handle) = &p.gui_handle {
        handle.set_visible(true);
    }
    true
}

unsafe extern "C" fn gui_hide(plugin: *const clap_plugin) -> bool {
    let p = plugin::plugin_mut_pub(plugin);
    if let Some(handle) = &p.gui_handle {
        handle.set_visible(false);
    }
    true
}

// ── Params (host controls) ──────────────────────────────────────────

use patches_dsl::host_control_manifest::{HostControlKind, HostControlParamMap, HostControlParamValue};
use patches_plugin_common::host_control_registry::HostControlRegistry;

static PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(params_count),
    get_info: Some(params_get_info),
    get_value: Some(params_get_value),
    value_to_text: Some(params_value_to_text),
    text_to_value: Some(params_text_to_value),
    flush: Some(params_flush),
};

fn param_f64(map: &HostControlParamMap, key: &str) -> Option<f64> {
    match map.get(key)? {
        HostControlParamValue::Int(i) => Some(*i as f64),
        HostControlParamValue::Float(f) => Some(*f),
        HostControlParamValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        HostControlParamValue::Str(_) => None,
    }
}

/// Compute (min, max, default) for a host-control entry. Falls back to
/// kind-appropriate defaults when fields are missing or malformed.
///
/// Knob / slider always expose a fixed `[-1, 1]` automation range to
/// the host. The declaration's `low` / `high` are display-only metadata
/// (kept in the manifest for UI consumers); patches remap to engineering
/// units on the cable via `-[bi(low, high)]->`. `default` is interpreted
/// in wire range `[-1, 1]` and clamps; missing → 0.
pub(crate) fn param_range(kind: HostControlKind, map: &HostControlParamMap) -> (f64, f64, f64) {
    match kind {
        HostControlKind::Knob | HostControlKind::Slider => {
            let dflt = param_f64(map, "default").unwrap_or(0.0).clamp(-1.0, 1.0);
            (-1.0, 1.0, dflt)
        }
        HostControlKind::Toggle => {
            let dflt = param_f64(map, "default").unwrap_or(0.0);
            (0.0, 1.0, if dflt >= 0.5 { 1.0 } else { 0.0 })
        }
        HostControlKind::Trigger => (0.0, 1.0, 0.0),
    }
}

unsafe extern "C" fn params_count(plugin: *const clap_plugin) -> u32 {
    let p = plugin::plugin_ref_pub(plugin);
    p.host_control_registry.live_len() as u32
}

unsafe extern "C" fn params_get_info(
    plugin: *const clap_plugin,
    param_index: u32,
    info: *mut clap_param_info,
) -> bool {
    if info.is_null() {
        return false;
    }
    let p = plugin::plugin_ref_pub(plugin);
    let entries = p.host_control_registry.live_sorted_by_id();
    let entry = match entries.get(param_index as usize) {
        Some(e) => e,
        None => return false,
    };
    let (name, live) = (entry.0, entry.1);
    let (min_v, max_v, dflt) = param_range(live.kind, &live.params);

    std::ptr::write_bytes(info, 0, 1);
    let info = &mut *info;
    info.id = live.id as clap_id;
    let mut flags = CLAP_PARAM_IS_AUTOMATABLE;
    if matches!(live.kind, HostControlKind::Toggle | HostControlKind::Trigger) {
        flags |= CLAP_PARAM_IS_STEPPED;
    }
    info.flags = flags;
    // cookie left null (zeroed by write_bytes above); cross-session
    // matching is by name via the state stream, and channel/slot is
    // plan-relative so a cached cookie would go stale on replan.
    write_name(&mut info.name, name);
    info.module[0] = 0;
    info.min_value = min_v;
    info.max_value = max_v;
    info.default_value = dflt;
    true
}

unsafe extern "C" fn params_get_value(
    plugin: *const clap_plugin,
    param_id: clap_id,
    out_value: *mut f64,
) -> bool {
    if out_value.is_null() {
        return false;
    }
    let p = plugin::plugin_ref_pub(plugin);
    match p.host_control_registry.live_by_id(param_id) {
        Some((_, e)) => {
            *out_value = e.last_value as f64;
            true
        }
        None => false,
    }
}

unsafe extern "C" fn params_value_to_text(
    _plugin: *const clap_plugin,
    _param_id: clap_id,
    value: f64,
    out_buffer: *mut c_char,
    out_buffer_capacity: u32,
) -> bool {
    if out_buffer.is_null() || out_buffer_capacity == 0 {
        return false;
    }
    let s = format!("{value:.4}");
    let bytes = s.as_bytes();
    let cap = (out_buffer_capacity as usize).saturating_sub(1);
    let n = bytes.len().min(cap);
    for (i, &b) in bytes.iter().take(n).enumerate() {
        *out_buffer.add(i) = b as c_char;
    }
    *out_buffer.add(n) = 0;
    true
}

unsafe extern "C" fn params_text_to_value(
    _plugin: *const clap_plugin,
    _param_id: clap_id,
    param_value_text: *const c_char,
    out_value: *mut f64,
) -> bool {
    if param_value_text.is_null() || out_value.is_null() {
        return false;
    }
    let s = match CStr::from_ptr(param_value_text).to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    match s.trim().parse::<f64>() {
        Ok(v) => {
            *out_value = v;
            true
        }
        Err(_) => false,
    }
}

unsafe extern "C" fn params_flush(
    plugin: *const clap_plugin,
    in_: *const clap_input_events,
    _out: *const clap_output_events,
) {
    // Walk the input event list; for each CLAP_EVENT_PARAM_VALUE
    // resolve `param_id → channel` via the audio-side table and push
    // it onto the host-control scratch. Drives a one-frame block
    // through the pipeline so the value lands on the backplane on
    // the very next tick (or before the host resumes process()).
    //
    // Per CLAP, this is called on the audio thread when active and
    // not processing, and on the main thread otherwise. The scratch
    // is single-writer; CLAP forbids overlap with process(), so the
    // mutation is safe under the host's own threading contract.
    let p = plugin::plugin_mut_pub(plugin);
    // Split-borrow so we can drive both the processor and mirror into
    // the registry without aliasing.
    //
    // TODO(0825?): when flush is invoked on the audio thread (active &
    // not-processing), `host_control_registry.record_value` races with
    // main-thread mutations from `on_main_thread`. Move the registry
    // mirror onto a SPSC drained by main-thread to remove the hazard.
    let plugin::PatchesClapPlugin {
        processor,
        host_control_registry,
        ..
    } = p;
    let Some(proc) = processor.as_mut() else {
        return;
    };
    let mut saw_event = false;
    let (size_fn, get_fn) = if !in_.is_null() {
        match ((*in_).size, (*in_).get) {
            (Some(s), Some(g)) => (Some(s), Some(g)),
            _ => (None, None),
        }
    } else {
        (None, None)
    };
    if let (Some(size_fn), Some(get_fn)) = (size_fn, get_fn) {
        let count = size_fn(in_);
        for i in 0..count {
            let header = get_fn(in_, i);
            if header.is_null() {
                continue;
            }
            if (*header).space_id != CLAP_CORE_EVENT_SPACE_ID
                || (*header).type_ != CLAP_EVENT_PARAM_VALUE
            {
                continue;
            }
            let pv = &*(header as *const clap_event_param_value);
            let Some(channel) = proc.host_control_channel_of(pv.param_id) else {
                continue;
            };
            let event = patches_core::HostControlEvent {
                channel,
                sample_offset: 0,
                value: pv.value as f32,
            };
            proc.write_host_control_event(event);
            host_control_registry.record_value(pv.param_id, pv.value as f32);
            saw_event = true;
        }
    }
    // Drive a one-frame block only when we actually consumed an event;
    // skipping the empty case avoids a spurious smoothing tick on every
    // host poll.
    if saw_event {
        proc.prepare_host_control_block(1);
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Write a string into a `[c_char; CLAP_NAME_SIZE]` buffer, null-terminated.
fn write_name(buf: &mut [c_char; CLAP_NAME_SIZE], name: &str) {
    let bytes = name.as_bytes();
    let len = bytes.len().min(CLAP_NAME_SIZE - 1);
    for i in 0..len {
        buf[i] = bytes[i] as c_char;
    }
    buf[len] = 0;
}

/// Write a length-prefixed byte slice to a CLAP output stream.
unsafe fn write_length_prefixed(stream: *const clap_ostream, data: &[u8]) -> bool {
    let len = data.len() as u32;
    let len_bytes = len.to_le_bytes();
    if !stream_write_all(stream, &len_bytes) {
        return false;
    }
    if !data.is_empty() && !stream_write_all(stream, data) {
        return false;
    }
    true
}

/// Read a length-prefixed byte slice from a CLAP input stream.
unsafe fn read_length_prefixed(stream: *const clap_istream) -> Option<Vec<u8>> {
    let mut len_bytes = [0u8; 4];
    if !stream_read_all(stream, &mut len_bytes) {
        return None;
    }
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len == 0 {
        return Some(Vec::new());
    }
    let mut buf = vec![0u8; len];
    if !stream_read_all(stream, &mut buf) {
        return None;
    }
    Some(buf)
}

/// Write all bytes to a CLAP output stream, handling partial writes.
unsafe fn stream_write_all(stream: *const clap_ostream, data: &[u8]) -> bool {
    let write_fn = match (*stream).write {
        Some(f) => f,
        None => return false,
    };
    // CLAP: write returns >0 on progress, 0 means "would block, retry",
    // <0 is fatal. Bound retries so a misbehaving host can't hang us.
    let mut offset = 0usize;
    let mut zero_retries = 0;
    while offset < data.len() {
        let written = write_fn(
            stream,
            data[offset..].as_ptr() as *const c_void,
            (data.len() - offset) as u64,
        );
        if written < 0 {
            return false;
        }
        if written == 0 {
            zero_retries += 1;
            if zero_retries > 16 {
                return false;
            }
            continue;
        }
        zero_retries = 0;
        offset += written as usize;
    }
    true
}

/// Result of a tolerant u32 read: distinguishes clean EOF from error.
pub(crate) enum ReadU32 {
    Ok(u32),
    Eof,
    Err,
}

/// Attempt to read a little-endian u32. Returns `Eof` if the first
/// read returns 0 bytes (clean end-of-stream), `Err` on any partial or
/// failed read after that.
pub(crate) unsafe fn try_read_u32(stream: *const clap_istream) -> ReadU32 {
    let read_fn = match (*stream).read {
        Some(f) => f,
        None => return ReadU32::Err,
    };
    let mut buf = [0u8; 4];
    let mut offset = 0usize;
    while offset < 4 {
        let n = read_fn(
            stream,
            buf[offset..].as_mut_ptr() as *mut c_void,
            (4 - offset) as u64,
        );
        if n == 0 && offset == 0 {
            return ReadU32::Eof;
        }
        if n <= 0 {
            return ReadU32::Err;
        }
        offset += n as usize;
    }
    ReadU32::Ok(u32::from_le_bytes(buf))
}

/// Read all bytes from a CLAP input stream, handling partial reads.
unsafe fn stream_read_all(stream: *const clap_istream, buf: &mut [u8]) -> bool {
    let read_fn = match (*stream).read {
        Some(f) => f,
        None => return false,
    };
    // CLAP: read returns >0 on progress, 0 mid-payload = retry-or-EOF,
    // <0 is fatal. Bound zero-retries so a stalled stream can't hang.
    let mut offset = 0usize;
    let mut zero_retries = 0;
    while offset < buf.len() {
        let read = read_fn(
            stream,
            buf[offset..].as_mut_ptr() as *mut c_void,
            (buf.len() - offset) as u64,
        );
        if read < 0 {
            return false;
        }
        if read == 0 {
            zero_retries += 1;
            if zero_retries > 16 {
                return false;
            }
            continue;
        }
        zero_retries = 0;
        offset += read as usize;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct OutCtx { buf: RefCell<Vec<u8>> }
    struct InCtx  { buf: Vec<u8>, pos: RefCell<usize> }

    unsafe extern "C" fn ostream_write(
        stream: *const clap_ostream, data: *const c_void, size: u64,
    ) -> i64 {
        let ctx = &*((*stream).ctx as *const OutCtx);
        let slice = std::slice::from_raw_parts(data as *const u8, size as usize);
        ctx.buf.borrow_mut().extend_from_slice(slice);
        size as i64
    }
    unsafe extern "C" fn istream_read(
        stream: *const clap_istream, data: *mut c_void, size: u64,
    ) -> i64 {
        let ctx = &*((*stream).ctx as *const InCtx);
        let mut pos = ctx.pos.borrow_mut();
        let avail = ctx.buf.len() - *pos;
        let n = avail.min(size as usize);
        if n == 0 { return 0; }
        std::ptr::copy_nonoverlapping(
            ctx.buf[*pos..].as_ptr(),
            data as *mut u8,
            n,
        );
        *pos += n;
        n as i64
    }

    fn mk_ostream(ctx: &OutCtx) -> clap_ostream {
        clap_ostream {
            ctx: ctx as *const OutCtx as *mut c_void,
            write: Some(ostream_write),
        }
    }
    fn mk_istream(ctx: &InCtx) -> clap_istream {
        clap_istream {
            ctx: ctx as *const InCtx as *mut c_void,
            read: Some(istream_read),
        }
    }

    /// Writing a module-paths section then reading it back yields the
    /// original list.
    #[test]
    fn module_paths_round_trip() {
        let out = OutCtx { buf: RefCell::new(Vec::new()) };
        let os = mk_ostream(&out);

        let paths = vec![
            "/tmp/a".to_string(),
            "/opt/patches/modules".to_string(),
            "".to_string(),
        ];
        unsafe {
            let count = paths.len() as u32;
            assert!(stream_write_all(&os, &count.to_le_bytes()));
            for p in &paths {
                assert!(write_length_prefixed(&os, p.as_bytes()));
            }
        }

        let in_ctx = InCtx { buf: out.buf.into_inner(), pos: RefCell::new(0) };
        let is = mk_istream(&in_ctx);
        unsafe {
            let count = match try_read_u32(&is) {
                ReadU32::Ok(n) => n,
                _ => panic!("expected count"),
            };
            let mut got = Vec::new();
            for _ in 0..count {
                let b = read_length_prefixed(&is).expect("read");
                got.push(String::from_utf8(b).unwrap());
            }
            assert_eq!(got, paths);
        }
    }

    /// Legacy state (no trailing module-paths section) — try_read_u32
    /// at stream end returns `Eof`, not `Err`.
    #[test]
    fn legacy_state_eof_is_clean() {
        let in_ctx = InCtx { buf: Vec::new(), pos: RefCell::new(0) };
        let is = mk_istream(&in_ctx);
        unsafe {
            assert!(matches!(try_read_u32(&is), ReadU32::Eof));
        }
    }

    /// Round-trip raw u32 entries through the stream helpers. Shape is
    /// pre-0773 (slot-keyed) but the test only exercises
    /// `stream_write_all` / `try_read_u32`, not the live tap_opts
    /// section format.
    #[test]
    fn tap_opts_section_round_trip() {
        let out = OutCtx { buf: RefCell::new(Vec::new()) };
        let os = mk_ostream(&out);
        let entries = [(0u32, 1024u32, 16u32, 512u32), (3, 2048, 8, 1024)];
        unsafe {
            let count = entries.len() as u32;
            assert!(stream_write_all(&os, &count.to_le_bytes()));
            for (s, f, d, w) in &entries {
                assert!(stream_write_all(&os, &s.to_le_bytes()));
                assert!(stream_write_all(&os, &f.to_le_bytes()));
                assert!(stream_write_all(&os, &d.to_le_bytes()));
                assert!(stream_write_all(&os, &w.to_le_bytes()));
            }
            // window size trailer.
            assert!(stream_write_all(&os, &800u32.to_le_bytes()));
            assert!(stream_write_all(&os, &600u32.to_le_bytes()));
        }

        let in_ctx = InCtx { buf: out.buf.into_inner(), pos: RefCell::new(0) };
        let is = mk_istream(&in_ctx);
        unsafe {
            let count = match try_read_u32(&is) {
                ReadU32::Ok(n) => n,
                _ => panic!("count"),
            };
            let mut got = Vec::new();
            for _ in 0..count {
                let s = match try_read_u32(&is) { ReadU32::Ok(n) => n, _ => panic!() };
                let f = match try_read_u32(&is) { ReadU32::Ok(n) => n, _ => panic!() };
                let d = match try_read_u32(&is) { ReadU32::Ok(n) => n, _ => panic!() };
                let w = match try_read_u32(&is) { ReadU32::Ok(n) => n, _ => panic!() };
                got.push((s, f, d, w));
            }
            assert_eq!(got, entries);
            let width = match try_read_u32(&is) { ReadU32::Ok(n) => n, _ => panic!() };
            let height = match try_read_u32(&is) { ReadU32::Ok(n) => n, _ => panic!() };
            assert_eq!((width, height), (800, 600));
            assert!(matches!(try_read_u32(&is), ReadU32::Eof));
        }
    }

    /// Ticket 0811: a serialized state with a host_controls trailing
    /// section round-trips through `deserialize_state`, populating
    /// `PersistedSettings::host_controls` keyed by name.
    #[test]
    fn host_controls_section_round_trip() {
        let mut bytes = Vec::new();
        // path_len + path
        bytes.extend_from_slice(&0u32.to_le_bytes());
        // source_len + source
        bytes.extend_from_slice(&0u32.to_le_bytes());
        // module_paths_count
        bytes.extend_from_slice(&0u32.to_le_bytes());
        // tap_opts_count
        bytes.extend_from_slice(&0u32.to_le_bytes());
        // window size
        bytes.extend_from_slice(&800u32.to_le_bytes());
        bytes.extend_from_slice(&600u32.to_le_bytes());
        // tap_modes_count
        bytes.extend_from_slice(&0u32.to_le_bytes());
        // host_controls_count + entries
        bytes.extend_from_slice(&2u32.to_le_bytes());
        for (n, v) in [("cutoff", 0.42_f32), ("res", 0.7_f32)] {
            let nb = n.as_bytes();
            bytes.extend_from_slice(&(nb.len() as u32).to_le_bytes());
            bytes.extend_from_slice(nb);
            bytes.extend_from_slice(&v.to_le_bytes());
        }

        let in_ctx = InCtx { buf: bytes, pos: RefCell::new(0) };
        let is = mk_istream(&in_ctx);
        let (_id, settings) = unsafe { deserialize_state(&is) }.expect("parse");
        assert_eq!(settings.host_controls.get("cutoff").copied(), Some(0.42));
        assert_eq!(settings.host_controls.get("res").copied(), Some(0.7));
    }

    /// Ticket 0811: legacy state (pre-host-control section) loads with
    /// an empty `host_controls` map rather than failing.
    #[test]
    fn legacy_state_has_empty_host_controls() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0u32.to_le_bytes()); // path_len
        bytes.extend_from_slice(&0u32.to_le_bytes()); // source_len
        // EOF here — legacy state without optional sections.

        let in_ctx = InCtx { buf: bytes, pos: RefCell::new(0) };
        let is = mk_istream(&in_ctx);
        let (_id, settings) = unsafe { deserialize_state(&is) }.expect("parse");
        assert!(settings.host_controls.is_empty());
    }

    /// Ticket 0811: registry-authoritative live values override the
    /// controller cache when collecting host-control values for save.
    /// Cache values for non-live names (e.g. tombstoned across a
    /// rename) are preserved so they survive a project reload.
    #[test]
    fn collect_host_controls_merges_registry_over_cache() {
        use patches_dsl::host_control_manifest::{
            HostControlDescriptor, HostControlKind, HostControlManifest,
        };
        use patches_plugin_common::host_control_registry::{
            HostControlRegistry, StandardHostControlRegistry,
        };
        use std::collections::HashMap;

        fn desc(slot: usize, name: &str) -> HostControlDescriptor {
            HostControlDescriptor {
                slot,
                name: name.to_string(),
                kind: HostControlKind::Knob,
                params: Default::default(),
                source: patches_dsl::Provenance::root(
                    patches_core::source_span::Span::synthetic(),
                ),
            }
        }

        let mut registry = StandardHostControlRegistry::new(64);
        let manifest: HostControlManifest =
            vec![desc(0, "cutoff"), desc(1, "res")];
        registry.apply(&manifest);

        // Live values come from the registry.
        let cutoff_id = registry.id_of("cutoff").unwrap();
        let res_id = registry.id_of("res").unwrap();
        registry.record_value(cutoff_id, 0.42);
        registry.record_value(res_id, 0.7);

        // Cache holds a stale value for cutoff (must be overridden) and
        // a value for an entry that no longer exists in the registry
        // (must survive — that's how renamed/removed controls keep
        // their last value across a session).
        let mut cache: HashMap<String, f32> = HashMap::new();
        cache.insert("cutoff".into(), 0.0);
        cache.insert("legacy_drive".into(), 0.9);

        let merged = collect_host_controls_for_save(&cache, &registry);
        assert_eq!(merged.get("cutoff").copied(), Some(0.42));
        assert_eq!(merged.get("res").copied(), Some(0.7));
        assert_eq!(merged.get("legacy_drive").copied(), Some(0.9));
        assert_eq!(merged.len(), 3);
    }

    /// A partial u32 (1–3 bytes) is an error, not EOF.
    #[test]
    fn partial_u32_is_err() {
        let in_ctx = InCtx { buf: vec![1, 0], pos: RefCell::new(0) };
        let is = mk_istream(&in_ctx);
        unsafe {
            assert!(matches!(try_read_u32(&is), ReadU32::Err));
        }
    }
}
