/**
 * @quarto/api/jupyter — display-data tests (PURE)
 *
 * Mirrors Q1 semantics from:
 *   external-sources/quarto-cli/src/core/jupyter/display-data.ts
 *
 * Rows 1 and 2 below are FROZEN Test Seam Spec rows (Phase 3F). Do not edit
 * their assertions to make a "correct" implementation pass — a conflict
 * between a correct implementation and a frozen row's stated expectation is
 * reported to the controller, not silently patched here.
 */

import { describe, it, expect } from "vitest";
import type { JupyterCellOutputData, JupyterOutput, JupyterOutputDisplayData } from "@quarto/types";
import {
  displayDataMimeType,
  displayDataLatexIsMath,
  displayDataWithMarkdownMath,
  displayDataIsImage,
  displayDataIsJson,
  isDisplayData,
  isCaptionableData,
} from "./display-data.js";
import {
  kTextMarkdown,
  kTextHtml,
  kTextPlain,
  kTextLatex,
  kImagePng,
  kImageJpeg,
  kImageSvg,
  kApplicationPdf,
  kApplicationJupyterWidgetView,
} from "./constants.js";

// ─── Row 1 ──────────────────────────────────────────────────────────────────
// displayDataMimeType: bundle with BOTH text/markdown AND text/html,
// options {toMarkdown: true} => returns 'text/markdown'.
// Discriminator: bundle must hold BOTH md+html (else vacuous).
// Named revert: replace the dynamic base-order with a fixed html-first list
// => returns 'text/html' => RED.

describe("displayDataMimeType — Row 1", () => {
  it("prefers text/markdown over text/html when both are present and toMarkdown is set", () => {
    const output: JupyterCellOutputData = {
      data: {
        [kTextMarkdown]: ["# hi"],
        [kTextHtml]: ["<p>hi</p>"],
      },
    };
    const selected = displayDataMimeType(output, {
      toHtml: false,
      toLatex: false,
      toMarkdown: true,
    });
    expect(selected).toBe(kTextMarkdown);
  });
});

// ─── Row 2 ──────────────────────────────────────────────────────────────────
// displayDataIsJson / displayDataMimeType: bundle containing
// application/vnd.jupyter.widget-view+json, options {toHtml: true} =>
// the widget MIME type is selected by displayDataMimeType AND
// displayDataIsJson(selected) === true (the <script> path).
// Named revert: remove the conditional widget-cluster splice => widget
// MIME never chosen => RED.

describe("displayDataMimeType / displayDataIsJson — Row 2", () => {
  it("selects the widget-view MIME type when toHtml is set, and it is classified as json", () => {
    const output: JupyterCellOutputData = {
      data: {
        [kApplicationJupyterWidgetView]: { model_id: "abc123" },
      },
    };
    const selected = displayDataMimeType(output, {
      toHtml: true,
      toLatex: false,
      toMarkdown: false,
    });
    expect(selected).toBe(kApplicationJupyterWidgetView);
    expect(selected).not.toBeNull();
    expect(displayDataIsJson(selected as string)).toBe(true);
  });
});

// ─── Latex predicate units ──────────────────────────────────────────────────
// displayDataLatexIsMath: pure predicate on latex source lines.
// Named revert: change the is-math test to `return true` => the non-math
// case goes RED.

describe("displayDataLatexIsMath", () => {
  it("recognizes inline/display math delimited by $...$", () => {
    expect(displayDataLatexIsMath(["$x^2$"])).toBe(true);
  });

  it("recognizes a \\begin{...}...\\end{...} latex environment", () => {
    expect(displayDataLatexIsMath(["\\begin{matrix}", "1 & 0 \\\\ 0 & 1", "\\end{matrix}"])).toBe(
      true,
    );
  });

  it("returns false for non-math latex text", () => {
    expect(displayDataLatexIsMath(["not math text"])).toBe(false);
  });

  it("returns false for an empty latex array", () => {
    expect(displayDataLatexIsMath([])).toBe(false);
  });
});

// ─── displayDataWithMarkdownMath ────────────────────────────────────────────
// Pre-transform: for a MATH latex-only output, hoists into
// data["text/markdown"]; for a NON-MATH latex-only output, returns the
// output UNCHANGED (no text/markdown slot added).
// Named revert: drop the isMath gate so it always hoists => the non-math
// "unchanged" assertion goes RED.

describe("displayDataWithMarkdownMath", () => {
  it("hoists math latex into data['text/markdown'] when no markdown slot exists", () => {
    const output: JupyterOutputDisplayData = {
      output_type: "display_data",
      data: {
        [kTextLatex]: ["$x^2$"],
      },
      metadata: {},
    };
    const result = displayDataWithMarkdownMath(output);
    expect(result.data[kTextMarkdown]).toEqual(["$x^2$"]);
  });

  it("leaves a non-math latex-only output unchanged (no text/markdown slot added)", () => {
    const output: JupyterOutputDisplayData = {
      output_type: "display_data",
      data: {
        [kTextLatex]: ["not math text"],
      },
      metadata: {},
    };
    const result = displayDataWithMarkdownMath(output);
    expect(result.data[kTextMarkdown]).toBeUndefined();
    expect(result).toEqual(output);
  });

  it("leaves output unchanged when a text/markdown slot already exists", () => {
    const output: JupyterOutputDisplayData = {
      output_type: "display_data",
      data: {
        [kTextLatex]: ["$x^2$"],
        [kTextMarkdown]: ["already here"],
      },
      metadata: {},
    };
    const result = displayDataWithMarkdownMath(output);
    expect(result.data[kTextMarkdown]).toEqual(["already here"]);
  });
});

// ─── displayDataIsImage ─────────────────────────────────────────────────────
// Focused unit: png/jpeg/svg/pdf are true; a non-image (text/plain) is
// false.

describe("displayDataIsImage", () => {
  it("is true for image/png, image/jpeg, image/svg+xml, application/pdf", () => {
    expect(displayDataIsImage(kImagePng)).toBe(true);
    expect(displayDataIsImage(kImageJpeg)).toBe(true);
    expect(displayDataIsImage(kImageSvg)).toBe(true);
    expect(displayDataIsImage(kApplicationPdf)).toBe(true);
  });

  it("is false for a non-image mime type (text/plain)", () => {
    expect(displayDataIsImage(kTextPlain)).toBe(false);
  });
});

// ─── displayDataIsJson ──────────────────────────────────────────────────────
// Focused unit: true only for the two widget MIME types (false for
// application/json and text/html) — locks the "only widget MIME types"
// correction (P3-10). There is no generic application/json path.

describe("displayDataIsJson", () => {
  it("is true for the two widget MIME types", () => {
    expect(displayDataIsJson("application/vnd.jupyter.widget-state+json")).toBe(true);
    expect(displayDataIsJson(kApplicationJupyterWidgetView)).toBe(true);
  });

  it("is false for a generic application/json mime type (no generic json path)", () => {
    expect(displayDataIsJson("application/json")).toBe(false);
  });

  it("is false for text/html", () => {
    expect(displayDataIsJson(kTextHtml)).toBe(false);
  });
});

// ─── isDisplayData / isCaptionableData ──────────────────────────────────────
// Newly-exported (consolidated from four/two byte-identical local copies in
// to-markdown.ts/labels.ts/widgets.ts/preserve.ts — see Plan 3 cleanup).
// isDisplayData is a type guard: true for display_data/execute_result,
// false for stream/error. isCaptionableData additionally requires the
// absence of `noCaption`.

describe("isDisplayData", () => {
  it("is true for a display_data output", () => {
    const output: JupyterOutput = { output_type: "display_data", data: {}, metadata: {} };
    expect(isDisplayData(output)).toBe(true);
  });

  it("is true for an execute_result output", () => {
    const output: JupyterOutput = {
      output_type: "execute_result",
      data: {},
      metadata: {},
      execution_count: 1,
    } as JupyterOutput;
    expect(isDisplayData(output)).toBe(true);
  });

  it("is false for a stream output", () => {
    const output: JupyterOutput = { output_type: "stream", name: "stdout", text: ["hi"] };
    expect(isDisplayData(output)).toBe(false);
  });

  it("is false for an error output", () => {
    const output: JupyterOutput = {
      output_type: "error",
      ename: "ValueError",
      evalue: "bad",
      traceback: [],
    } as JupyterOutput;
    expect(isDisplayData(output)).toBe(false);
  });
});

describe("isCaptionableData", () => {
  it("is true for a display_data output without noCaption set", () => {
    const output: JupyterOutput = { output_type: "display_data", data: {}, metadata: {} };
    expect(isCaptionableData(output)).toBe(true);
  });

  it("is false for a display_data output with noCaption set", () => {
    const output: JupyterOutputDisplayData = {
      output_type: "display_data",
      data: {},
      metadata: {},
      noCaption: true,
    };
    expect(isCaptionableData(output)).toBe(false);
  });

  it("is false for a non-display-data output (e.g. stream)", () => {
    const output: JupyterOutput = { output_type: "stream", name: "stdout", text: ["hi"] };
    expect(isCaptionableData(output)).toBe(false);
  });
});
