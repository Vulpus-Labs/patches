//! Cross-platform GUI implementation using vizia + baseview.
//!
//! Creates a vizia window embedded in the host's parent window via
//! baseview. Includes a scrollable, zoomable patch graph view.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use vizia::prelude::*;
use vizia::vg;
use vizia::ParentWindow;

use crate::gui::{GuiState, PatchSnapshot, SnapshotNode};

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
}

impl Model for PluginUiData {
    fn event(&mut self, _cx: &mut EventContext, event: &mut Event) {
        event.take(|e, _| match e {
            PluginUiEvent::Browse => {
                {
                    let mut gui =
                        self.gui_state.lock().unwrap_or_else(|e| e.into_inner());
                    gui.browse_requested = true;
                }
                self.host.request_callback();
            }
            PluginUiEvent::Reload => {
                {
                    let mut gui =
                        self.gui_state.lock().unwrap_or_else(|e| e.into_inner());
                    gui.reload_requested = true;
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
    snapshot: Signal<Option<PatchSnapshot>>,
}

// Safety: Signal<T> is just an ID + PhantomData — no thread-local state.
unsafe impl Send for UiSignals {}
unsafe impl Sync for UiSignals {}

// ── Graph layout ───────────────────────────────────────────────────

/// Layout constants for the graph view.
const NODE_WIDTH: f64 = 160.0;
const NODE_HEADER_HEIGHT: f64 = 24.0;
const PORT_ROW_HEIGHT: f64 = 18.0;
const NODE_PADDING: f64 = 4.0;
const VERTEX_SPACING: f64 = 30.0;
const GRAPH_MARGIN: f64 = 20.0;

/// Compute node height based on connected port count.
fn node_height(node: &SnapshotNode) -> f64 {
    let port_rows = node.inputs.len().max(node.outputs.len());
    NODE_HEADER_HEIGHT + NODE_PADDING * 2.0 + port_rows as f64 * PORT_ROW_HEIGHT
}

/// Computed position of a node in the graph layout.
#[derive(Clone)]
struct NodeLayout {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    node: SnapshotNode,
}

impl NodeLayout {
    fn port_y(&self, port_name: &str, is_input: bool) -> f32 {
        let ports = if is_input {
            &self.node.inputs
        } else {
            &self.node.outputs
        };
        let idx = ports.iter().position(|p| p == port_name).unwrap_or(0);
        self.y
            + NODE_HEADER_HEIGHT as f32
            + NODE_PADDING as f32
            + idx as f32 * PORT_ROW_HEIGHT as f32
            + PORT_ROW_HEIGHT as f32 / 2.0
    }

    fn input_x(&self) -> f32 {
        self.x
    }

    fn output_x(&self) -> f32 {
        self.x + self.width
    }
}

/// Result of laying out the graph.
struct GraphLayout {
    nodes: Vec<NodeLayout>,
}

/// Lay out nodes using the Sugiyama algorithm via `rust-sugiyama`,
/// transposed so signal flows left-to-right.
fn layout_graph(snapshot: &PatchSnapshot) -> GraphLayout {
    if snapshot.nodes.is_empty() {
        return GraphLayout { nodes: vec![] };
    }

    let id_to_idx: HashMap<&str, u32> = snapshot
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i as u32))
        .collect();

    // Sugiyama lays out top-to-bottom. We want left-to-right, so feed
    // (height, width) and transpose the output coordinates.
    let vertices: Vec<(u32, (f64, f64))> = snapshot
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (i as u32, (node_height(node), NODE_WIDTH)))
        .collect();

    let mut edges: Vec<(u32, u32)> = snapshot
        .edges
        .iter()
        .filter_map(|e| {
            let from = *id_to_idx.get(e.from_node.as_str())?;
            let to = *id_to_idx.get(e.to_node.as_str())?;
            Some((from, to))
        })
        .collect();
    edges.sort();
    edges.dedup();

    let config = rust_sugiyama::configure::Config {
        vertex_spacing: VERTEX_SPACING,
        ..Default::default()
    };

    let components =
        rust_sugiyama::from_vertices_and_edges(&vertices, &edges, &config);

    let mut layouts = Vec::with_capacity(snapshot.nodes.len());
    let mut max_x: f32 = 0.0;
    let mut max_y: f32 = 0.0;
    let mut y_offset: f32 = 0.0;

    for (component, _w, _h) in &components {
        let mut comp_max_y: f32 = 0.0;
        for &(idx, (sx, sy)) in component {
            let i = idx;
            let node = &snapshot.nodes[i];
            let h = node_height(node) as f32;
            // Transpose: Sugiyama x → our y, Sugiyama y → our x.
            let x = GRAPH_MARGIN as f32 + sy as f32;
            let y = GRAPH_MARGIN as f32 + y_offset + sx as f32;
            layouts.push(NodeLayout {
                x,
                y,
                width: NODE_WIDTH as f32,
                height: h,
                node: node.clone(),
            });
            let right = x + NODE_WIDTH as f32;
            let bottom = y + h;
            if right > max_x {
                max_x = right;
            }
            if bottom > comp_max_y {
                comp_max_y = bottom;
            }
        }
        if comp_max_y > max_y {
            max_y = comp_max_y;
        }
        y_offset = comp_max_y + VERTEX_SPACING as f32 - GRAPH_MARGIN as f32;
    }

    GraphLayout { nodes: layouts }
}

// ── Zoom ───────────────────────────────────────────────────────────

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const ZOOM_STEP: f32 = 0.25;

// ── Patch graph view ───────────────────────────────────────────────

/// A custom vizia view that draws the patch graph using the Canvas (Skia) API.
///
/// Reads its zoom level from a shared `Signal<f32>` set by toolbar buttons.
struct PatchGraphView {
    snapshot_signal: Signal<Option<PatchSnapshot>>,
    zoom_signal: Signal<f32>,
}

impl PatchGraphView {
    fn new(
        cx: &mut Context,
        snapshot_signal: Signal<Option<PatchSnapshot>>,
        zoom_signal: Signal<f32>,
    ) -> Handle<'_, Self> {
        Self {
            snapshot_signal,
            zoom_signal,
        }
        .build(cx, |_| {})
    }
}

impl View for PatchGraphView {
    fn draw(&self, cx: &mut DrawContext, canvas: &Canvas) {
        cx.draw_background(canvas);

        let snapshot_opt: Option<PatchSnapshot> = self.snapshot_signal.get();
        let snapshot = match snapshot_opt {
            Some(ref s) if !s.nodes.is_empty() => s,
            _ => return,
        };

        let zoom = self.zoom_signal.get();
        let bounds = cx.bounds();
        let gl = layout_graph(snapshot);

        let layout_map: HashMap<&str, usize> = gl
            .nodes
            .iter()
            .enumerate()
            .map(|(i, l)| (l.node.id.as_str(), i))
            .collect();

        // Clip + zoom.
        canvas.save();
        let clip_rect =
            vg::Rect::from_xywh(bounds.x, bounds.y, bounds.w, bounds.h);
        canvas.clip_rect(clip_rect, None, Some(true));
        canvas.translate((bounds.x, bounds.y));
        canvas.scale((zoom, zoom));

        // ── Fonts (via system font manager) ────────────────────────

        let font_mgr = vg::FontMgr::new();
        let typeface =
            font_mgr.legacy_make_typeface(None, vg::FontStyle::default());
        let header_font = match typeface {
            Some(ref tf) => vg::Font::new(tf.clone(), Some(12.0)),
            None => {
                let mut f = vg::Font::default();
                f.set_size(12.0);
                f
            }
        };
        let port_font = match typeface {
            Some(ref tf) => vg::Font::new(tf.clone(), Some(10.0)),
            None => {
                let mut f = vg::Font::default();
                f.set_size(10.0);
                f
            }
        };

        // ── Paints ─────────────────────────────────────────────────

        let mut edge_paint = vg::Paint::default();
        edge_paint.set_color(vg::Color::from_argb(180, 120, 180, 255));
        edge_paint.set_style(vg::PaintStyle::Stroke);
        edge_paint.set_stroke_width(1.5);
        edge_paint.set_anti_alias(true);

        let mut node_bg_paint = vg::Paint::default();
        node_bg_paint.set_color(vg::Color::from_argb(255, 40, 44, 52));
        node_bg_paint.set_style(vg::PaintStyle::Fill);
        node_bg_paint.set_anti_alias(true);

        let mut node_border_paint = vg::Paint::default();
        node_border_paint.set_color(vg::Color::from_argb(255, 80, 90, 110));
        node_border_paint.set_style(vg::PaintStyle::Stroke);
        node_border_paint.set_stroke_width(1.0);
        node_border_paint.set_anti_alias(true);

        let mut header_bg_paint = vg::Paint::default();
        header_bg_paint.set_color(vg::Color::from_argb(255, 60, 70, 90));
        header_bg_paint.set_style(vg::PaintStyle::Fill);
        header_bg_paint.set_anti_alias(true);

        let mut header_text_paint = vg::Paint::default();
        header_text_paint.set_color(vg::Color::from_argb(255, 230, 230, 240));
        header_text_paint.set_anti_alias(true);

        let mut port_text_paint = vg::Paint::default();
        port_text_paint.set_color(vg::Color::from_argb(220, 190, 190, 200));
        port_text_paint.set_anti_alias(true);

        let mut input_dot_paint = vg::Paint::default();
        input_dot_paint.set_color(vg::Color::from_argb(255, 100, 200, 120));
        input_dot_paint.set_style(vg::PaintStyle::Fill);
        input_dot_paint.set_anti_alias(true);

        let mut output_dot_paint = vg::Paint::default();
        output_dot_paint.set_color(vg::Color::from_argb(255, 200, 140, 80));
        output_dot_paint.set_style(vg::PaintStyle::Fill);
        output_dot_paint.set_anti_alias(true);

        // ── Draw edges ─────────────────────────────────────────────

        for edge in &snapshot.edges {
            let from_idx = match layout_map.get(edge.from_node.as_str()) {
                Some(&i) => i,
                None => continue,
            };
            let to_idx = match layout_map.get(edge.to_node.as_str()) {
                Some(&i) => i,
                None => continue,
            };
            let from_layout = &gl.nodes[from_idx];
            let to_layout = &gl.nodes[to_idx];

            let x0 = from_layout.output_x();
            let y0 = from_layout.port_y(&edge.from_port, false);
            let x1 = to_layout.input_x();
            let y1 = to_layout.port_y(&edge.to_port, true);

            let mut pb = vg::PathBuilder::new();
            pb.move_to((x0, y0));
            let dx = (x1 - x0).abs() * 0.4;
            pb.cubic_to((x0 + dx, y0), (x1 - dx, y1), (x1, y1));
            let path = pb.detach();
            canvas.draw_path(&path, &edge_paint);
        }

        // ── Draw nodes ─────────────────────────────────────────────

        for layout in &gl.nodes {
            let nx = layout.x;
            let ny = layout.y;
            let nw = layout.width;
            let nh = layout.height;

            let body_rect = vg::Rect::from_xywh(nx, ny, nw, nh);
            let rrect = vg::RRect::new_rect_xy(body_rect, 4.0, 4.0);
            canvas.draw_rrect(rrect, &node_bg_paint);
            canvas.draw_rrect(rrect, &node_border_paint);

            // Header.
            let header_rect =
                vg::Rect::from_xywh(nx, ny, nw, NODE_HEADER_HEIGHT as f32);
            canvas.save();
            canvas.clip_rrect(rrect, None, Some(true));
            canvas.draw_rect(header_rect, &header_bg_paint);
            canvas.restore();

            let label =
                format!("{} : {}", layout.node.id, layout.node.module_name);
            let text_y = ny + NODE_HEADER_HEIGHT as f32 * 0.7;
            canvas.draw_str(
                &label,
                (nx + 8.0, text_y),
                &header_font,
                &header_text_paint,
            );

            // Input ports (left side).
            for (i, port) in layout.node.inputs.iter().enumerate() {
                let py = ny
                    + NODE_HEADER_HEIGHT as f32
                    + NODE_PADDING as f32
                    + i as f32 * PORT_ROW_HEIGHT as f32
                    + PORT_ROW_HEIGHT as f32 / 2.0;
                canvas.draw_circle((nx + 6.0, py), 3.0, &input_dot_paint);
                canvas.draw_str(
                    port.as_str(),
                    (nx + 13.0, py + 3.5),
                    &port_font,
                    &port_text_paint,
                );
            }

            // Output ports (right side).
            for (i, port) in layout.node.outputs.iter().enumerate() {
                let py = ny
                    + NODE_HEADER_HEIGHT as f32
                    + NODE_PADDING as f32
                    + i as f32 * PORT_ROW_HEIGHT as f32
                    + PORT_ROW_HEIGHT as f32 / 2.0;
                canvas.draw_circle(
                    (nx + nw - 6.0, py),
                    3.0,
                    &output_dot_paint,
                );
                let (text_width, _) =
                    port_font.measure_str(port.as_str(), None);
                let text_x = nx + nw - 13.0 - text_width;
                canvas.draw_str(
                    port.as_str(),
                    (text_x, py + 3.5),
                    &port_font,
                    &port_text_paint,
                );
            }
        }

        canvas.restore();
    }
}

// ── Zoom model ─────────────────────────────────────────────────────

/// Events emitted by the zoom buttons.
#[allow(clippy::enum_variant_names)]
enum ZoomEvent {
    In,
    Out,
}

/// Model that owns the zoom signal and updates it on button presses.
struct ZoomModel {
    zoom: Signal<f32>,
}

impl Model for ZoomModel {
    fn event(&mut self, _cx: &mut EventContext, event: &mut Event) {
        event.take(|e, _| {
            let cur = self.zoom.get();
            match e {
                ZoomEvent::In => {
                    self.zoom.set((cur + ZOOM_STEP).min(MAX_ZOOM));
                }
                ZoomEvent::Out => {
                    self.zoom.set((cur - ZOOM_STEP).max(MIN_ZOOM));
                }
            }
        });
    }
}

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

    let (initial_path, initial_status, initial_snapshot) = {
        let gui = gui_state.lock().unwrap_or_else(|e| e.into_inner());
        let path = gui
            .file_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No file loaded".into());
        let status = if gui.status.is_empty() {
            " ".to_owned()
        } else {
            gui.status.clone()
        };
        let snapshot = gui.patch_snapshot.clone();
        (path, status, snapshot)
    };

    let signals: Arc<Mutex<Option<UiSignals>>> = Arc::new(Mutex::new(None));
    let signals_app = signals.clone();
    let signals_idle = signals.clone();
    let gui_state_idle = gui_state.clone();
    let host_ptr = HostPtr(host);

    let window_handle = Application::new(move |cx| {
        let path_sig = Signal::new(initial_path.clone());
        let status_sig = Signal::new(initial_status.clone());
        let snapshot_sig = Signal::new(initial_snapshot.clone());
        let zoom_sig = Signal::new(1.0f32);

        *signals_app.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(UiSignals {
                path: path_sig,
                status: status_sig,
                snapshot: snapshot_sig,
            });

        PluginUiData {
            gui_state: gui_state.clone(),
            host: host_ptr,
        }
        .build(cx);

        ZoomModel { zoom: zoom_sig }.build(cx);

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

                // Zoom controls.
                Button::new(cx, |cx| Label::new(cx, "\u{2212}"))
                    .on_press(|cx| cx.emit(ZoomEvent::Out))
                    .width(Pixels(30.0));

                Label::new(
                    cx,
                    zoom_sig.map(|z| format!("{}%", (*z * 100.0) as u32)),
                )
                .width(Pixels(45.0))
                .text_align(TextAlign::Center);

                Button::new(cx, |cx| Label::new(cx, "+"))
                    .on_press(|cx| cx.emit(ZoomEvent::In))
                    .width(Pixels(30.0));
            })
            .horizontal_gap(Pixels(4.0))
            .height(Auto);

            // Status label.
            Label::new(cx, status_sig).width(Stretch(1.0));

            // Scrollable patch graph view.
            let snap = snapshot_sig;
            let zs = zoom_sig;
            ScrollView::new(cx, move |cx| {
                PatchGraphView::new(cx, snap, zs)
                    .width(Pixels(2000.0))
                    .height(Pixels(1500.0))
                    .background_color(Color::rgb(30, 32, 38));
            })
            .width(Stretch(1.0))
            .height(Stretch(1.0));
        })
        .padding(Pixels(8.0))
        .vertical_gap(Pixels(4.0));
    })
    .inner_size((width, height))
    .user_scale_factor(scale)
    .on_idle(move |_cx| {
        let sigs_guard =
            signals_idle.lock().unwrap_or_else(|e| e.into_inner());
        let Some(ref sigs) = *sigs_guard else { return };
        let gui =
            gui_state_idle.lock().unwrap_or_else(|e| e.into_inner());
        let new_path = gui
            .file_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No file loaded".into());
        let new_status = if gui.status.is_empty() {
            " ".to_owned()
        } else {
            gui.status.clone()
        };
        let new_snapshot = gui.patch_snapshot.clone();
        drop(gui);
        sigs.path.set(new_path);
        sigs.status.set(new_status);
        sigs.snapshot.set(new_snapshot);
    })
    .open_parented(&ParentWindow(parent));

    Some(ViziaGuiHandle {
        window: window_handle,
    })
}
