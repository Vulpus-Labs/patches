const LED_COLOURS = {
  gate_led: [64, 192, 96],
  trigger_led: [224, 160, 64],
};
const LED_DEFAULT = [200, 200, 200];

// Perceptual gamma: low scalar values should read clearly *off* even
// though the audio-side decay is exponential.
const LED_GAMMA = 2.4;

// Border lights only when the dot is bright enough that an unlit
// border would visually disappear.
const BORDER_THRESHOLD = 0.4;

// Compute the styles for a single LED dot from a 0..1 scalar value.
// Returns { fill, border } as CSS rgb(...) strings, or { fill, border: "" }
// when the value is below the border threshold (no border style).
export function ledColour(kind, value) {
  const rgb = LED_COLOURS[kind] ?? LED_DEFAULT;
  let v = value;
  if (!(v > 0)) v = 0;
  if (v > 1) v = 1;
  const lit = Math.pow(v, LED_GAMMA);
  const r = Math.round(rgb[0] * lit);
  const g = Math.round(rgb[1] * lit);
  const b = Math.round(rgb[2] * lit);
  return {
    fill: `rgb(${r},${g},${b})`,
    border: v > BORDER_THRESHOLD ? `rgb(${rgb[0]},${rgb[1]},${rgb[2]})` : "",
  };
}

// Parse a slotWidgets key of form "<slot>:<kind>". Returns null on
// malformed input.
export function parseLedKey(key) {
  const sep = key.indexOf(":");
  if (sep < 0) return null;
  const slot = parseInt(key.slice(0, sep), 10);
  if (Number.isNaN(slot)) return null;
  return { slot, kind: key.slice(sep + 1) };
}

// Trigger flash decay (UI-side). Audio side latches a fired flag and
// the consumer (Rust gui.rs) takes-and-clears once per tap push; JS
// owns the visual decay so it's smooth at frame rate.
export const TRIGGER_DECAY_MS = 140;

// Compute the current intensity of a triggered LED given the elapsed
// time since fire. Returns a value in [0, 1]; values < 0.001 are
// floored to 0 so the rAF caller can drop the entry.
export function triggerIntensity(ageMs) {
  if (!(ageMs >= 0)) return 0;
  const v = Math.exp(-ageMs / TRIGGER_DECAY_MS);
  return v < 0.001 ? 0 : v;
}
