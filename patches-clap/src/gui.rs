//! Webview GUI — wry parented to the CLAP host window.
//!
//! Ticket 0670: open a `wry::WebView` as a child of the host's window
//! and render static HTML.
//!
//! Ticket 0671: bidirectional IPC. JS posts intents via
//! `window.ipc.postMessage(JSON)`; Rust pushes versioned `GuiSnapshot`s
//! via `evaluate_script("window.__patches.applyState(...)")` at up to
//! ~30 Hz, skipping when unchanged. No audio-thread involvement.

use std::ffi::c_void;
use std::num::NonZeroIsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use raw_window_handle::{
    DisplayHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use wry::{Rect, WebView, WebViewBuilder};

use patches_plugin_common::{GuiSnapshot, GuiState, Intent};

const HELLO_HTML: &str = include_str!("../assets/hello.html");

/// Minimum interval between snapshot pushes. ~30 Hz cap.
const PUSH_INTERVAL: Duration = Duration::from_millis(33);

/// Wrap the host pointer so it can move into the ipc handler closure.
/// Safety: the host pointer is valid for the lifetime of the GUI; we
/// only call `request_callback`, a main-thread-safe CLAP entry point.
#[derive(Clone, Copy)]
struct HostPtr(*const clap_sys::host::clap_host);
unsafe impl Send for HostPtr {}
unsafe impl Sync for HostPtr {}

/// Handle to the live webview. Dropping destroys the native webview.
pub(crate) struct WebviewGuiHandle {
    webview: WebView,
    /// JSON of the last snapshot pushed to JS; used to skip no-op updates.
    last_snapshot: Mutex<Option<String>>,
    /// Timestamp of the last push; used to cap cadence.
    last_push: Mutex<Instant>,
}

impl WebviewGuiHandle {
    /// Main-thread update hook. Serialises `GuiState` into a
    /// [`GuiSnapshot`] and pushes it to JS if something changed and the
    /// cadence cap permits.
    pub(crate) fn update(&self, gui_state: &Mutex<GuiState>) {
        let snapshot = {
            let guard = match gui_state.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            GuiSnapshot::from_state(&guard)
        };
        let json = match serde_json::to_string(&snapshot) {
            Ok(s) => s,
            Err(_) => return,
        };

        let mut last = match self.last_snapshot.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if last.as_deref() == Some(json.as_str()) {
            return;
        }
        let mut last_push = match self.last_push.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if last_push.elapsed() < PUSH_INTERVAL {
            return;
        }

        let script = format!("window.__patches && window.__patches.applyState({json});");
        if self.webview.evaluate_script(&script).is_ok() {
            *last = Some(json);
            *last_push = Instant::now();
        }
    }

    /// Push one meter frame to JS. Ticket 0673 — dedicated channel so the
    /// snapshot-throttle in `update` doesn't apply.
    pub(crate) fn push_meter(&self, peak_l: f32, peak_r: f32, rms_l: f32, rms_r: f32) {
        let script = format!(
            "window.__patches && window.__patches.applyMeter({peak_l},{peak_r},{rms_l},{rms_r});"
        );
        let _ = self.webview.evaluate_script(&script);
    }
}

/// Build a `RawWindowHandle` for the CLAP parent window on the current platform.
///
/// # Safety
/// `parent` must be the platform-appropriate handle from `clap_window.specific`.
unsafe fn make_raw_window_handle(parent: *mut c_void) -> Option<RawWindowHandle> {
    #[cfg(target_os = "macos")]
    {
        use raw_window_handle::AppKitWindowHandle;
        let ns_view = NonZeroIsize::new(parent as isize)?;
        let h = AppKitWindowHandle::new(
            std::ptr::NonNull::new(parent)?,
        );
        let _ = ns_view;
        Some(RawWindowHandle::AppKit(h))
    }
    #[cfg(target_os = "windows")]
    {
        use raw_window_handle::Win32WindowHandle;
        let hwnd = NonZeroIsize::new(parent as isize)?;
        let h = Win32WindowHandle::new(hwnd);
        Some(RawWindowHandle::Win32(h))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = parent;
        None
    }
}

struct ParentHandle {
    raw: RawWindowHandle,
}

impl HasWindowHandle for ParentHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        Ok(unsafe { WindowHandle::borrow_raw(self.raw) })
    }
}

impl HasDisplayHandle for ParentHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, raw_window_handle::HandleError> {
        #[cfg(target_os = "macos")]
        let raw = RawDisplayHandle::AppKit(raw_window_handle::AppKitDisplayHandle::new());
        #[cfg(target_os = "windows")]
        let raw = RawDisplayHandle::Windows(raw_window_handle::WindowsDisplayHandle::new());
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

/// Create the webview GUI embedded in the host-supplied parent window.
///
/// # Safety
/// `parent` must be a valid platform window handle matching the CLAP
/// window API negotiated during `gui.create`.
pub(crate) unsafe fn create_gui(
    parent: *mut c_void,
    gui_state: Arc<Mutex<GuiState>>,
    host: *const clap_sys::host::clap_host,
    width: u32,
    height: u32,
    _scale: f64,
) -> Option<WebviewGuiHandle> {
    if parent.is_null() {
        return None;
    }

    let raw = make_raw_window_handle(parent)?;
    let parent_handle = ParentHandle { raw };

    let ipc_state = gui_state.clone();
    let ipc_host = HostPtr(host);

    let builder = WebViewBuilder::new()
        .with_html(HELLO_HTML)
        .with_bounds(Rect {
            position: wry::dpi::LogicalPosition::new(0, 0).into(),
            size: wry::dpi::LogicalSize::new(width, height).into(),
        })
        .with_ipc_handler(move |req: wry::http::Request<String>| {
            let body = req.body();
            let intent: Intent = match serde_json::from_str(body) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("patches-clap-webview: bad ipc message: {e} ({body})");
                    return;
                }
            };
            if let Ok(mut g) = ipc_state.lock() {
                intent.apply(&mut g);
            }
            // Ask the host to run on_main_thread so the flipped flag is
            // drained. Safe on any thread per CLAP spec.
            let host = ipc_host.0;
            if !host.is_null() {
                unsafe {
                    if let Some(f) = (*host).request_callback {
                        f(host);
                    }
                }
            }
        });

    let webview = match builder.build_as_child(&parent_handle) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("patches-clap-webview: wry build_as_child failed: {e}");
            return None;
        }
    };

    Some(WebviewGuiHandle {
        webview,
        last_snapshot: Mutex::new(None),
        last_push: Mutex::new(Instant::now() - PUSH_INTERVAL),
    })
}
