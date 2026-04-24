//! Webview GUI — wry parented to the CLAP host window.
//!
//! Ticket 0670 scope: open a `wry::WebView` as a child of the host's
//! window handle and render a static HTML page. No IPC, no dynamic
//! content. Content and resize policy land in 0671/0672.

use std::ffi::c_void;
use std::num::NonZeroIsize;
use std::sync::{Arc, Mutex};

use raw_window_handle::{
    DisplayHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use wry::{Rect, WebView, WebViewBuilder};

use patches_plugin_common::GuiState;

const HELLO_HTML: &str = include_str!("../assets/hello.html");

/// Handle to the live webview. Dropping destroys the native webview.
pub(crate) struct WebviewGuiHandle {
    _webview: WebView,
}

impl WebviewGuiHandle {
    /// Main-thread update hook. Ticket 0670 is static content, so this
    /// is intentionally a no-op; 0671 wires IPC / state sync here.
    pub(crate) fn update(&self, _gui_state: &Mutex<GuiState>) {}
}

/// Build a `RawWindowHandle` for the CLAP parent window on the current platform.
///
/// # Safety
/// `parent` must be the platform-appropriate handle from `clap_window.specific`.
unsafe fn make_raw_window_handle(parent: *mut c_void) -> Option<RawWindowHandle> {
    #[cfg(target_os = "macos")]
    {
        // `clap_window.specific.cocoa` is an `NSView*`.
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

/// Minimal `HasWindowHandle` / `HasDisplayHandle` wrapper around the
/// CLAP-supplied parent handle. `wry::WebViewBuilder::build_as_child`
/// only borrows this for the duration of the call.
struct ParentHandle {
    raw: RawWindowHandle,
}

impl HasWindowHandle for ParentHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
        // Safety: the RawWindowHandle was constructed from a host-provided
        // pointer that remains valid for the lifetime of the plugin GUI.
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
    _gui_state: Arc<Mutex<GuiState>>,
    _host: *const clap_sys::host::clap_host,
    width: u32,
    height: u32,
    _scale: f64,
) -> Option<WebviewGuiHandle> {
    if parent.is_null() {
        return None;
    }

    let raw = make_raw_window_handle(parent)?;
    let parent_handle = ParentHandle { raw };

    let builder = WebViewBuilder::new()
        .with_html(HELLO_HTML)
        .with_bounds(Rect {
            position: wry::dpi::LogicalPosition::new(0, 0).into(),
            size: wry::dpi::LogicalSize::new(width, height).into(),
        });

    let webview = match builder.build_as_child(&parent_handle) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("patches-clap-webview: wry build_as_child failed: {e}");
            return None;
        }
    };

    Some(WebviewGuiHandle { _webview: webview })
}
