/**
 * @quarto/api/jupyter — tags tests (PURE)
 *
 * Mirrors Q1 semantics from:
 *   external-sources/quarto-cli/src/core/jupyter/tags.ts
 *
 * Rows 4 and 5 below are FROZEN Test Seam Spec rows (Phase 3B, per
 * plan3-task-3-brief.md). Do not edit their assertions to make a "correct"
 * implementation pass — a conflict between a correct implementation and a
 * frozen row's stated expectation is reported to the controller, not
 * silently patched here.
 */

import { describe, it, expect } from "vitest";
import type { JupyterCellWithOptions, JupyterToMarkdownOptions } from "@quarto/types";
import {
  hideCell,
  hideCode,
  hideOutput,
  hideWarnings,
  includeCell,
  includeCode,
  includeOutput,
  includeWarnings,
  echoFenced,
} from "./tags.js";

// ─── helpers to build minimal cell/options fixtures ────────────────────────

function makeCell(cellOptions: Record<string, unknown> = {}): JupyterCellWithOptions {
  return {
    cell_type: "code",
    metadata: {},
    source: [],
    id: "cell-1",
    options: cellOptions,
    optionsSource: [],
  };
}

// JupyterToMarkdownOptions carries a lot of render-plumbing fields
// (executeOptions, assets, language, ...) that `tags.ts` never reads — only
// `execute` and `keepHidden` matter here. Cast rather than fabricate the
// full interface.
function makeOptions(
  execute: Record<string, unknown> = {},
  keepHidden = false,
): JupyterToMarkdownOptions {
  return {
    execute,
    keepHidden,
  } as unknown as JupyterToMarkdownOptions;
}

// ─── Row 4 ──────────────────────────────────────────────────────────────────
// includeWarnings: a cell with global warning:false + local warning:true =>
// warnings included (true). Discriminator: global must differ from local
// (global=false, local=true) — a case where they agree can't discriminate.
// Named revert: collapse includeWarnings to a flat read of the global
// (`return !!options.execute.warning`), removing both the output-override
// branch AND the cell-priority fallback => reads global false => excluded
// (false) => RED.

describe("includeWarnings — Row 4", () => {
  it("includes warnings when global warning:false is overridden by local warning:true", () => {
    const cell = makeCell({ warning: true });
    const options = makeOptions({ warning: false });
    expect(includeWarnings(cell, options)).toBe(true);
  });
});

// ─── Row 5 ──────────────────────────────────────────────────────────────────
// echoFenced: a cell with echo: "fenced" => the fenced-echo path (returns
// true). Named revert: remove the echoFenced branch (plain echo, i.e.
// `return false`) => RED.

describe("echoFenced — Row 5", () => {
  it("returns true when the cell sets echo: 'fenced'", () => {
    const cell = makeCell({ echo: "fenced" });
    const options = makeOptions({ echo: true });
    expect(echoFenced(cell, options)).toBe(true);
  });

  it("returns false when neither cell nor global echo is 'fenced' (discriminator)", () => {
    const cell = makeCell({ echo: true });
    const options = makeOptions({ echo: true });
    expect(echoFenced(cell, options)).toBe(false);
  });
});

// ─── hideCell ───────────────────────────────────────────────────────────────

describe("hideCell", () => {
  it("hides the cell when global include:false and keepHidden:true", () => {
    const cell = makeCell({});
    const options = makeOptions({ include: false }, true);
    expect(hideCell(cell, options)).toBe(true);
  });

  it("does not hide the cell when global include:true (discriminator)", () => {
    const cell = makeCell({});
    const options = makeOptions({ include: true }, true);
    expect(hideCell(cell, options)).toBe(false);
  });
});

// ─── hideCode ───────────────────────────────────────────────────────────────

describe("hideCode", () => {
  it("hides code when cell-local echo:false and keepHidden:true", () => {
    const cell = makeCell({ echo: false });
    const options = makeOptions({ echo: true }, true);
    expect(hideCode(cell, options)).toBe(true);
  });

  it("does not hide code when cell-local echo:true (discriminator)", () => {
    const cell = makeCell({ echo: true });
    const options = makeOptions({ echo: true }, true);
    expect(hideCode(cell, options)).toBe(false);
  });
});

// ─── hideOutput ─────────────────────────────────────────────────────────────

describe("hideOutput", () => {
  it("hides output when global output:false and keepHidden:true", () => {
    const cell = makeCell({});
    const options = makeOptions({ output: false }, true);
    expect(hideOutput(cell, options)).toBe(true);
  });

  it("does not hide output when keepHidden:false (discriminator)", () => {
    const cell = makeCell({});
    const options = makeOptions({ output: false }, false);
    expect(hideOutput(cell, options)).toBe(false);
  });
});

// ─── hideWarnings ───────────────────────────────────────────────────────────
// Exercises the same output-override branch as Row 4 (global output:false +
// local output not false), but on hideWarnings: the branch returns the
// cell-local warning flag directly (ported verbatim from Q1 — hideWarnings
// and includeWarnings return the SAME value inside this branch; that
// asymmetry is a genuine Q1 source quirk, not something to "fix" here).

describe("hideWarnings", () => {
  it("reflects a truthy cell-local warning under the output-override branch", () => {
    const cell = makeCell({ warning: true });
    const options = makeOptions({ output: false });
    expect(hideWarnings(cell, options)).toBe(true);
  });

  it("reflects a falsy cell-local warning under the output-override branch (discriminator)", () => {
    const cell = makeCell({ warning: false });
    const options = makeOptions({ output: false });
    expect(hideWarnings(cell, options)).toBe(false);
  });
});

// ─── includeCell ────────────────────────────────────────────────────────────

describe("includeCell", () => {
  it("includes the cell when global include:true", () => {
    const cell = makeCell({});
    const options = makeOptions({ include: true });
    expect(includeCell(cell, options)).toBe(true);
  });

  it("excludes the cell when global include:false and keepHidden:false (discriminator)", () => {
    const cell = makeCell({});
    const options = makeOptions({ include: false }, false);
    expect(includeCell(cell, options)).toBe(false);
  });
});

// ─── includeCode ────────────────────────────────────────────────────────────

describe("includeCode", () => {
  it("includes code when cell-local echo:true", () => {
    const cell = makeCell({ echo: true });
    const options = makeOptions({ echo: false });
    expect(includeCode(cell, options)).toBe(true);
  });

  it("excludes code when cell-local echo:false and keepHidden:false (discriminator)", () => {
    const cell = makeCell({ echo: false });
    const options = makeOptions({ echo: false }, false);
    expect(includeCode(cell, options)).toBe(false);
  });
});

// ─── includeOutput ──────────────────────────────────────────────────────────

describe("includeOutput", () => {
  it("excludes output when cell-local output:false and keepHidden:false", () => {
    const cell = makeCell({ output: false });
    const options = makeOptions({}, false);
    expect(includeOutput(cell, options)).toBe(false);
  });

  it("includes output when cell-local output:true (discriminator)", () => {
    const cell = makeCell({ output: true });
    const options = makeOptions({}, false);
    expect(includeOutput(cell, options)).toBe(true);
  });
});
