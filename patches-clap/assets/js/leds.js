// Trigger flash decay (UI-side). Audio side latches a fired flag and
// the consumer (Rust gui.rs) takes-and-clears once per tap push; JS
// owns the visual decay so it's smooth at frame rate rather than
// quantised to the ~30 Hz tap-push cadence.
const TRIGGER_DECAY_MS = 140;

// LED on-colour per kind, used as the lit-state RGB. Brightness is
// modulated continuously by the scalar (0..1) so the dot fades with
// the trigger / gate decay tail rather than snapping on/off.
const LED_COLOURS = {
  gate_led: [64, 192, 96],     // #40c060
  trigger_led: [224, 160, 64], // #e0a040
};
// Perceptual gamma: low scalar values should read clearly *off* even
// though the audio-side decay is exponential. Rapid retriggers leave a
// visible afterglow rather than a constant-on impression.
const LED_GAMMA = 2.4;

// Mutated by taps.js (rebuildTaps) and read by tickTriggerLeds + taps render.
let ledNodes = Object.create(null);    // (slot+":"+kind) → element
const triggerFireTime = Object.create(null); // slot → last-fire ms (perf clock)

function applyLed(node, kind, value) {
  if (!node) return;
  const rgb = LED_COLOURS[kind] ?? [200, 200, 200];
  let v = value;
  if (!(v > 0)) v = 0;
  if (v > 1) v = 1;
  const lit = Math.pow(v, LED_GAMMA);
  const r = Math.round(rgb[0] * lit);
  const g = Math.round(rgb[1] * lit);
  const b = Math.round(rgb[2] * lit);
  node.style.backgroundColor = `rgb(${r},${g},${b})`;
  node.style.borderColor = v > 0.4 ? `rgb(${rgb[0]},${rgb[1]},${rgb[2]})` : "";
}

// rAF loop driving trigger LED decays. Cheap: handful of nodes, one
// style write each per frame, only while triggers exist.
function tickTriggerLeds() {
  const now = performance.now();
  for (const key of Object.keys(ledNodes)) {
    const sep = key.indexOf(":");
    if (sep < 0) continue;
    if (key.slice(sep + 1) !== "trigger_led") continue;
    const slot = parseInt(key.slice(0, sep), 10);
    const fired = triggerFireTime[slot];
    let v = 0;
    if (typeof fired === "number") {
      const age = now - fired;
      v = Math.exp(-age / TRIGGER_DECAY_MS);
      if (v < 0.001) {
        v = 0;
        delete triggerFireTime[slot];
      }
    }
    applyLed(ledNodes[key], "trigger_led", v);
  }
  requestAnimationFrame(tickTriggerLeds);
}
requestAnimationFrame(tickTriggerLeds);
