import { describe, it, expect } from "vitest";
import { findFirstRisingCross } from "../src/core/scope.js";

function sine(n, cycles) {
  const out = new Float32Array(n);
  for (let i = 0; i < n; i++) out[i] = Math.sin((2 * Math.PI * cycles * i) / n);
  return out;
}

describe("findFirstRisingCross", () => {
  it("returns null on flat zero signal (no peak → eps=0 → never armed)", () => {
    const s = new Float32Array(100);
    expect(findFirstRisingCross(s, 100)).toBe(null);
  });

  it("returns null on DC offset (never dips below -eps)", () => {
    const s = new Float32Array(100).fill(0.5);
    expect(findFirstRisingCross(s, 100)).toBe(null);
  });

  it("finds the first rising zero-cross in a sine wave (after schmitt arm)", () => {
    // 2 cycles in 64 samples; first cycle arms via negative half (i≈32→48),
    // second upward zero-cross at i=48.
    const s = sine(64, 2);
    const k = findFirstRisingCross(s, 64);
    expect(k).not.toBe(null);
    expect(s[k - 1]).toBeLessThan(0);
    expect(s[k]).toBeGreaterThanOrEqual(0);
  });

  it("ignores noise crossings below the eps hysteresis threshold", () => {
    // Big positive peak of 1.0 sets eps = 0.05. Tiny ripple ±0.01 around
    // zero should NOT trigger; only a true dip below -0.05 then rise.
    const s = new Float32Array(50);
    for (let i = 0; i < 20; i++) s[i] = 0.01 * Math.sin(i); // sub-eps ripple
    s[20] = 1.0; // sets the peak → eps = 0.05
    for (let i = 21; i < 30; i++) s[i] = 0.01 * Math.sin(i); // still sub-eps
    // No sample ever goes below -eps (-0.05), so armed never flips true.
    expect(findFirstRisingCross(s, 50)).toBe(null);
  });

  it("triggers on the first armed rising cross", () => {
    const s = new Float32Array(20);
    s[5] = 1.0;            // peak → eps = 0.05
    for (let i = 8; i <= 14; i++) s[i] = -0.5; // sustained dip → armed
    for (let i = 15; i < 20; i++) s[i] = 0.2;  // rise above zero
    const k = findFirstRisingCross(s, 20);
    // armed at j=8; first j where s[j-1]<0 && s[j]>=0 is j=15.
    expect(k).toBe(15);
  });
});
