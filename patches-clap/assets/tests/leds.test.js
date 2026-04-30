import { describe, it, expect } from "vitest";
import {
  ledColour, parseLedKey, triggerIntensity, TRIGGER_DECAY_MS,
} from "../src/core/leds.js";

describe("ledColour", () => {
  it("returns black fill and no border at value 0", () => {
    const { fill, border } = ledColour("gate_led", 0);
    expect(fill).toBe("rgb(0,0,0)");
    expect(border).toBe("");
  });

  it("returns full intensity fill and lit border at value 1", () => {
    const { fill, border } = ledColour("gate_led", 1);
    expect(fill).toBe("rgb(64,192,96)");
    expect(border).toBe("rgb(64,192,96)");
  });

  it("clamps values above 1 to value=1", () => {
    expect(ledColour("gate_led", 5)).toEqual(ledColour("gate_led", 1));
  });

  it("clamps NaN / negative / undefined to value=0", () => {
    const off = ledColour("gate_led", 0);
    expect(ledColour("gate_led", -1)).toEqual(off);
    expect(ledColour("gate_led", NaN)).toEqual(off);
    expect(ledColour("gate_led", undefined)).toEqual(off);
  });

  it("falls back to neutral grey for unknown kind", () => {
    const { fill } = ledColour("unknown_kind", 1);
    expect(fill).toBe("rgb(200,200,200)");
  });

  it("border lights only above the threshold (0.4)", () => {
    expect(ledColour("trigger_led", 0.39).border).toBe("");
    expect(ledColour("trigger_led", 0.5).border).toBe("rgb(224,160,64)");
  });
});

describe("parseLedKey", () => {
  it("parses valid slot:kind keys", () => {
    expect(parseLedKey("3:gate_led")).toEqual({ slot: 3, kind: "gate_led" });
    expect(parseLedKey("12:trigger_led")).toEqual({ slot: 12, kind: "trigger_led" });
  });

  it("returns null when colon is missing", () => {
    expect(parseLedKey("nope")).toBe(null);
  });

  it("returns null when slot prefix is not numeric", () => {
    expect(parseLedKey("abc:gate_led")).toBe(null);
  });
});

describe("triggerIntensity", () => {
  it("returns 0 for negative or NaN ages", () => {
    expect(triggerIntensity(-1)).toBe(0);
    expect(triggerIntensity(NaN)).toBe(0);
  });

  it("returns ~1 immediately after fire", () => {
    expect(triggerIntensity(0)).toBeCloseTo(1, 10);
  });

  it("decays exponentially with TRIGGER_DECAY_MS time constant", () => {
    expect(triggerIntensity(TRIGGER_DECAY_MS)).toBeCloseTo(Math.E ** -1, 10);
  });

  it("floors to 0 once below 0.001 (allows caller to drop entry)", () => {
    expect(triggerIntensity(TRIGGER_DECAY_MS * 10)).toBe(0);
  });
});
