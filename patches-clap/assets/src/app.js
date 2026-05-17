import { syncTapLayout, applyTapOpts, renderTaps, tapsState } from "./adapter/taps.js";
import {
  renderHalt, renderDiagnostics, renderStatusLog,
  renderFilePath, renderModulePaths, renderModuleNames, activateTab,
} from "./adapter/panels.js";
import { startLedDecayLoop } from "./adapter/leds.js";
import { sendIpc, postIntent, startReadyHandshake } from "./adapter/ipc.js";
import { changeIntent } from "./core/intents.js";

const api = (window.__patches = window.__patches || {});
api.lastSnapshot = null;
api.lastFrame = null;

api.applyState = (snapshot) => {
  api.lastSnapshot = snapshot;
  syncTapLayout(snapshot?.taps);
  applyTapOpts(snapshot?.tap_opts);
  renderHalt(snapshot?.halt_message);
  renderDiagnostics(snapshot?.diagnostics);
  renderStatusLog(snapshot?.status_log);
  renderFilePath(snapshot?.file_path);
  renderModulePaths(snapshot?.module_paths);
  renderModuleNames(snapshot?.module_names);
};

api.applyTaps = (frame) => {
  api.lastFrame = frame;
  renderTaps(frame);
};

api.postIntent = postIntent;

document.addEventListener("change", (ev) => {
  const payload = changeIntent(ev.target);
  if (payload) sendIpc(payload);
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
    const sbundle = tapsState.slotWidgets[sslot];
    if (sbundle?.scope && t.dataset.tapName) {
      const snapNext = !sbundle.scope.getSnap();
      sbundle.scope.setSnap(snapNext);
      t.classList.toggle("is-active", snapNext);
      postIntent("set_tap_opt", { name: t.dataset.tapName, scope_snap: snapNext });
    }
    return;
  }
  if (t.classList.contains("btn-mode") && t.dataset.spectrumSlot !== undefined) {
    const slot = parseInt(t.dataset.spectrumSlot, 10);
    const bundle = tapsState.slotWidgets[slot];
    if (bundle?.spectrum && t.dataset.tapName) {
      const next = bundle.spectrum.getMode() === "heatmap" ? "curve" : "heatmap";
      bundle.spectrum.setMode(next);
      t.textContent = next === "heatmap" ? "curve" : "heatmap";
      postIntent("set_tap_opt", {
        name: t.dataset.tapName,
        spectrum_heatmap: next === "heatmap",
      });
    }
    return;
  }
  if (t.classList.contains("btn-remove") && t.dataset.removePath !== undefined) {
    postIntent("remove_bundle_dir", { path: t.dataset.removePath });
    return;
  }
  if (t.classList.contains("btn") && t.dataset.intent) {
    postIntent(t.dataset.intent);
  }
});

startLedDecayLoop();
startReadyHandshake();
