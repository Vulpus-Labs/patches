api.lastSnapshot = null;
api.lastFrame = null;

api.applyState = (snapshot) => {
  api.lastSnapshot = snapshot;
  api._syncTapLayout(snapshot?.taps);
  api._applyTapOpts(snapshot?.tap_opts);
  api._renderHalt(snapshot?.halt_message);
  api._renderDiagnostics(snapshot?.diagnostics);
  api._renderStatusLog(snapshot?.status_log);
  api._renderFilePath(snapshot?.file_path);
  api._renderModulePaths(snapshot?.module_paths);
  api._renderModuleNames(snapshot?.module_names);
};

api.applyTaps = (frame) => {
  api.lastFrame = frame;
  api._renderTaps(frame);
};

document.addEventListener("change", (ev) => {
  const t = ev.target;
  if (!t?.classList?.contains("tap-opt")) return;
  const name = t.dataset.tapName;
  if (!name) return;
  const payload = { kind: "set_tap_opts", name };
  if (t.dataset.spectrumOpt === "fft_size") {
    payload.spectrum_fft_size = parseInt(t.value, 10);
  } else if (t.dataset.scopeOpt === "decimation") {
    payload.scope_decimation = parseInt(t.value, 10);
  } else if (t.dataset.scopeOpt === "window") {
    payload.scope_window_samples = parseInt(t.value, 10);
  }
  sendIpc(payload);
});

document.addEventListener("click", (ev) => {
  const t = ev.target;
  if (!t?.classList) return;
  if (t.classList.contains("tab")) {
    activateTab(t.dataset.pane);
    return;
  }
  if (t.classList.contains("btn-snap") && t.dataset.scopeSlot !== undefined) {
    const sslot = parseInt(t.dataset.scopeSlot, 10);
    const sbundle = slotWidgets[sslot];
    if (sbundle?.scope && t.dataset.tapName) {
      const snapNext = !sbundle.scope.getSnap();
      sbundle.scope.setSnap(snapNext);
      t.classList.toggle("is-active", snapNext);
      postIntent("set_tap_opts", { name: t.dataset.tapName, scope_snap: snapNext });
    }
    return;
  }
  if (t.classList.contains("btn-mode") && t.dataset.spectrumSlot !== undefined) {
    const slot = parseInt(t.dataset.spectrumSlot, 10);
    const bundle = slotWidgets[slot];
    if (bundle?.spectrum && t.dataset.tapName) {
      const next = bundle.spectrum.getMode() === "heatmap" ? "curve" : "heatmap";
      bundle.spectrum.setMode(next);
      // Button label shows the *other* mode (what clicking switches to).
      t.textContent = next === "heatmap" ? "curve" : "heatmap";
      postIntent("set_tap_opts", {
        name: t.dataset.tapName,
        spectrum_heatmap: next === "heatmap",
      });
    }
    return;
  }
  if (t.classList.contains("btn-remove") && t.dataset.removeIndex !== undefined) {
    postIntent("remove_path", { index: parseInt(t.dataset.removeIndex, 10) });
    return;
  }
  if (t.classList.contains("btn") && t.dataset.intent) {
    postIntent(t.dataset.intent);
  }
});
