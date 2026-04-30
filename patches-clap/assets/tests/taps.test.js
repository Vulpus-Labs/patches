import { describe, it, expect } from "vitest";
import {
  tapsSignature, foldStereoPairs, collectLeds, tapOptValue, cssEscape,
} from "../src/core/taps.js";

describe("tapsSignature", () => {
  it("returns empty string for null/undefined", () => {
    expect(tapsSignature(null)).toBe("");
    expect(tapsSignature(undefined)).toBe("");
  });

  it("encodes slot, name, and components", () => {
    const sig = tapsSignature([
      { slot: 0, name: "a", components: ["meter"] },
      { slot: 1, name: "b", components: ["osc", "spectrum"] },
    ]);
    expect(sig).toBe("0:a:meter|1:b:osc,spectrum");
  });

  it("treats missing components as empty", () => {
    expect(tapsSignature([{ slot: 0, name: "x" }])).toBe("0:x:");
  });

  it("changes when component order changes (order-sensitive)", () => {
    const a = tapsSignature([{ slot: 0, name: "x", components: ["a", "b"] }]);
    const b = tapsSignature([{ slot: 0, name: "x", components: ["b", "a"] }]);
    expect(a).not.toBe(b);
  });
});

describe("foldStereoPairs", () => {
  const stereoTap = (slot, name) => ({
    slot, name, components: ["stereo_meter"],
  });

  it("folds consecutive /left + /right halves at consecutive slots", () => {
    const out = foldStereoPairs([
      stereoTap(0, "bus/left"),
      stereoTap(1, "bus/right"),
    ]);
    expect(out).toEqual([
      { stereo: true, stem: "bus", left: stereoTap(0, "bus/left"), right: stereoTap(1, "bus/right") },
    ]);
  });

  it("does not fold when stems differ", () => {
    const out = foldStereoPairs([
      stereoTap(0, "a/left"),
      stereoTap(1, "b/right"),
    ]);
    expect(out).toHaveLength(2);
    expect(out[0].stereo).toBe(false);
    expect(out[1].stereo).toBe(false);
  });

  it("does not fold when slots are not consecutive", () => {
    const out = foldStereoPairs([
      stereoTap(0, "x/left"),
      stereoTap(2, "x/right"),
    ]);
    expect(out).toHaveLength(2);
    expect(out[0].stereo).toBe(false);
  });

  it("does not fold when the right half is missing", () => {
    const out = foldStereoPairs([stereoTap(0, "lonely/left")]);
    expect(out).toEqual([{ stereo: false, tap: stereoTap(0, "lonely/left") }]);
  });

  it("does not fold a /left without the stereo_meter component", () => {
    const left = { slot: 0, name: "x/left", components: ["meter"] };
    const right = { slot: 1, name: "x/right", components: ["meter"] };
    const out = foldStereoPairs([left, right]);
    expect(out).toHaveLength(2);
    expect(out[0].stereo).toBe(false);
  });

  it("interleaves folded and unfolded entries", () => {
    const mono = { slot: 2, name: "solo", components: ["meter"] };
    const out = foldStereoPairs([
      stereoTap(0, "p/left"),
      stereoTap(1, "p/right"),
      mono,
    ]);
    expect(out).toHaveLength(2);
    expect(out[0].stereo).toBe(true);
    expect(out[1]).toEqual({ stereo: false, tap: mono });
  });
});

describe("collectLeds", () => {
  it("extracts gate_led and trigger_led components, ignoring others", () => {
    const out = collectLeds([
      { slot: 0, name: "a", components: ["gate_led", "meter"] },
      { slot: 1, name: "b", components: ["spectrum"] },
      { slot: 2, name: "c", components: ["trigger_led", "gate_led"] },
    ]);
    expect(out).toEqual([
      { slot: 0, name: "a", kind: "gate_led" },
      { slot: 2, name: "c", kind: "trigger_led" },
      { slot: 2, name: "c", kind: "gate_led" },
    ]);
  });

  it("handles taps without a components field", () => {
    expect(collectLeds([{ slot: 0, name: "x" }])).toEqual([]);
  });
});

describe("tapOptValue", () => {
  it("reads scope decimation", () => {
    expect(tapOptValue({ scopeOpt: "decimation" }, { scope_decimation: 8 })).toBe(8);
  });

  it("reads scope window samples", () => {
    expect(tapOptValue({ scopeOpt: "window" }, { scope_window_samples: 1024 })).toBe(1024);
  });

  it("reads spectrum FFT size", () => {
    expect(tapOptValue({ spectrumOpt: "fft_size" }, { spectrum_fft_size: 2048 })).toBe(2048);
  });

  it("returns null for unknown dataset shape", () => {
    expect(tapOptValue({}, {})).toBe(null);
    expect(tapOptValue({ scopeOpt: "wat" }, { scope_decimation: 1 })).toBe(null);
  });

  it("returns null when the field is absent on the opts record", () => {
    expect(tapOptValue({ scopeOpt: "decimation" }, {})).toBe(null);
  });
});

describe("cssEscape", () => {
  it("escapes double quotes and backslashes", () => {
    expect(cssEscape('a"b\\c')).toBe('a\\"b\\\\c');
  });
});
