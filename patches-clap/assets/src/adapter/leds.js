import { ledColour, parseLedKey, triggerIntensity } from "../core/leds.js";

// Mutable state shared with adapter/taps.js (which writes ledNodes
// inside rebuildTaps) and adapter/taps.js + this module's rAF loop
// (which read both fields).
export const ledsState = {
  ledNodes: Object.create(null),    // (slot+":"+kind) → element
  triggerFireTime: Object.create(null), // slot → last-fire ms
};

export function resetLedNodes() {
  ledsState.ledNodes = Object.create(null);
}

export function applyLed(node, kind, value) {
  if (!node) return;
  const { fill, border } = ledColour(kind, value);
  node.style.backgroundColor = fill;
  node.style.borderColor = border;
}

function tickTriggerLeds() {
  const now = performance.now();
  const { ledNodes, triggerFireTime } = ledsState;
  for (const key of Object.keys(ledNodes)) {
    const parsed = parseLedKey(key);
    if (!parsed || parsed.kind !== "trigger_led") continue;
    const fired = triggerFireTime[parsed.slot];
    const v = typeof fired === "number" ? triggerIntensity(now - fired) : 0;
    if (v === 0 && typeof fired === "number") delete triggerFireTime[parsed.slot];
    applyLed(ledNodes[key], "trigger_led", v);
  }
  requestAnimationFrame(tickTriggerLeds);
}

export function startLedDecayLoop() {
  requestAnimationFrame(tickTriggerLeds);
}
