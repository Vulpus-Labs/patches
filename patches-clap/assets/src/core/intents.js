// Map a `change` event on a select.tap-opt element to an IPC payload.
// Returns null if the event target is not relevant.
export function changeIntent(target) {
  if (!target?.classList?.contains("tap-opt")) return null;
  const name = target.dataset.tapName;
  if (!name) return null;
  const payload = { kind: "set_tap_opt", name };
  if (target.dataset.spectrumOpt === "fft_size") {
    payload.spectrum_fft_size = parseInt(target.value, 10);
  } else if (target.dataset.scopeOpt === "decimation") {
    payload.scope_decimation = parseInt(target.value, 10);
  } else if (target.dataset.scopeOpt === "window") {
    payload.scope_window_samples = parseInt(target.value, 10);
  } else {
    return null;
  }
  return payload;
}
