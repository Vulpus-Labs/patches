//! Cross-platform GUI implementation using vizia + baseview.
//!
//! Creates a vizia window embedded in the host's parent window via
//! baseview. Provides a minimal controls surface: file path label,
//! Browse / Reload buttons, and a status line. Patch graph rendering
//! lives in `patches-lsp` and the `patches-svg` CLI, not in the plugin.

use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use vizia::prelude::*;
use vizia::ParentWindow;

use crate::diagnostic_widget::build_diagnostic_view;
use crate::gui::{DiagnosticView, GuiState};

// ── Host pointer wrapper ───────────────────────────────────────────

/// Wrapper for the CLAP host pointer, made Send + Sync for vizia's model.
#[derive(Clone, Copy)]
struct HostPtr(*const clap_sys::host::clap_host);

// Safety: the host pointer is only dereferenced to call request_callback,
// which CLAP guarantees is thread-safe.
unsafe impl Send for HostPtr {}
unsafe impl Sync for HostPtr {}

impl HostPtr {
    fn request_callback(&self) {
        unsafe {
            if let Some(f) = (*self.0).request_callback {
                f(self.0);
            }
        }
    }
}

// ── Vizia data model ───────────────────────────────────────────────

/// Model for handling button click events.
struct PluginUiData {
    gui_state: Arc<Mutex<GuiState>>,
    host: HostPtr,
}

/// Events emitted by the vizia UI.
enum PluginUiEvent {
    Browse,
    Reload,
    AddPath,
    RemovePath(usize),
    Rescan,
}

impl Model for PluginUiData {
    fn event(&mut self, _cx: &mut EventContext, event: &mut Event) {
        event.take(|e, _| match e {
            PluginUiEvent::Browse => {
                {
                    let mut gui =
                        self.gui_state.lock().expect("gui_state mutex poisoned");
                    gui.browse_requested = true;
                }
                self.host.request_callback();
            }
            PluginUiEvent::Reload => {
                {
                    let mut gui =
                        self.gui_state.lock().expect("gui_state mutex poisoned");
                    gui.reload_requested = true;
                }
                self.host.request_callback();
            }
            PluginUiEvent::AddPath => {
                {
                    let mut gui =
                        self.gui_state.lock().expect("gui_state mutex poisoned");
                    gui.add_path_requested = true;
                }
                self.host.request_callback();
            }
            PluginUiEvent::RemovePath(idx) => {
                {
                    let mut gui =
                        self.gui_state.lock().expect("gui_state mutex poisoned");
                    gui.remove_path_index = Some(idx);
                }
                self.host.request_callback();
            }
            PluginUiEvent::Rescan => {
                {
                    let mut gui =
                        self.gui_state.lock().expect("gui_state mutex poisoned");
                    gui.rescan_requested = true;
                }
                self.host.request_callback();
            }
        });
    }
}

// ── Signal sharing ─────────────────────────────────────────────────

/// Holds reactive signals shared between the app closure and the idle callback.
struct UiSignals {
    path: Signal<String>,
    status: Signal<String>,
    diagnostics: Signal<DiagnosticView>,
    module_paths: Signal<Vec<String>>,
}

// Safety: Signal<T> is just an ID + PhantomData — no thread-local state.
unsafe impl Send for UiSignals {}
unsafe impl Sync for UiSignals {}

// ── Public API ─────────────────────────────────────────────────────

/// Handle to the vizia GUI window. Dropping this closes the window.
pub(crate) struct ViziaGuiHandle {
    window: WindowHandle,
}

impl ViziaGuiHandle {
    pub(crate) fn update(&self, _gui_state: &Mutex<GuiState>) {}
}

impl Drop for ViziaGuiHandle {
    fn drop(&mut self) {
        self.window.close();
    }
}

/// Create the vizia GUI embedded in the host-provided parent window.
///
/// # Safety
/// `parent` must be a valid platform window handle.
/// `host` must remain valid for the lifetime of the returned handle.
pub(crate) unsafe fn create_gui(
    parent: *mut c_void,
    gui_state: Arc<Mutex<GuiState>>,
    host: *const clap_sys::host::clap_host,
    width: u32,
    height: u32,
    scale: f64,
) -> Option<ViziaGuiHandle> {
    if parent.is_null() {
        return None;
    }

    let (initial_path, initial_status, initial_view, initial_module_paths) = {
        let gui = gui_state.lock().expect("gui_state mutex poisoned");
        let path = gui
            .file_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No file loaded".into());
        let status = gui.status_text();
        let view = gui.diagnostic_view.clone();
        let mps = gui
            .module_paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        (path, status, view, mps)
    };

    let signals: Arc<Mutex<Option<UiSignals>>> = Arc::new(Mutex::new(None));
    let signals_app = signals.clone();
    let signals_idle = signals.clone();
    let gui_state_idle = gui_state.clone();
    let host_ptr = HostPtr(host);

    let window_handle = Application::new(move |cx| {
        let path_sig = Signal::new(initial_path.clone());
        let status_sig = Signal::new(initial_status.clone());
        let diag_sig: Signal<DiagnosticView> = Signal::new(initial_view.clone());
        let mp_sig: Signal<Vec<String>> = Signal::new(initial_module_paths.clone());

        *signals_app.lock().expect("gui_state mutex poisoned") =
            Some(UiSignals {
                path: path_sig,
                status: status_sig,
                diagnostics: diag_sig,
                module_paths: mp_sig,
            });

        PluginUiData {
            gui_state: gui_state.clone(),
            host: host_ptr,
        }
        .build(cx);

        VStack::new(cx, |cx| {
            // Top toolbar.
            HStack::new(cx, |cx| {
                Label::new(cx, path_sig)
                    .width(Stretch(1.0))
                    .text_wrap(false);

                Button::new(cx, |cx| Label::new(cx, "Browse\u{2026}"))
                    .on_press(|cx| cx.emit(PluginUiEvent::Browse))
                    .width(Pixels(90.0));

                Button::new(cx, |cx| Label::new(cx, "Reload"))
                    .on_press(|cx| cx.emit(PluginUiEvent::Reload))
                    .width(Pixels(90.0));
            })
            .horizontal_gap(Pixels(4.0))
            .height(Auto);

            // Module path editor.
            HStack::new(cx, |cx| {
                Label::new(cx, "Module paths")
                    .width(Stretch(1.0));
                Button::new(cx, |cx| Label::new(cx, "Add\u{2026}"))
                    .on_press(|cx| cx.emit(PluginUiEvent::AddPath))
                    .width(Pixels(80.0));
                Button::new(cx, |cx| Label::new(cx, "Rescan"))
                    .on_press(|cx| cx.emit(PluginUiEvent::Rescan))
                    .width(Pixels(90.0));
            })
            .horizontal_gap(Pixels(4.0))
            .height(Auto);

            ScrollView::new(cx, move |cx| {
                Binding::new(cx, mp_sig, move |cx| {
                    let paths = mp_sig.get();
                    if paths.is_empty() {
                        Label::new(cx, "(no module paths configured)")
                            .width(Stretch(1.0));
                    } else {
                        for (idx, p) in paths.iter().enumerate() {
                            HStack::new(cx, |cx| {
                                Label::new(cx, p.as_str())
                                    .width(Stretch(1.0))
                                    .text_wrap(false);
                                Button::new(cx, |cx| Label::new(cx, "Remove"))
                                    .on_press(move |cx| {
                                        cx.emit(PluginUiEvent::RemovePath(idx))
                                    })
                                    .width(Pixels(80.0));
                            })
                            .horizontal_gap(Pixels(4.0))
                            .height(Auto);
                        }
                    }
                });
            })
            .width(Stretch(1.0))
            .height(Pixels(120.0))
            .background_color(Color::rgb(20, 22, 26));

            // Scrollable error surface: either structured diagnostics (when
            // the last compile failed) or the plain status log.
            ScrollView::new(cx, move |cx| {
                Binding::new(cx, diag_sig, move |cx| {
                    let view = diag_sig.get();
                    if view.diagnostics.is_empty() {
                        Label::new(cx, status_sig)
                            .width(Stretch(1.0))
                            .text_wrap(true);
                    } else {
                        build_diagnostic_view(cx, &view);
                    }
                });
            })
            .width(Stretch(1.0))
            .height(Stretch(1.0))
            .background_color(Color::rgb(24, 26, 30));
        })
        .padding(Pixels(8.0))
        .vertical_gap(Pixels(4.0));
    })
    .inner_size((width, height))
    .user_scale_factor(scale)
    .on_idle(move |_cx| {
        let sigs_guard =
            signals_idle.lock().expect("gui_state mutex poisoned");
        let Some(ref sigs) = *sigs_guard else { return };
        let gui =
            gui_state_idle.lock().expect("gui_state mutex poisoned");
        let new_path = gui
            .file_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No file loaded".into());
        let new_status = gui.status_text();
        let new_view = gui.diagnostic_view.clone();
        let new_mps = gui
            .module_paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        drop(gui);
        sigs.path.set(new_path);
        sigs.status.set(new_status);
        sigs.diagnostics.set(new_view);
        sigs.module_paths.set(new_mps);
    })
    .open_parented(&ParentWindow(parent));

    Some(ViziaGuiHandle {
        window: window_handle,
    })
}
