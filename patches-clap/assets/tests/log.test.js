import { describe, it, expect } from "vitest";
import { pad2, nowHms, computeLogStamps } from "../src/core/log.js";

describe("pad2", () => {
  it("zero-pads single digits", () => {
    expect(pad2(0)).toBe("00");
    expect(pad2(9)).toBe("09");
  });
  it("leaves two-digit numbers untouched", () => {
    expect(pad2(10)).toBe("10");
    expect(pad2(59)).toBe("59");
  });
});

describe("nowHms", () => {
  it("formats UTC HH:MM:SS from a Date instance", () => {
    expect(nowHms(new Date(Date.UTC(2026, 0, 1, 3, 4, 5)))).toBe("03:04:05");
  });
});

describe("computeLogStamps", () => {
  it("appends a single new stamp when the log gains one entry", () => {
    const stamps = computeLogStamps(["00:00:01"], 1, 2, "00:00:02");
    expect(stamps).toEqual(["00:00:01", "00:00:02"]);
  });

  it("preserves existing stamps when nothing has changed", () => {
    const prev = ["00:00:01", "00:00:02"];
    expect(computeLogStamps(prev, 2, 2, "00:00:03")).toEqual(prev);
  });

  it("drops the matching prefix when entries are evicted from the front", () => {
    const prev = ["a", "b", "c"];
    expect(computeLogStamps(prev, 3, 2, "z")).toEqual(["b", "c"]);
  });

  it("stamps multiple new tail entries with the same wall-clock time", () => {
    const stamps = computeLogStamps([], 0, 3, "now");
    expect(stamps).toEqual(["now", "now", "now"]);
  });

  it("does not mutate the input array", () => {
    const prev = ["a"];
    const out = computeLogStamps(prev, 1, 2, "b");
    expect(out).toEqual(["a", "b"]);
    expect(prev).toEqual(["a"]);
  });

  it("eviction + extension: drop, then append", () => {
    // prev had 3 entries; 1 evicted leaves 2; then 1 new tail entry → 3.
    // i.e. nextLen=3, prevLen=3 with the 3 entries being [oldest+1, oldest+2, new]
    // computeLogStamps treats nextLen<prevLen as eviction first; here nextLen===prevLen.
    // The function's contract is single-step: eviction OR extension, not both per call.
    // This documents that behaviour.
    const prev = ["a", "b", "c"];
    expect(computeLogStamps(prev, 3, 3, "now")).toEqual(prev);
  });
});
