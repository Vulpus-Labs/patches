import { describe, it, expect } from "vitest";
import { changeIntent } from "../src/core/intents.js";

// Minimal stand-in for an HTMLElement select with classList and dataset.
function selectStub({ classes = ["tap-opt"], dataset = {}, value = "" } = {}) {
  return {
    classList: { contains: (c) => classes.includes(c) },
    dataset,
    value: String(value),
  };
}

describe("changeIntent", () => {
  it("returns null for null target", () => {
    expect(changeIntent(null)).toBe(null);
  });

  it("returns null for elements without the tap-opt class", () => {
    expect(changeIntent(selectStub({ classes: [] }))).toBe(null);
  });

  it("returns null when the tap name is missing", () => {
    expect(changeIntent(selectStub({ dataset: { scopeOpt: "decimation" }, value: "8" })))
      .toBe(null);
  });

  it("builds a scope decimation payload", () => {
    const t = selectStub({
      dataset: { tapName: "x", scopeOpt: "decimation" },
      value: "16",
    });
    expect(changeIntent(t)).toEqual({
      kind: "set_tap_opt", name: "x", scope_decimation: 16,
    });
  });

  it("builds a scope window samples payload", () => {
    const t = selectStub({
      dataset: { tapName: "y", scopeOpt: "window" },
      value: "2048",
    });
    expect(changeIntent(t)).toEqual({
      kind: "set_tap_opt", name: "y", scope_window_samples: 2048,
    });
  });

  it("builds a spectrum FFT size payload", () => {
    const t = selectStub({
      dataset: { tapName: "z", spectrumOpt: "fft_size" },
      value: "4096",
    });
    expect(changeIntent(t)).toEqual({
      kind: "set_tap_opt", name: "z", spectrum_fft_size: 4096,
    });
  });

  it("returns null for tap-opt with an unrecognised dataset shape", () => {
    const t = selectStub({ dataset: { tapName: "x", scopeOpt: "weird" }, value: "1" });
    expect(changeIntent(t)).toBe(null);
  });
});
