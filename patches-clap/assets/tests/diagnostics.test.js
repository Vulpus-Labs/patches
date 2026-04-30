import { describe, it, expect } from "vitest";
import { formatDiagnostic } from "../src/core/diagnostics.js";

describe("formatDiagnostic", () => {
  it("uses 'note' as default severity class", () => {
    expect(formatDiagnostic({}).sevClass).toBe("sev-note");
  });

  it("uses provided severity in the class suffix", () => {
    expect(formatDiagnostic({ severity: "error" }).sevClass).toBe("sev-error");
  });

  it("falls back to empty string when message is missing", () => {
    expect(formatDiagnostic({}).message).toBe("");
  });

  it("returns metaText null when no parts present (renderer skips meta row)", () => {
    expect(formatDiagnostic({ message: "x" }).metaText).toBe(null);
  });

  it("joins location, label, and bracketed code with two spaces", () => {
    const out = formatDiagnostic({
      location: "foo.patches:3:1",
      label: "unknown_module",
      code: "E007",
    });
    expect(out.metaText).toBe("foo.patches:3:1  unknown_module  [E007]");
  });

  it("includes only the parts that are present", () => {
    expect(formatDiagnostic({ code: "W42" }).metaText).toBe("[W42]");
    expect(formatDiagnostic({ label: "x" }).metaText).toBe("x");
  });
});
