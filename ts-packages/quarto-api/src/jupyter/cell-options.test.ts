/**
 * @quarto/api/jupyter — cell-options tests (PURE)
 *
 * Mirrors Q1 semantics from:
 *   external-sources/quarto-cli/src/core/lib/partition-cell-options.ts
 *
 * Row 8 below is a FROZEN Test Seam Spec row (Phase 3B, plan3-task-5-brief.md).
 * Do not edit its assertion to make a "correct" implementation pass — a
 * conflict between a correct implementation and a frozen row's stated
 * expectation is reported to the controller, not silently patched here.
 */

import { describe, it, expect } from "vitest";
import { parseCellOptions } from "./cell-options.js";

describe("parseCellOptions", () => {
  // ─── Row 8 (FROZEN) ───────────────────────────────────────────────────────
  it("Row 8: parses a leading `#| label: fig-1` line for python", () => {
    const source = ["#| label: fig-1", "print('hi')"];
    expect(parseCellOptions(source, "python").options.label).toBe("fig-1");
  });

  // ─── focused units ──────────────────────────────────────────────────────

  it("parses multiple contiguous option lines", () => {
    const source = [
      "#| label: fig-1",
      '#| fig-cap: "A cap"',
      "print('hi')",
    ];
    const result = parseCellOptions(source, "python");
    expect(result.options).toEqual({ label: "fig-1", "fig-cap": "A cap" });
    expect(result.optionsSource).toEqual([
      "#| label: fig-1",
      '#| fig-cap: "A cap"',
    ]);
  });

  it("returns {} and [] for a cell with no option lines, leaving code untouched", () => {
    const source = ["print('hi')", "print('bye')"];
    const result = parseCellOptions(source, "python");
    expect(result.options).toEqual({});
    expect(result.optionsSource).toEqual([]);
    // first code line is not consumed / mutated
    expect(source[0]).toBe("print('hi')");
  });

  it("handles a tuple [open, close] comment-char language (c) via the open marker", () => {
    const source = ["/*| label: x", "int main() {}"];
    const result = parseCellOptions(source, "c");
    // We assert only that the tuple case doesn't crash and that the
    // open-marker prefix is recognized as an option line (binding the
    // `string | [string, string]` branch) — Q1 does not exercise `c` cell
    // options in practice, so we don't assert a specific parsed value here,
    // only that the line is captured verbatim in optionsSource and parsing
    // completes without throwing.
    expect(() => parseCellOptions(source, "c")).not.toThrow();
    expect(result.optionsSource).toEqual(["/*| label: x"]);
  });

  it("does not affect the plain-string comment-char path (python) when exercising the tuple branch", () => {
    const source = ["#| label: fig-1", "print('hi')"];
    const result = parseCellOptions(source, "python");
    expect(result.options).toEqual({ label: "fig-1" });
  });

  it("returns {} rather than throwing on a malformed yaml block", () => {
    const source = ["#| label: [unterminated", "print('hi')"];
    expect(() => parseCellOptions(source, "python")).not.toThrow();
    const result = parseCellOptions(source, "python");
    expect(result.options).toEqual({});
  });
});
