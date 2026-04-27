/**
 * @quarto/api — format namespace tests (PURE)
 *
 * Mirrors Q1 test cases from: external-sources/quarto-cli/tests/unit/pandoc-formats.test.ts
 * ALL 8 predicates are tested with BOTH a true AND a false case, per §2aa seam-spec.
 * Named revert that reddens each test is noted inline.
 */

import { describe, it, expect } from "vitest";
import type { Format } from "@quarto/types";
import {
  isHtmlCompatible,
  isIpynbOutput,
  isLatexOutput,
  isMarkdownOutput,
  isPresentationOutput,
  isHtmlDashboardOutput,
  isServerShiny,
  isServerShinyPython,
} from "./index.js";

// ─── helpers to build minimal Format objects ──────────────────────────────────

function makeFormat(pandocTo: string, extra?: Partial<Format>): Format {
  return {
    identifier: {},
    language: {},
    metadata: {},
    render: {},
    execute: {},
    pandoc: { to: pandocTo },
    ...extra,
  };
}

// ─── isHtmlCompatible ─────────────────────────────────────────────────────────

describe("format.isHtmlCompatible", () => {
  it("true for html output (pandoc.to='html')", () => {
    // Revert to `return false` → RED
    expect(isHtmlCompatible(makeFormat("html"))).toBe(true);
  });

  it("true for revealjs output", () => {
    expect(isHtmlCompatible(makeFormat("revealjs"))).toBe(true);
  });

  it("true for markdown with prefer-html=true", () => {
    const fmt = makeFormat("markdown", {
      identifier: { "base-format": "markdown" },
      render: { "prefer-html": true },
    });
    expect(isHtmlCompatible(fmt)).toBe(true);
  });

  it("true for ipynb output", () => {
    expect(isHtmlCompatible(makeFormat("ipynb"))).toBe(true);
  });

  it("false for pdf output (pandoc.to='pdf')", () => {
    // Revert to `return true` → RED
    expect(isHtmlCompatible(makeFormat("pdf"))).toBe(false);
  });

  it("false for latex output", () => {
    expect(isHtmlCompatible(makeFormat("latex"))).toBe(false);
  });

  it("false for markdown base-format with prefer-html NOT set (prefer-html false-side)", () => {
    // markdown base, prefer-html omitted → isHtmlOutput false, isIpynbOutput false,
    // prefer-html branch not triggered → false
    // Revert: removing the prefer-html guard branch wouldn't redden any other test;
    // this test binds that branch's false side.
    const fmt = makeFormat("markdown", {
      identifier: { "base-format": "markdown" },
      render: {},
    });
    expect(isHtmlCompatible(fmt)).toBe(false);
  });
});

// ─── isIpynbOutput ────────────────────────────────────────────────────────────

describe("format.isIpynbOutput", () => {
  it("true for pandoc.to='ipynb'", () => {
    // Revert → RED
    expect(isIpynbOutput({ to: "ipynb" })).toBe(true);
  });

  it("false for pandoc.to='html'", () => {
    expect(isIpynbOutput({ to: "html" })).toBe(false);
  });
});

// ─── isLatexOutput ────────────────────────────────────────────────────────────

describe("format.isLatexOutput", () => {
  it("true for pandoc.to='pdf'", () => {
    expect(isLatexOutput({ to: "pdf" })).toBe(true);
  });

  it("true for pandoc.to='latex'", () => {
    expect(isLatexOutput({ to: "latex" })).toBe(true);
  });

  it("true for pandoc.to='beamer'", () => {
    expect(isLatexOutput({ to: "beamer" })).toBe(true);
  });

  it("false for pandoc.to='html'", () => {
    // Revert to `return true` → RED
    expect(isLatexOutput({ to: "html" })).toBe(false);
  });

  it("false for pandoc.to='docx'", () => {
    expect(isLatexOutput({ to: "docx" })).toBe(false);
  });
});

// ─── isMarkdownOutput ─────────────────────────────────────────────────────────

describe("format.isMarkdownOutput", () => {
  it("true when base-format is 'markdown'", () => {
    // Revert → RED
    const fmt = makeFormat("html", { identifier: { "base-format": "markdown" } });
    expect(isMarkdownOutput(fmt)).toBe(true);
  });

  it("true when base-format is 'gfm'", () => {
    const fmt = makeFormat("html", { identifier: { "base-format": "gfm" } });
    expect(isMarkdownOutput(fmt)).toBe(true);
  });

  it("true for ipynb pandoc output (even without markdown base-format)", () => {
    const fmt = makeFormat("ipynb");
    expect(isMarkdownOutput(fmt)).toBe(true);
  });

  it("false for html output with no markdown base-format", () => {
    // Revert to `return true` → RED
    expect(isMarkdownOutput(makeFormat("html"))).toBe(false);
  });

  it("false for pdf output", () => {
    expect(isMarkdownOutput(makeFormat("pdf"))).toBe(false);
  });
});

// ─── isPresentationOutput ────────────────────────────────────────────────────

describe("format.isPresentationOutput", () => {
  it("true for pandoc.to='revealjs'", () => {
    // Revert → RED
    expect(isPresentationOutput({ to: "revealjs" })).toBe(true);
  });

  it("true for pandoc.to='beamer'", () => {
    expect(isPresentationOutput({ to: "beamer" })).toBe(true);
  });

  it("true for pandoc.to='pptx'", () => {
    expect(isPresentationOutput({ to: "pptx" })).toBe(true);
  });

  it("false for pandoc.to='html'", () => {
    // Revert to `return true` → RED
    expect(isPresentationOutput({ to: "html" })).toBe(false);
  });

  it("false when to is undefined", () => {
    expect(isPresentationOutput({})).toBe(false);
  });
});

// ─── isHtmlDashboardOutput ───────────────────────────────────────────────────

describe("format.isHtmlDashboardOutput", () => {
  it("true for 'dashboard'", () => {
    // Revert → RED
    expect(isHtmlDashboardOutput("dashboard")).toBe(true);
  });

  it("true for 'custom-dashboard'", () => {
    expect(isHtmlDashboardOutput("custom-dashboard")).toBe(true);
  });

  it("false for 'html'", () => {
    // Revert to `return true` → RED
    expect(isHtmlDashboardOutput("html")).toBe(false);
  });

  it("false for undefined", () => {
    expect(isHtmlDashboardOutput(undefined)).toBe(false);
  });

  it("false for 'dashboard-extra' (does not end with -dashboard)", () => {
    expect(isHtmlDashboardOutput("dashboard-extra")).toBe(false);
  });
});

// ─── isServerShiny ────────────────────────────────────────────────────────────

describe("format.isServerShiny", () => {
  it("true when metadata.server.type is 'shiny'", () => {
    // Revert → RED
    const fmt = makeFormat("html", {
      metadata: { server: { type: "shiny" } },
    });
    expect(isServerShiny(fmt)).toBe(true);
  });

  it("false when metadata.server is absent", () => {
    // Revert to `return true` → RED
    expect(isServerShiny(makeFormat("html"))).toBe(false);
  });

  it("false when metadata.server.type is not 'shiny'", () => {
    const fmt = makeFormat("html", {
      metadata: { server: { type: "other" } },
    });
    expect(isServerShiny(fmt)).toBe(false);
  });

  it("false for undefined format", () => {
    expect(isServerShiny(undefined)).toBe(false);
  });
});

// ─── isServerShinyPython ─────────────────────────────────────────────────────

describe("format.isServerShinyPython", () => {
  const shinyFmt = makeFormat("html", {
    metadata: { server: { type: "shiny" } },
  });

  it("true when shiny AND engine is 'jupyter'", () => {
    // Revert → RED
    expect(isServerShinyPython(shinyFmt, "jupyter")).toBe(true);
  });

  it("false when shiny but engine is 'knitr'", () => {
    // Revert to `return true` → RED
    expect(isServerShinyPython(shinyFmt, "knitr")).toBe(false);
  });

  it("false when engine is 'jupyter' but not shiny", () => {
    expect(isServerShinyPython(makeFormat("html"), "jupyter")).toBe(false);
  });

  it("false when engine is undefined", () => {
    expect(isServerShinyPython(shinyFmt, undefined)).toBe(false);
  });
});
