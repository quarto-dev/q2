/**
 * Tests for src/metadata-as-format.ts — Q1 metadataAsFormat partition (seam T1).
 *
 * TDD: these tests were written BEFORE metadata-as-format.ts existed. Each named-revert
 * test has a specific code change that makes it go RED.
 *
 * Parity source: external-sources/quarto-cli/src/config/metadata.ts:165-239
 */
import { describe, it, expect } from "vitest";
import { applyExecuteDefaults, metadataAsFormat } from "./metadata-as-format.js";
import type { TsFormatInfo } from "./types.js";
import type { Format } from "@quarto/types";

// ---------------------------------------------------------------------------
// T1.1 — nested-bin peel
// Revert: remove Stage-1 nested-bin peel → echo misfiles into format.metadata
// ---------------------------------------------------------------------------
describe("T1.1 nested-bin peel", () => {
  it("routes a nested execute block into format.execute", () => {
    const fi: TsFormatInfo = {
      identifier: { "base-format": "html", "target-format": "", "display-name": "" },
      metadata: { execute: { echo: false } },
    };
    const fmt = metadataAsFormat(fi);
    expect(fmt.execute["echo"]).toBe(false);
    // Confirm it did NOT also land in metadata
    expect(fmt.metadata["execute"]).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// T1.2 — each flat key → its bin (no named revert required, but must pass)
// Note: `extension-name` is used for the identifier routing test because it is
// optional in TsFormatIdentifier. If `target-format` were used, the explicit
// identifier's `target-format: ""` would override it at merge time, making the
// routing invisible. `extension-name` is absent from the explicit identifier here,
// so the flat-routed value survives to the final output.
// ---------------------------------------------------------------------------
describe("T1.2 flat key routing", () => {
  it("routes unambiguous flat keys to correct bins", () => {
    const fi: TsFormatInfo = {
      identifier: { "base-format": "html", "target-format": "", "display-name": "" },
      metadata: {
        "keep-tex": true,           // render-only
        "eval": false,              // execute-only
        "toc": true,                // pandoc-only
        "extension-name": "pptx",  // identifier (optional in explicit; not overridden)
      },
    };
    const fmt = metadataAsFormat(fi);
    expect(fmt.render["keep-tex"]).toBe(true);
    expect(fmt.execute["eval"]).toBe(false);
    expect(fmt.pandoc["toc"]).toBe(true);
    expect(fmt.identifier["extension-name"]).toBe("pptx");
  });
});

// ---------------------------------------------------------------------------
// T1.3 — pdf-standard order discriminator
// Revert: swap Stage-2 order to check pandoc before render → pdf-standard lands in pandoc
// ---------------------------------------------------------------------------
describe("T1.3 pdf-standard render-before-pandoc", () => {
  it("routes pdf-standard to render (not pandoc) because render is checked first", () => {
    const fi: TsFormatInfo = {
      identifier: { "base-format": "html", "target-format": "", "display-name": "" },
      metadata: { "pdf-standard": "1b" },
    };
    const fmt = metadataAsFormat(fi);
    expect(fmt.render["pdf-standard"]).toBe("1b");
    expect(fmt.pandoc["pdf-standard"]).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// T1.4 — server normalization (no named revert required)
// ---------------------------------------------------------------------------
describe("T1.4 server normalization", () => {
  it("converts a string server value to { type: <string> }", () => {
    const fi: TsFormatInfo = {
      identifier: { "base-format": "html", "target-format": "", "display-name": "" },
      metadata: { "server": "shiny" },
    };
    const fmt = metadataAsFormat(fi);
    expect(fmt.metadata["server"]).toEqual({ type: "shiny" });
  });
});

// ---------------------------------------------------------------------------
// T1.5 — ipynb coalesce
// Revert: remove the coalesce step → ipynb-filters is undefined / singular survives
// ---------------------------------------------------------------------------
describe("T1.5 ipynb-filter coalesced to ipynb-filters", () => {
  it("moves ipynb-filter into ipynb-filters array and deletes singular", () => {
    const fi: TsFormatInfo = {
      identifier: { "base-format": "html", "target-format": "", "display-name": "" },
      metadata: { "ipynb-filter": "pandoc-quarto" },
    };
    const fmt = metadataAsFormat(fi);
    expect(fmt.execute["ipynb-filters"]).toEqual(["pandoc-quarto"]);
    expect(fmt.execute["ipynb-filter"]).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// T1.6 — gfm variant expansion (no named revert required)
// ---------------------------------------------------------------------------
describe("T1.6 gfm variant expansion", () => {
  it("replaces gfm prefix in variant with the full commonmark extension string", () => {
    const fi: TsFormatInfo = {
      identifier: { "base-format": "html", "target-format": "", "display-name": "" },
      metadata: { "variant": "gfm+footnotes" },
    };
    const fmt = metadataAsFormat(fi);
    // kGfmCommonmarkVariant = "+autolink_bare_uris+emoji+footnotes+gfm_auto_identifiers+pipe_tables+strikeout+task_lists+tex_math_dollars"
    const expectedVariant =
      "+autolink_bare_uris+emoji+footnotes+gfm_auto_identifiers+pipe_tables+strikeout+task_lists+tex_math_dollars+footnotes";
    expect(fmt.render["variant"]).toBe(expectedVariant);
  });
});

// ---------------------------------------------------------------------------
// T1.7 — flat language-ish → metadata, NOT language
// Revert: add a flat-key → format.language branch in Stage 2 → key lands in language
// ---------------------------------------------------------------------------
describe("T1.7 flat language-keyed value routes to metadata (not language)", () => {
  it("sends toc-title-document to metadata, never to language", () => {
    const fi: TsFormatInfo = {
      identifier: { "base-format": "html", "target-format": "", "display-name": "" },
      metadata: { "toc-title-document": "Contents" },
    };
    const fmt = metadataAsFormat(fi);
    expect(fmt.metadata["toc-title-document"]).toBe("Contents");
    expect(fmt.language["toc-title-document"]).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// T1.8 — nested language peel
// Revert: drop 'language' from the Stage-1 bin-name set → nested block falls to metadata
// ---------------------------------------------------------------------------
describe("T1.8 nested language peel", () => {
  it("routes a nested language block into format.language", () => {
    const fi: TsFormatInfo = {
      identifier: { "base-format": "html", "target-format": "", "display-name": "" },
      metadata: { language: { "callout-note-title": "Note" } },
    };
    const fmt = metadataAsFormat(fi);
    expect(fmt.language["callout-note-title"]).toBe("Note");
    expect(fmt.metadata["language"]).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// T1.9 — move-not-duplicate
// Revert: also write each classified key into format.metadata → format.metadata['eval'] defined
// ---------------------------------------------------------------------------
describe("T1.9 move-not-duplicate", () => {
  it("a key routed to execute is absent from metadata", () => {
    const fi: TsFormatInfo = {
      identifier: { "base-format": "html", "target-format": "", "display-name": "" },
      metadata: { "eval": false },
    };
    const fmt = metadataAsFormat(fi);
    expect(fmt.execute["eval"]).toBe(false);
    expect(fmt.metadata["eval"]).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// T1.10 — explicit identifier wins (no named revert required)
// ---------------------------------------------------------------------------
describe("T1.10 explicit identifier wins over flat-key classified", () => {
  it("explicit identifier fields override any flat-key classified identifier entries", () => {
    const fi: TsFormatInfo = {
      identifier: {
        "base-format": "html",
        "target-format": "revealjs",
        "display-name": "Reveal",
      },
      metadata: {
        "display-name": "FlatName",   // flat key routes to identifier
        "target-format": "html5",     // flat key routes to identifier
      },
    };
    const fmt = metadataAsFormat(fi);
    // Explicit wins for display-name
    expect(fmt.identifier["display-name"]).toBe("Reveal");
    // Explicit wins for target-format
    expect(fmt.identifier["target-format"]).toBe("revealjs");
    // Explicit-only field present
    expect(fmt.identifier["base-format"]).toBe("html");
  });
});

// ---------------------------------------------------------------------------
// T1.11 — applyExecuteDefaults fills Q1 base execute-visibility defaults
//
// The Julia engine (first real jupyterToMarkdown consumer) drops every cell
// when format.execute.include / .output / .eval are undefined. applyExecuteDefaults
// fills the Q1 base defaults (formats-shared.ts:210-217) for absent keys only.
//
// Named revert: make applyExecuteDefaults a no-op (return format unchanged) →
// include/output/eval undefined → the "defaults present" expectations go RED.
// ---------------------------------------------------------------------------
describe("T1.11 applyExecuteDefaults", () => {
  it("fills absent execute-visibility keys with Q1 base defaults", () => {
    // A doc that only set execute.daemon (like the Plan-4B minimal doc).
    const fmt = applyExecuteDefaults(
      metadataAsFormat({
        identifier: { "base-format": "html", "target-format": "", "display-name": "" },
        metadata: { execute: { daemon: false } },
      }),
    );
    // Base defaults filled (these gate includeCell / includeCode / includeOutput).
    expect(fmt.execute["include"]).toBe(true);
    expect(fmt.execute["eval"]).toBe(true);
    expect(fmt.execute["output"]).toBe(true);
    expect(fmt.execute["echo"]).toBe(true);
    expect(fmt.execute["warning"]).toBe(true);
    expect(fmt.execute["error"]).toBe(false);
    // The document's own value is preserved (not clobbered by a default).
    expect(fmt.execute["daemon"]).toBe(false);
  });

  it("does not override a document-set visibility key (echo: false wins)", () => {
    const fmt = applyExecuteDefaults(
      metadataAsFormat({
        identifier: { "base-format": "html", "target-format": "", "display-name": "" },
        metadata: { execute: { echo: false } },
      }),
    );
    // Explicit echo:false is preserved; the other defaults are still filled.
    expect(fmt.execute["echo"]).toBe(false);
    expect(fmt.execute["include"]).toBe(true);
  });

  it("is a pure fill — leaves a fully-specified execute block unchanged", () => {
    const full: Format["execute"] = {
      include: false, eval: false, output: false, echo: false, warning: false, error: true,
    };
    const fmt: Format = {
      identifier: {}, render: {}, execute: { ...full }, pandoc: {}, language: {}, metadata: {},
    };
    applyExecuteDefaults(fmt);
    expect(fmt.execute).toEqual(full);
  });
});
