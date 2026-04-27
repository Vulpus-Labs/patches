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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use raw_window_handle::{
    DisplayHandle, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use wry::{Rect, WebView, WebViewBuilder};

use patches_observation::processor::ProcessorId;
use patches_observation::subscribers::SubscribersHandle;
use patches_plugin_common::{
    GuiSnapshot, GuiState, Intent, TapDisplayOpts, TapFrame, TapSlotFrame,
};
use patches_observation::processor::{ScopeReadOpts, SpectrumReadOpts};

const SHELL_HTML: &str = include_str!("../assets/index.html");
const SHELL_CSS: &str = include_str!("../assets/app.css");
const SHELL_JS: &str = include_str!("../assets/app.js");

fn intent_log_label(intent: &Intent) -> &'static str {
    match intent {
        Intent::Browse => "browse_requested",
        Intent::Reload => "reload_requested",
        Intent::Rescan => "rescan_requested",
        Intent::AddPath => "add_path_requested",
        Intent::RemovePath { .. } => "remove_path_requested",
        Intent::SetTapOpts { .. } => "set_tap_opts",
    }
}

fn shell_document() -> String {
    SHELL_HTML
        .replace("__APP_CSS__", SHELL_CSS)
        .replace("__APP_JS__", SHELL_JS)
}

/// Minimum interval between snapshot pushes. ~30 Hz cap.
const PUSH_INTERVAL: Duration = Duration::from_millis(33);

/// Minimum interval between tap-frame pushes. Independent of
/// [`PUSH_INTERVAL`] so live tap data isn't suppressed by snapshot
/// dedupe (and vice versa).
const TAP_PUSH_INTERVAL: Duration = Duration::from_millis(33);

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
    /// JSON of the last tap frame pushed; used to skip no-op updates.
    last_tap_json: Mutex<Option<String>>,
    /// Timestamp of the last tap-frame push.
    last_tap_push: Mutex<Instant>,
    /// Reusable scratch buffers for scope/spectrum reads.
    tap_scratch: Mutex<TapScratch>,
    /// Stop flag for the request-callback ticker thread.
    ticker_stop: Arc<AtomicBool>,
    /// Ticker thread handle. Joined on Drop.
    ticker: Option<JoinHandle<()>>,
    /// Host visibility flag — flipped by CLAP `gui_show` / `gui_hide`.
    /// When `false`, snapshot pushes and tap-frame pushes are skipped
    /// so the observer's spectrum/scope reads (and FFTs) don't run.
    /// Defaults to `true`: many hosts never call `show` and just rely
    /// on `set_parent` to mean "visible now".
    visible: AtomicBool,
}

impl Drop for WebviewGuiHandle {
    fn drop(&mut self) {
        self.ticker_stop.store(true, Ordering::Release);
        if let Some(t) = self.ticker.take() {
            let _ = t.join();
        }
    }
}

#[derive(Default)]
struct TapScratch {
    scope: Vec<f32>,
    spectrum: Vec<f32>,
}

impl WebviewGuiHandle {
    /// Main-thread update hook. Serialises `GuiState` into a
    /// [`GuiSnapshot`] and pushes it to JS if something changed and the
    /// cadence cap permits.
    /// Resize the live webview to `width` × `height` logical pixels.
    /// Called from `gui.set_size`. CSS layout reflows automatically.
    pub(crate) fn set_bounds(&self, width: u32, height: u32) {
        let _ = self.webview.set_bounds(Rect {
            position: wry::dpi::LogicalPosition::new(0, 0).into(),
            size: wry::dpi::LogicalSize::new(width, height).into(),
        });
    }

    /// Mark the GUI as shown / hidden in response to CLAP `gui_show` /
    /// `gui_hide`. Safe from any thread; just flips an atomic.
    pub(crate) fn set_visible(&self, visible: bool) {
        self.visible.store(visible, Ordering::Release);
    }

    fn is_visible(&self) -> bool {
        self.visible.load(Ordering::Acquire)
    }

    pub(crate) fn update(&self, gui_state: &Mutex<GuiState>) {
        if !self.is_visible() {
            return;
        }
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

    /// Build a [`TapFrame`] from the current `GuiState` tap manifest plus
    /// the observer's atomic-scalar surface, and push it to JS via
    /// `applyTaps`. Throttled to [`TAP_PUSH_INTERVAL`] with cheap JSON
    /// dedupe; reads only — no audio-thread interaction.
    pub(crate) fn push_taps(
        &self,
        subs: &SubscribersHandle,
        gui_state: &Mutex<GuiState>,
    ) {
        if !self.is_visible() {
            return;
        }
        let mut last_push = match self.last_tap_push.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if last_push.elapsed() < TAP_PUSH_INTERVAL {
            return;
        }

        // Snapshot taps + per-slot opts under a single brief lock so we
        // don't hold gui_state while running FFTs.
        let (taps, opts_by_slot) = match gui_state.lock() {
            Ok(g) => (g.taps.clone(), g.tap_opts.clone()),
            Err(_) => return,
        };

        let mut scratch = match self.tap_scratch.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let mut slots: Vec<TapSlotFrame> = Vec::with_capacity(taps.len());
        for tap in &taps {
            let want_scope = tap.components.iter().any(|c| c == "osc");
            let want_spectrum = tap.components.iter().any(|c| c == "spectrum");
            let opts = opts_by_slot
                .get(&tap.slot)
                .copied()
                .unwrap_or_else(TapDisplayOpts::default);
            let p = subs.read(tap.slot, ProcessorId::MeterPeak);
            let r = subs.read(tap.slot, ProcessorId::MeterRms);
            let w = if want_scope {
                let scope_opts = ScopeReadOpts {
                    decimation: opts.scope_decimation,
                    window_samples: opts.scope_window_samples,
                };
                let _ = subs.read_scope_into_with(tap.slot, scope_opts, &mut scratch.scope);
                Some(scratch.scope.clone())
            } else {
                None
            };
            let m = if want_spectrum {
                let spec_opts = SpectrumReadOpts { fft_size: opts.spectrum_fft_size };
                let _ = subs.read_spectrum_into_with(tap.slot, spec_opts, &mut scratch.spectrum);
                Some(scratch.spectrum.clone())
            } else {
                None
            };
            let want_gate = tap.components.iter().any(|c| c == "gate_led");
            let want_trigger = tap.components.iter().any(|c| c == "trigger_led");
            let g = if want_gate {
                Some(subs.read(tap.slot, ProcessorId::GateLed))
            } else {
                None
            };
            let t = if want_trigger {
                Some(subs.take_trigger(tap.slot))
            } else {
                None
            };
            slots.push(TapSlotFrame { s: tap.slot, p, r, w, m, g, t });
        }

        let frame = TapFrame::new(slots);
        let json = match serde_json::to_string(&frame) {
            Ok(s) => s,
            Err(_) => return,
        };

        let mut last_json = match self.last_tap_json.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if last_json.as_deref() == Some(json.as_str()) {
            *last_push = Instant::now();
            return;
        }

        let script = format!("window.__patches && window.__patches.applyTaps({json});");
        if self.webview.evaluate_script(&script).is_ok() {
            *last_json = Some(json);
            *last_push = Instant::now();
        }
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
    scale: f64,
) -> Option<WebviewGuiHandle> {
    if parent.is_null() {
        return None;
    }

    let raw = make_raw_window_handle(parent)?;
    let parent_handle = ParentHandle { raw };

    let ipc_state = gui_state.clone();
    let ipc_host = HostPtr(host);

    // CLAP `set_scale` reports the host DPI scale. wry's LogicalSize is
    // already DPI-aware via the parent window, but webview content
    // (CSS px) is not — apply the host scale as the page zoom so a
    // 2x host renders the UI at 2x in CSS px terms.
    let zoom = if scale > 0.0 { scale } else { 1.0 };

    let builder = WebViewBuilder::new()
        .with_html(shell_document())
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
                // SetTapOpts fires on every selector tweak; don't
                // pollute the status log with those.
                if !matches!(intent, Intent::SetTapOpts { .. }) {
                    g.push_status(format!("intent: {}", intent_log_label(&intent)));
                }
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

    if (zoom - 1.0).abs() > f64::EPSILON {
        let _ = webview.zoom(zoom);
    }

    // Ticker thread: drive `on_main_thread` at ~30 Hz while the GUI is
    // open so `push_taps` runs even when the host has no other reason
    // to call back. CLAP allows `request_callback` from any thread.
    let ticker_stop = Arc::new(AtomicBool::new(false));
    let ticker = {
        let stop = ticker_stop.clone();
        let host_ptr = HostPtr(host);
        std::thread::spawn(move || {
            // Force whole-struct capture (disjoint capture would otherwise
            // pull only `host_ptr.0`, a non-Send raw pointer).
            let host_ptr = host_ptr;
            while !stop.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(33));
                let h = host_ptr.0;
                if h.is_null() {
                    continue;
                }
                unsafe {
                    if let Some(f) = (*h).request_callback {
                        f(h);
                    }
                }
            }
        })
    };

    Some(WebviewGuiHandle {
        webview,
        last_snapshot: Mutex::new(None),
        last_push: Mutex::new(Instant::now() - PUSH_INTERVAL),
        last_tap_json: Mutex::new(None),
        last_tap_push: Mutex::new(Instant::now() - TAP_PUSH_INTERVAL),
        tap_scratch: Mutex::new(TapScratch::default()),
        ticker_stop,
        ticker: Some(ticker),
        visible: AtomicBool::new(true),
    })
}
