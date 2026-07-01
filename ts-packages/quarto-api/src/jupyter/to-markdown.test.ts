/**
 * @quarto/api/jupyter — to-markdown tests
 *
 * Frozen Test Seam Spec rows covered here: 3, 9, 10, 12, 13, 14, 15.
 * Each `it` is written to a frozen assertion; its named revert (see the Task 8
 * report for the RED transcripts) must redden exactly that assertion.
 *
 * A recording in-memory `host.fs` captures figure writes; every other fs
 * method is a no-op (to-markdown only ever writes figures).
 */

import { describe, it, expect } from "vitest";
import type {
  ExecuteOptions,
  JupyterNotebook,
  JupyterNotebookAssetPaths,
  JupyterToMarkdownOptions,
} from "@quarto/types";
import type { PlatformHost } from "../platform/index.js";

import { jupyterToMarkdown, mdFormatOutput, mdRawOutput } from "./to-markdown.js";
import { isPreservedHtml } from "./preserve.js";

// ─── recording in-memory fs host ────────────────────────────────────────────

interface RecordingHost {
  host: Pick<PlatformHost, "fs">;
  writes: Array<{ path: string; content: string | Uint8Array }>;
}

function makeRecordingHost(): RecordingHost {
  const writes: Array<{ path: string; content: string | Uint8Array }> = [];
  const host: Pick<PlatformHost, "fs"> = {
    fs: {
      readTextFileSync: () => {
        throw new Error("readTextFileSync: not implemented in this fake");
      },
      writeFileSync: (path, content) => {
        writes.push({ path, content });
      },
      exists: () => false,
      ensureDir: () => {},
      makeTempDir: () => "/tmp/fake",
      makeTempFile: () => "/tmp/fake-file",
      remove: () => {},
      walk: () => [],
    },
  };
  return { host, writes };
}

// ─── fixtures ───────────────────────────────────────────────────────────────

const assets: JupyterNotebookAssetPaths = {
  base_dir: "",
  files_dir: "notebook_files",
  figures_dir: "notebook_files/figure-html",
  supporting_dir: "notebook_files",
};

function makeOptions(over: Partial<JupyterToMarkdownOptions>): JupyterToMarkdownOptions {
  return {
    executeOptions: {} as unknown as ExecuteOptions,
    language: "python",
    assets,
    execute: { echo: true, output: true, warning: true, include: true },
    toHtml: false,
    toLatex: false,
    toMarkdown: false,
    toIpynb: false,
    toPresentation: false,
    ...over,
  };
}

function pythonKernelNotebook(cells: JupyterNotebook["cells"]): JupyterNotebook {
  return {
    cells,
    metadata: {
      kernelspec: { name: "python3", language: "python", display_name: "Python 3" },
    },
    nbformat: 4,
    nbformat_minor: 5,
  };
}

// ─── Row 3 — non-math text/latex ⇒ ```{=tex} raw block ──────────────────────

describe("Row 3 — non-math text/latex emits a {=tex} raw block", () => {
  it("emits ```{=tex} for a non-math latex display_data output", async () => {
    const nb = pythonKernelNotebook([
      {
        cell_type: "code",
        metadata: {},
        source: ["x = 1\n"],
        outputs: [
          {
            output_type: "display_data",
            data: { "text/latex": ["\\newcommand{\\foo}{bar}"] },
            metadata: {},
          },
        ],
      },
    ]);
    const { host } = makeRecordingHost();
    const result = await jupyterToMarkdown(host, nb, makeOptions({ toLatex: true }));

    const md = result.cellOutputs[0].markdown;
    // baseline: non-math latex ⇒ {=tex} raw block, NOT a math/markdown render
    expect(md).toContain("{=tex}");
    expect(md).toContain("\\newcommand{\\foo}{bar}");
  });
});

// ─── Row 9 — text/html passes through literally; preserve is inert ──────────

describe("Row 9 — text/html output passes through literally", () => {
  it("emits the literal <table> and leaves htmlPreserve empty", async () => {
    const nb = pythonKernelNotebook([
      {
        cell_type: "code",
        metadata: {},
        source: ["df\n"],
        outputs: [
          {
            output_type: "display_data",
            data: { "text/html": ["<table><tr><td>x</td></tr></table>"] },
            metadata: {},
          },
        ],
      },
    ]);
    const { host } = makeRecordingHost();
    const result = await jupyterToMarkdown(host, nb, makeOptions({ toHtml: true }));

    expect(result.cellOutputs[0].markdown).toContain("<table>");
    expect(result.htmlPreserve).toBeUndefined();
    expect(isPreservedHtml("<table><tr><td>x</td></tr></table>")).toBe(false);
  });
});

// ─── Row 10 — hoisted plotly html-library <script> stripped from cell body ──

const PLOTLY_LIB = '<script type="text/javascript">require.undef("plotly");</script>';

describe("Row 10 — hoisted html-library script is stripped from the cell walk", () => {
  it("removes the plotly lib from the cell output and captures it as a dependency", async () => {
    const nb = pythonKernelNotebook([
      {
        cell_type: "code",
        metadata: {},
        source: ["fig.show()\n"],
        outputs: [
          {
            output_type: "display_data",
            data: { "text/html": [PLOTLY_LIB] },
            metadata: {},
          },
        ],
      },
    ]);
    const { host } = makeRecordingHost();
    const result = await jupyterToMarkdown(host, nb, makeOptions({ toHtml: true }));

    const combined = result.cellOutputs.map((o) => o.markdown).join("");
    // the hoisted lib must NOT appear in the cell body (it was stripped)
    expect(combined).not.toContain("require.undef");
    // path-exercised: the pre-walk strip mutated cell.outputs in place …
    expect(nb.cells[0].outputs).toHaveLength(0);
    // … and captured the lib into the widget dependencies
    expect(result.dependencies?.htmlLibraries).toHaveLength(1);
    expect(result.dependencies?.htmlLibraries[0]).toContain("require.undef");
  });
});

// ─── Row 12 — dependencies straddle the isHtml gate ─────────────────────────

function widgetNotebook(): JupyterNotebook {
  return pythonKernelNotebook([
    {
      cell_type: "code",
      metadata: {},
      source: ["w\n"],
      outputs: [
        {
          output_type: "display_data",
          data: {
            "application/vnd.jupyter.widget-view+json": {
              version_major: 2,
              version_minor: 0,
              model_id: "abc",
            },
          },
          metadata: {},
        },
      ],
    },
  ]);
}

describe("Row 12 — widget dependencies are gated on isHtml", () => {
  it("defines dependencies for {toHtml:true, toIpynb:false}", async () => {
    const { host } = makeRecordingHost();
    const result = await jupyterToMarkdown(
      host,
      widgetNotebook(),
      makeOptions({ toHtml: true, toIpynb: false }),
    );
    expect(result.dependencies).toBeDefined();
  });

  it("leaves dependencies undefined for {toIpynb:true}", async () => {
    const { host } = makeRecordingHost();
    const result = await jupyterToMarkdown(
      host,
      widgetNotebook(),
      makeOptions({ toHtml: true, toIpynb: true }),
    );
    expect(result.dependencies).toBeUndefined();
  });
});

// ─── Row 13 — image/png base64 ⇒ decoded bytes written to figures_dir ───────

describe("Row 13 — image/png output is base64-decoded and written to figures_dir", () => {
  it("records a writeFileSync of the decoded bytes and emits an image ref", async () => {
    // base64 of "PNGDATA"
    const b64 = "UE5HREFUQQ==";
    const nb = pythonKernelNotebook([
      {
        cell_type: "code",
        metadata: {},
        source: ["plot()\n"],
        outputs: [
          {
            output_type: "display_data",
            data: { "image/png": [b64] },
            metadata: {},
          },
        ],
      },
    ]);
    const { host, writes } = makeRecordingHost();
    const result = await jupyterToMarkdown(host, nb, makeOptions({ toHtml: false }));

    // ≥1 recorded write whose path starts with the figures dir and ends .png
    const pngWrites = writes.filter(
      (w) => w.path.startsWith(assets.figures_dir) && w.path.endsWith(".png"),
    );
    expect(pngWrites.length).toBeGreaterThanOrEqual(1);
    const content = pngWrites[0].content;
    expect(content).toBeInstanceOf(Uint8Array);
    expect(new TextDecoder().decode(content as Uint8Array)).toBe("PNGDATA");

    // markdown carries an image reference to the written png
    expect(result.cellOutputs[0].markdown).toContain("![](");
    expect(result.cellOutputs[0].markdown).toContain(".png)");
  });
});

// ─── Row 14 — error traceback ANSI is stripped ──────────────────────────────

describe("Row 14 — error output has its ANSI escape codes stripped", () => {
  it("emits the traceback text with the ANSI escape bytes absent", async () => {
    const esc = "\u001b[31m";
    const reset = "\u001b[0m";
    const nb = pythonKernelNotebook([
      {
        cell_type: "code",
        metadata: {},
        source: ["boom()\n"],
        outputs: [
          {
            output_type: "error",
            ename: "ValueError",
            evalue: "bad",
            traceback: [`${esc}Traceback${reset} line1`, "frame2"],
          },
        ],
      },
    ]);
    const { host } = makeRecordingHost();
    const result = await jupyterToMarkdown(host, nb, makeOptions({ toHtml: false }));

    const md = result.cellOutputs[0].markdown;
    expect(md).toContain("ValueError: bad");
    expect(md).toContain("Traceback");
    expect(md).toContain("line1");
    // ANSI escape bytes must be gone
    expect(md).not.toContain("\u001b");
  });
});

// ─── Row 15 — Julia-consumer cellOutput shape ───────────────────────────────

describe("Row 15 — cellOutputs carry {id, markdown} objects", () => {
  it("cellOutputs[0].markdown is a string and id is non-empty", async () => {
    const nb = pythonKernelNotebook([
      { cell_type: "markdown", metadata: {}, source: ["# Hello\n"] },
      {
        cell_type: "code",
        metadata: {},
        source: ["1 + 1\n"],
        outputs: [
          {
            output_type: "execute_result",
            data: { "text/plain": ["2"] },
            metadata: {},
            execution_count: 1,
          },
        ],
      },
    ]);
    const { host } = makeRecordingHost();
    const result = await jupyterToMarkdown(host, nb, makeOptions({ toHtml: true }));

    expect(typeof result.cellOutputs[0].markdown).toBe("string");
    expect(result.cellOutputs[0].id.length).toBeGreaterThan(0);
    expect(typeof result.cellOutputs[1].markdown).toBe("string");
    expect(result.cellOutputs[1].id.length).toBeGreaterThan(0);
    // Q1 parity: pandoc is never populated
    expect(result.pandoc).toBeUndefined();
  });
});

// ─── generic helpers exported for Task 9 ────────────────────────────────────

describe("mdFormatOutput / mdRawOutput exports", () => {
  it("mdFormatOutput wraps source in a {=fmt} fence", () => {
    expect(mdFormatOutput("tex", ["\\alpha"])).toContain("{=tex}");
  });
  it("mdRawOutput dispatches text/html to an {=html} block", () => {
    expect(mdRawOutput("text/html", ["<b>x</b>"])).toContain("{=html}");
  });
});
