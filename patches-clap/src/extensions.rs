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
use clap_sys::ext::state::{clap_plugin_state, CLAP_EXT_STATE};
use clap_sys::id::CLAP_INVALID_ID;
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
/// ```
///
/// Legacy states written before 0566 end after the source section;
/// `state_load` treats a clean EOF at the module-paths count as an
/// empty list.
unsafe extern "C" fn state_save(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    let p = plugin::plugin_ref_pub(plugin);

    let path_bytes = {
        let gui = p.gui_state.lock().expect("gui_state mutex poisoned");
        gui.file_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let source_bytes = &p.dsl_source;

    if !write_length_prefixed(stream, path_bytes.as_bytes()) {
        return false;
    }
    if !write_length_prefixed(stream, source_bytes.as_bytes()) {
        return false;
    }

    let count = p.module_paths.len() as u32;
    if !stream_write_all(stream, &count.to_le_bytes()) {
        return false;
    }
    for mp in &p.module_paths {
        let s = mp.to_string_lossy();
        if !write_length_prefixed(stream, s.as_bytes()) {
            return false;
        }
    }
    true
}

unsafe extern "C" fn state_load(
    plugin: *const clap_plugin,
    stream: *const clap_istream,
) -> bool {
    let p = plugin::plugin_mut_pub(plugin);

    let path_bytes = match read_length_prefixed(stream) {
        Some(b) => b,
        None => return false,
    };
    let source_bytes = match read_length_prefixed(stream) {
        Some(b) => b,
        None => return false,
    };

    let path_str = match String::from_utf8(path_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let source = match String::from_utf8(source_bytes) {
        Ok(s) => s,
        Err(_) => return false,
    };

    {
        let mut gui = p.gui_state.lock().expect("gui_state mutex poisoned");
        if path_str.is_empty() {
            gui.file_path = None;
        } else {
            gui.file_path = Some(std::path::PathBuf::from(&path_str));
        }
    }
    p.dsl_source = source;

    // Optional trailing module_paths section. A clean EOF here means a
    // legacy state written before ticket 0566 — default to empty.
    p.module_paths = match try_read_u32(stream) {
        ReadU32::Ok(count) => {
            let mut out = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let bytes = match read_length_prefixed(stream) {
                    Some(b) => b,
                    None => return false,
                };
                let s = match String::from_utf8(bytes) {
                    Ok(s) => s,
                    Err(_) => return false,
                };
                out.push(std::path::PathBuf::from(s));
            }
            out
        }
        ReadU32::Eof => Vec::new(),
        ReadU32::Err => return false,
    };

    // Mirror into GUI so the path editor reflects what's persisted,
    // even before activate runs.
    {
        let mut gui = p.gui_state.lock().expect("gui_state mutex poisoned");
        gui.module_paths = p.module_paths.clone();
    }

    // If activated, compile and push the plan.
    if p.runtime.is_some() && !p.dsl_source.is_empty() {
        if let Err(e) = p.compile_and_push_plan() {
            eprintln!("patches-clap: state load compile failed: {e}");
        }
    }

    true
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
    *width = p.gui_width;
    *height = p.gui_height;
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
    p.gui_width = w;
    p.gui_height = h;
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

    match crate::gui::create_gui(
        parent,
        p.gui_state.clone(),
        p.host,
        p.gui_width,
        p.gui_height,
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
    let mut offset = 0usize;
    while offset < data.len() {
        let written = write_fn(
            stream,
            data[offset..].as_ptr() as *const c_void,
            (data.len() - offset) as u64,
        );
        if written <= 0 {
            return false;
        }
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
    let mut offset = 0usize;
    while offset < buf.len() {
        let read = read_fn(
            stream,
            buf[offset..].as_mut_ptr() as *mut c_void,
            (buf.len() - offset) as u64,
        );
        if read <= 0 {
            return false;
        }
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
