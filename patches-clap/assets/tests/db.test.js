import { describe, it, expect } from "vitest";
import { DB_FLOOR, ampToDb, dbToRatio, dbColour } from "../src/core/db.js";

describe("ampToDb", () => {
  it("clamps non-positive amplitudes to DB_FLOOR", () => {
    expect(ampToDb(0)).toBe(DB_FLOOR);
    expect(ampToDb(-0.5)).toBe(DB_FLOOR);
  });

  it("returns 0 dB for unity amplitude", () => {
    expect(ampToDb(1)).toBeCloseTo(0, 10);
  });

  it("returns -20 dB for amplitude 0.1", () => {
    expect(ampToDb(0.1)).toBeCloseTo(-20, 10);
  });

  it("clamps very small amplitudes to DB_FLOOR", () => {
    expect(ampToDb(1e-10)).toBe(DB_FLOOR);
  });
});

describe("dbToRatio", () => {
  it("maps DB_FLOOR to 0", () => {
    expect(dbToRatio(DB_FLOOR)).toBe(0);
  });

  it("maps 0 dB to 1", () => {
    expect(dbToRatio(0)).toBe(1);
  });

  it("maps positive dB to 1 (clamp)", () => {
    expect(dbToRatio(6)).toBe(1);
  });

  it("maps below floor to 0 (clamp)", () => {
    expect(dbToRatio(-100)).toBe(0);
  });
});

describe("dbColour", () => {
  it("red at and above -6 dB", () => {
    expect(dbColour(-6)).toBe("#e04040");
    expect(dbColour(0)).toBe("#e04040");
  });

  it("amber between -18 and -6 dB", () => {
    expect(dbColour(-18)).toBe("#e0a040");
    expect(dbColour(-7)).toBe("#e0a040");
  });

  it("green below -18 dB", () => {
    expect(dbColour(-19)).toBe("#40c060");
    expect(dbColour(-60)).toBe("#40c060");
  });
});
