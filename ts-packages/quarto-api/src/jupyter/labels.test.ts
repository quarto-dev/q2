/**
 * @quarto/api/jupyter — labels tests (PURE)
 *
 * Mirrors Q1 semantics from:
 *   external-sources/quarto-cli/src/core/jupyter/labels.ts
 *   external-sources/quarto-cli/src/core/html.ts:16 (asHtmlId)
 *
 * Rows 6 and 7 below are FROZEN Test Seam Spec rows (Phase 3B,
 * plan3-task-4-brief.md). Do not edit their assertions to make a "correct"
 * implementation pass — a conflict between a correct implementation and a
 * frozen row's stated expectation is reported to the controller, not
 * silently patched here.
 */

import { describe, it, expect } from "vitest";
import type { JupyterCellWithOptions } from "@quarto/types";
import {
  cellLabel,
  cellLabelValidator,
  resolveCaptions,
  asHtmlId,
} from "./labels.js";
import { pandocAutoIdentifier } from "./pandoc-id.js";

// ─── helper to build minimal cell fixtures ─────────────────────────────────

function makeCell(
  cellOptions: Record<string, unknown> = {},
  overrides: Partial<JupyterCellWithOptions> = {},
): JupyterCellWithOptions {
  return {
    cell_type: "code",
    metadata: {},
    source: [],
    id: "cell-1",
    options: cellOptions,
    optionsSource: [],
    ...overrides,
  };
}

// ─── Row 6 ──────────────────────────────────────────────────────────────────
// cellLabelValidator: two cells with the SAME label => the duplicate is
// flagged. Q1 signals a duplicate by throwing an Error (not a return
// value) the second time the validator is called with that label.
// Named revert: remove the dedup check (i.e. always add the label without
// ever checking `cellLabels.has(label)`) => no flag ever raised => RED.

describe("cellLabelValidator — Row 6", () => {
  it("throws on the second cell carrying the same label", () => {
    const validate = cellLabelValidator();
    const first = makeCell({ label: "fig-a" });
    const second = makeCell({ label: "fig-a" }, { id: "cell-2" });

    expect(() => validate(first)).not.toThrow();
    expect(() => validate(second)).toThrow(/duplicate/i);
  });

  it("does not flag two cells with DIFFERENT labels (discriminator)", () => {
    const validate = cellLabelValidator();
    const first = makeCell({ label: "fig-a" });
    const second = makeCell({ label: "fig-b" }, { id: "cell-2" });

    expect(() => validate(first)).not.toThrow();
    expect(() => validate(second)).not.toThrow();
  });
});

// ─── Row 7 ──────────────────────────────────────────────────────────────────
// resolveCaptions: a cell with fig-cap => the caption is returned; a cell
// with tbl-cap => NO caption returned here (boundary — tbl-cap is handled
// by a downstream lua filter, not this module).
// Named revert: remove fig-cap extraction (collapse resolveCaptions to
// always return `{ cellCaption: undefined, outputCaptions: [] }`) => the
// fig-cap positive case goes RED.

describe("resolveCaptions — Row 7", () => {
  it("extracts a fig-cap caption into outputCaptions", () => {
    const cell = makeCell({ "fig-cap": "A figure caption" });
    expect(resolveCaptions(cell)).toEqual({
      cellCaption: undefined,
      outputCaptions: ["A figure caption"],
    });
  });

  it("does NOT extract tbl-cap here (boundary — downstream lua filter's job)", () => {
    const cell = makeCell({ "tbl-cap": "A table caption" });
    expect(resolveCaptions(cell)).toEqual({
      cellCaption: undefined,
      outputCaptions: [],
    });
  });
});

// ─── cellLabel ──────────────────────────────────────────────────────────────

describe("cellLabel", () => {
  it("prefixes the normalized label option with '#'", () => {
    const cell = makeCell({ label: "fig-plot" });
    expect(cellLabel(cell)).toBe("#fig-plot");
  });

  it("falls back to metadata.name when no label option is set", () => {
    const cell = makeCell({}, { metadata: { name: "fig-fallback" } });
    expect(cellLabel(cell)).toBe("#fig-fallback");
  });

  it("returns '' when neither label nor name is present (discriminator)", () => {
    const cell = makeCell({});
    expect(cellLabel(cell)).toBe("");
  });
});

// ─── asHtmlId ───────────────────────────────────────────────────────────────
// asHtmlId(text) === pandocAutoIdentifier(text, false) — always the
// asciify:false path. Distinguish it from pandocAutoIdentifier(text, true)
// with a case where transliteration changes the result (see the frozen
// asciify-arg-binding row in pandoc-id.test.ts for the full derivation of
// "lan-vital" vs "elan-vital").

describe("asHtmlId", () => {
  it('normalizes "Hello, World!" the same way pandocAutoIdentifier(text, false) does', () => {
    expect(asHtmlId("Hello, World!")).toBe(
      pandocAutoIdentifier("Hello, World!", false),
    );
    expect(asHtmlId("Hello, World!")).toBe("hello-world");
  });

  it('matches pandocAutoIdentifier(text, false), NOT pandocAutoIdentifier(text, true) (non-vacuous distinguisher)', () => {
    const text = "Élan Vital";
    expect(asHtmlId(text)).toBe(pandocAutoIdentifier(text, false));
    expect(asHtmlId(text)).not.toBe(pandocAutoIdentifier(text, true));
  });
});
