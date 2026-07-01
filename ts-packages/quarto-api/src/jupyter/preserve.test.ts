/**
 * @quarto/api/jupyter — preserve tests (INERT today)
 *
 * Mirrors Q1 semantics from:
 *   external-sources/quarto-cli/src/core/jupyter/preserve.ts (lines 12-60)
 *
 * Both units below bind the "inert" behavior: `isPreservedHtml` is a
 * constant-`false` no-op, so `removeAndPreserveHtml`'s gated swap never
 * fires. **Named revert** for both: make `isPreservedHtml` return `true`
 * (uncomment the one-line override at the bottom of `preserve.ts`, or edit
 * `isPreservedHtml` directly to `return true;`) — both units go RED. See
 * the report for the RED transcript.
 */

import { describe, it, expect } from "vitest";
import type { JupyterNotebook, JupyterOutputDisplayData } from "@quarto/types";
import { removeAndPreserveHtml, isPreservedHtml } from "./preserve.js";
import { kTextHtml, kTextMarkdown } from "./constants.js";

// ─── isPreservedHtml — constant-false ──────────────────────────────────────
// Discriminator: includes an input that "looks preserved" (contains the
// literal token "preserve") to prove this isn't a substring/heuristic check
// masquerading as constant-false.
// Named revert: make isPreservedHtml return true => all three go RED.

describe("isPreservedHtml — constant-false no-op", () => {
  it("returns false for arbitrary html", () => {
    expect(isPreservedHtml("<div>anything</div>")).toBe(false);
  });

  it("returns false for empty string", () => {
    expect(isPreservedHtml("")).toBe(false);
  });

  it("returns false even for input that looks preserved", () => {
    expect(isPreservedHtml("<!-- preserve this -->")).toBe(false);
  });
});

// ─── removeAndPreserveHtml — inert (gated swap never fires) ────────────────
// Discriminator: builds a notebook whose code-cell output actually has a
// text/html slot (a <table>), so a non-inert implementation would have
// something to swap. Asserts BOTH the return value (empty/undefined) AND
// that the output bundle is byte-for-byte unchanged (text/html still
// present, text/markdown NOT added) — a vacuous check would only look at
// one side.
// Named revert: make isPreservedHtml return true => the swap fires =>
// data["text/html"] deleted + data["text/markdown"] added + map non-empty
// => both assertions below go RED.

function makeNotebookWithHtmlOutput(): JupyterNotebook {
  const displayOutput: JupyterOutputDisplayData = {
    output_type: "display_data",
    data: {
      [kTextHtml]: ["<table><tr><td>1</td></tr></table>"],
    },
    metadata: {},
  };
  return {
    cells: [
      {
        cell_type: "code",
        metadata: {},
        source: ["print('hi')"],
        outputs: [displayOutput],
      },
    ],
    metadata: {
      kernelspec: { name: "python3", language: "python", display_name: "Python 3" },
    },
    nbformat: 4,
    nbformat_minor: 5,
  };
}

describe("removeAndPreserveHtml — inert (isPreservedHtml is constant-false)", () => {
  it("returns no preserve map — nothing is preserved", () => {
    const nb = makeNotebookWithHtmlOutput();
    const preserveMap = removeAndPreserveHtml(nb);
    expect(preserveMap === undefined || Object.keys(preserveMap).length === 0).toBe(
      true,
    );
  });

  it("leaves the output bundle unchanged — text/html untouched, no text/markdown added", () => {
    const nb = makeNotebookWithHtmlOutput();
    removeAndPreserveHtml(nb);
    const output = nb.cells[0].outputs![0] as JupyterOutputDisplayData;
    expect(output.data[kTextHtml]).toEqual(["<table><tr><td>1</td></tr></table>"]);
    expect(output.data[kTextMarkdown]).toBeUndefined();
  });
});
