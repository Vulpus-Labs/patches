import {
  tapsSignature, cssEscape, foldStereoPairs, collectLeds, tapOptValue,
} from "../core/taps.js";
import { meterWidget, scopeWidget, spectrumWidget } from "./widgets.js";
import { ledsState, applyLed, resetLedNodes } from "./leds.js";

export const tapsState = {
  slotWidgets: Object.create(null), // slot → { meter, scope, spectrum }
  lastTapsKey: null,
};

const ROW_GROUPS = [
  ["meter"],
  ["osc", "spectrum"],
];

function buildWidgetBox(kind, t, bundle) {
  const box = document.createElement("div");
  box.className = `tap-widget kind-${kind}`;
  const canvas = document.createElement("canvas");
  if (kind === "meter") {
    canvas.width = 240; canvas.height = 18;
    bundle.meter = meterWidget(canvas, "horizontal");
  } else if (kind === "osc") {
    canvas.width = 320; canvas.height = 96;
    bundle.scope = scopeWidget(canvas);
    const snapToggle = document.createElement("button");
    snapToggle.className = "btn btn-snap";
    snapToggle.type = "button";
    snapToggle.textContent = "snap";
    snapToggle.dataset.scopeSlot = String(t.slot);
    snapToggle.dataset.tapName = t.name;
    box.appendChild(snapToggle);
    const decSel = document.createElement("select");
    decSel.className = "tap-opt";
    decSel.dataset.scopeSlot = String(t.slot);
    decSel.dataset.tapName = t.name;
    decSel.dataset.scopeOpt = "decimation";
    for (const [val, label] of [["1", "÷1"], ["4", "÷4"], ["8", "÷8"], ["16", "÷16"], ["32", "÷32"], ["64", "÷64"]]) {
      const o = document.createElement("option");
      o.value = val;
      o.textContent = label;
      if (val === "16") o.selected = true;
      decSel.appendChild(o);
    }
    box.appendChild(decSel);
  } else if (kind === "spectrum") {
    canvas.width = 320; canvas.height = 96;
    bundle.spectrum = spectrumWidget(canvas);
    const toggle = document.createElement("button");
    toggle.className = "btn btn-mode";
    toggle.type = "button";
    toggle.textContent = "heatmap";
    toggle.dataset.spectrumSlot = String(t.slot);
    toggle.dataset.tapName = t.name;
    box.appendChild(toggle);
    const fftSel = document.createElement("select");
    fftSel.className = "tap-opt";
    fftSel.dataset.spectrumSlot = String(t.slot);
    fftSel.dataset.tapName = t.name;
    fftSel.dataset.spectrumOpt = "fft_size";
    for (const [val, label] of [["1024", "FFT 1024"], ["2048", "FFT 2048"], ["4096", "FFT 4096"]]) {
      const o = document.createElement("option");
      o.value = val;
      o.textContent = label;
      if (val === "1024") o.selected = true;
      fftSel.appendChild(o);
    }
    box.appendChild(fftSel);
  } else {
    return null;
  }
  box.appendChild(canvas);
  return box;
}

function renderStereoMeterRow(root, stem, left, right) {
  const row = document.createElement("div");
  row.className = "tap-row tap-row-stereo";
  const label = document.createElement("div");
  label.className = "tap-name";
  label.textContent = `${stem} (stereo_meter)`;
  row.appendChild(label);

  const widgets = document.createElement("div");
  widgets.className = "tap-widgets tap-widgets-stereo";
  row.appendChild(widgets);

  const bundleL = {};
  const bundleR = {};
  const lBox = buildWidgetBox("meter", left, bundleL);
  const rBox = buildWidgetBox("meter", right, bundleR);
  if (lBox) {
    lBox.classList.add("stereo-half", "stereo-l");
    widgets.appendChild(lBox);
  }
  if (rBox) {
    rBox.classList.add("stereo-half", "stereo-r");
    widgets.appendChild(rBox);
  }
  root.appendChild(row);

  tapsState.slotWidgets[left.slot] = bundleL;
  tapsState.slotWidgets[right.slot] = bundleR;
}

function rebuildTaps(taps) {
  const root = document.getElementById("taps");
  if (!root) return;
  root.innerHTML = "";
  tapsState.slotWidgets = Object.create(null);
  resetLedNodes();
  if (!taps || taps.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = "no taps declared in this patch";
    root.appendChild(empty);
    return;
  }
  const sorted = taps.slice().sort((a, b) => a.slot - b.slot);

  const leds = collectLeds(sorted);
  if (leds.length > 0) {
    const strip = document.createElement("div");
    strip.className = "led-strip";
    for (const led of leds) {
      const cell = document.createElement("div");
      cell.className = "led-cell";
      const dot = document.createElement("div");
      dot.className = `led led-${led.kind}`;
      dot.title = `${led.name} (${led.kind})`;
      const label = document.createElement("span");
      label.className = "led-label";
      label.textContent = led.name;
      cell.appendChild(dot);
      cell.appendChild(label);
      strip.appendChild(cell);
      ledsState.ledNodes[`${led.slot}:${led.kind}`] = dot;
    }
    root.appendChild(strip);
  }

  for (const entry of foldStereoPairs(sorted)) {
    if (entry.stereo) {
      renderStereoMeterRow(root, entry.stem, entry.left, entry.right);
      continue;
    }
    const t = entry.tap;
    const compsRaw = t.components ?? [];
    const bundle = {};
    let rowsRendered = 0;
    for (const group of ROW_GROUPS) {
      const present = group.filter((k) => compsRaw.includes(k));
      if (present.length === 0) continue;
      const row = document.createElement("div");
      row.className = "tap-row";
      const label = document.createElement("div");
      label.className = "tap-name";
      if (rowsRendered === 0) {
        label.textContent = `${t.name} (${compsRaw.join("+")})`;
      } else {
        label.textContent = `↳ ${present.join("+")}`;
        label.classList.add("tap-name-cont");
      }
      row.appendChild(label);
      const widgets = document.createElement("div");
      widgets.className = "tap-widgets";
      row.appendChild(widgets);
      for (const kind of present) {
        const box = buildWidgetBox(kind, t, bundle);
        if (box) widgets.appendChild(box);
      }
      root.appendChild(row);
      rowsRendered++;
    }
    if (rowsRendered > 0) tapsState.slotWidgets[t.slot] = bundle;
  }
}

export function syncTapLayout(taps) {
  const sig = tapsSignature(taps);
  if (sig === tapsState.lastTapsKey) return;
  tapsState.lastTapsKey = sig;
  rebuildTaps(taps);
}

// Restore selector values to match persisted tap_opts so a window
// close/reopen or state load doesn't snap controls back to defaults.
// Keyed by tap name (ticket 0773).
export function applyTapOpts(optsByName) {
  if (!optsByName) return;
  for (const el of document.querySelectorAll("select.tap-opt")) {
    const name = el.dataset.tapName;
    if (!name) continue;
    const o = optsByName[name];
    if (!o) continue;
    const val = tapOptValue(el.dataset, o);
    if (val != null) el.value = String(val);
  }
  for (const nameKey of Object.keys(optsByName)) {
    const op = optsByName[nameKey];
    const snapBtn = document.querySelector(`button.btn-snap[data-tap-name="${cssEscape(nameKey)}"]`);
    const modeBtn = document.querySelector(`button.btn-mode[data-tap-name="${cssEscape(nameKey)}"]`);
    if (snapBtn) {
      const s = parseInt(snapBtn.dataset.scopeSlot, 10);
      const b = tapsState.slotWidgets[s];
      if (b?.scope) {
        b.scope.setSnap(!!op.scope_snap);
        snapBtn.classList.toggle("is-active", !!op.scope_snap);
      }
    }
    if (modeBtn) {
      const s2 = parseInt(modeBtn.dataset.spectrumSlot, 10);
      const b2 = tapsState.slotWidgets[s2];
      if (b2?.spectrum) {
        const mode = op.spectrum_heatmap ? "heatmap" : "curve";
        b2.spectrum.setMode(mode);
        modeBtn.textContent = mode === "heatmap" ? "curve" : "heatmap";
      }
    }
  }
}

export function renderTaps(frame) {
  if (!frame?.slots) return;
  for (const s of frame.slots) {
    const b = tapsState.slotWidgets[s.s];
    if (!b) continue;
    if (b.meter) b.meter.update({ peak: s.p, rms: s.r });
    if (b.scope && s.w) b.scope.update(s.w);
    if (b.spectrum && s.m) b.spectrum.update(s.m);
  }
  for (const sl of frame.slots) {
    if (typeof sl.g === "number") {
      applyLed(ledsState.ledNodes[`${sl.s}:gate_led`], "gate_led", sl.g);
    }
    if (sl.t === true) {
      ledsState.triggerFireTime[sl.s] = performance.now();
    }
  }
}
