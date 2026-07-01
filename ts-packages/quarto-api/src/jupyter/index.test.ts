/**
 * @quarto/api/jupyter — index (makeJupyter factory) tests
 *
 * Frozen Test Seam Spec Row 19 (conformance): `makeJupyter`'s return is
 * annotated `QuartoAPI["jupyter"]` — that annotation IS the gate. Named
 * revert: delete any one of the 15 `NotImplemented` stub lines in
 * `index.ts` ⇒ `tsc --noEmit` fails with a missing-property error on the
 * object literal ⇒ RED. (Demonstrated in the Task 12 report — not
 * reproducible as a vitest assertion, since it is a *compile-time* failure;
 * the `ns: QuartoAPI["jupyter"] = makeJupyter(...)` assignment below is the
 * always-on, visible half of the gate that runs on every `tsc --noEmit`.)
 *
 * Runtime coverage: all 23 keys present, `notebookExtensions` is the real
 * `[".ipynb"]` value (not a function), and each of the 15 stubs throws.
 *
 * Smoke coverage (not a discriminator — catches gross integration breakage
 * only): a hand-built 2-code+1-markdown notebook, and a small `.ipynb`
 * fixture on disk, both run end-to-end through `toMarkdown`.
 */

import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type {
  ExecuteOptions,
  JupyterNotebook,
  JupyterNotebookAssetPaths,
  JupyterToMarkdownOptions,
} from "@quarto/types";
import type { QuartoAPI } from "@quarto/types";
import type { PlatformHost } from "../platform/index.js";

import { makeJupyter } from "./index.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

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

const fakeAssets: JupyterNotebookAssetPaths = {
  base_dir: "",
  files_dir: "notebook_files",
  figures_dir: "notebook_files/figure-html",
  supporting_dir: "notebook_files",
};

function makeToMarkdownOptions(
  over: Partial<JupyterToMarkdownOptions> = {},
): JupyterToMarkdownOptions {
  return {
    executeOptions: {} as unknown as ExecuteOptions,
    language: "python",
    assets: fakeAssets,
    execute: { echo: true, output: true, warning: true, include: true },
    toHtml: false,
    toLatex: false,
    toMarkdown: false,
    toIpynb: false,
    toPresentation: false,
    ...over,
  };
}

// ─── Row 19 — compile-time conformance gate ─────────────────────────────────

describe("Row 19 — makeJupyter(host) conforms to QuartoAPI[\"jupyter\"]", () => {
  it("assigns to the QuartoAPI[\"jupyter\"] type (the compile-time half of the gate)", () => {
    const { host } = makeRecordingHost();
    // This assignment IS the Row 19 gate: if `index.ts` is missing any of
    // the 23 members `QuartoAPI["jupyter"]` declares, `tsc --noEmit` fails
    // here with a missing-property error. Named revert: delete one
    // `NotImplemented` stub line in `index.ts` ⇒ RED (see Task 12 report).
    const ns: QuartoAPI["jupyter"] = makeJupyter(host);
    expect(ns).toBeDefined();
  });
});

// ─── Runtime shape ───────────────────────────────────────────────────────────

const EXPECTED_KEYS = [
  // 7 real
  "toMarkdown",
  "isPercentScript",
  "percentScriptToMarkdown",
  "assets",
  "resultIncludes",
  "widgetDependencyIncludes",
  "resultEngineDependencies",
  // 1 real value
  "notebookExtensions",
  // 15 stubs
  "isJupyterNotebook",
  "kernelspecFromMarkdown",
  "kernelspecForLanguage",
  "fromJSON",
  "markdownFromNotebookFile",
  "markdownFromNotebookJSON",
  "quartoMdToJupyter",
  "notebookFiltered",
  "pythonExec",
  "capabilities",
  "capabilitiesMessage",
  "capabilitiesJson",
  "installationMessage",
  "unactivatedEnvMessage",
  "pythonInstallationMessage",
] as const;

const STUB_KEYS = [
  "isJupyterNotebook",
  "kernelspecFromMarkdown",
  "kernelspecForLanguage",
  "fromJSON",
  "markdownFromNotebookFile",
  "markdownFromNotebookJSON",
  "quartoMdToJupyter",
  "notebookFiltered",
  "pythonExec",
  "capabilities",
  "capabilitiesMessage",
  "capabilitiesJson",
  "installationMessage",
  "unactivatedEnvMessage",
  "pythonInstallationMessage",
] as const;

describe("makeJupyter — runtime shape", () => {
  it("has exactly the 23 expected members", () => {
    const { host } = makeRecordingHost();
    const ns = makeJupyter(host);
    const keys = Object.keys(ns).sort();
    expect(keys).toEqual([...EXPECTED_KEYS].sort());
    expect(keys).toHaveLength(23);
  });

  it("notebookExtensions is the real [\".ipynb\"] value, not a function", () => {
    const { host } = makeRecordingHost();
    const ns = makeJupyter(host);
    expect(Array.isArray(ns.notebookExtensions)).toBe(true);
    expect(ns.notebookExtensions).toEqual([".ipynb"]);
  });

  it.each(STUB_KEYS)("%s throws (NotImplemented stub)", (key) => {
    const { host } = makeRecordingHost();
    const ns = makeJupyter(host) as unknown as Record<string, (...args: unknown[]) => unknown>;
    expect(() => ns[key]()).toThrow(
      new RegExp(`quarto\\.jupyter\\.${key} is not implemented`),
    );
  });
});

// ─── Smoke tests ─────────────────────────────────────────────────────────────

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

describe("smoke — toMarkdown end-to-end via makeJupyter", () => {
  it("converts a simple 2-code+1-markdown notebook to markdown cellOutputs", async () => {
    const { host } = makeRecordingHost();
    const ns = makeJupyter(host);

    const nb = pythonKernelNotebook([
      {
        cell_type: "markdown",
        metadata: {},
        source: ["## Smoke heading\n", "\n", "Some smoke-test prose."],
      },
      {
        cell_type: "code",
        metadata: {},
        source: ["print('smoke one')"],
        execution_count: 1,
        outputs: [
          { output_type: "stream", name: "stdout", text: ["smoke one\n"] },
        ],
      },
      {
        cell_type: "code",
        metadata: {},
        source: ["1 + 1"],
        execution_count: 2,
        outputs: [
          {
            output_type: "execute_result",
            execution_count: 2,
            data: { "text/plain": ["2"] },
            metadata: {},
          },
        ],
      },
    ]);

    const result = await ns.toMarkdown(nb, makeToMarkdownOptions());

    expect(Array.isArray(result.cellOutputs)).toBe(true);
    expect(result.cellOutputs.length).toBeGreaterThan(0);
    for (const cellOutput of result.cellOutputs) {
      expect(typeof cellOutput.markdown).toBe("string");
    }

    const joined = result.cellOutputs.map((o) => o.markdown).join("");
    expect(joined).toContain("Smoke heading");
    expect(joined).toContain("smoke-test prose");
    expect(joined).toContain("print('smoke one')");
    expect(joined).toContain("1 + 1");
  });

  it("converts the __fixtures__/simple.ipynb fixture end-to-end", async () => {
    const { host } = makeRecordingHost();
    const ns = makeJupyter(host);

    const fixtureText = readFileSync(
      join(__dirname, "__fixtures__", "simple.ipynb"),
      "utf-8",
    );
    const nb = JSON.parse(fixtureText) as JupyterNotebook;

    const result = await ns.toMarkdown(nb, makeToMarkdownOptions());

    expect(result.cellOutputs.length).toBeGreaterThan(0);
    const joined = result.cellOutputs.map((o) => o.markdown).join("");
    expect(joined).toContain("Fixture Title");
    expect(joined).toContain("hello from fixture");
    expect(joined).toContain("print('hello from fixture')");
  });

  it("also runs with toIpynb:true and asserts notebookOutputs shape when present", async () => {
    const { host } = makeRecordingHost();
    const ns = makeJupyter(host);

    const fixtureText = readFileSync(
      join(__dirname, "__fixtures__", "simple.ipynb"),
      "utf-8",
    );
    const nb = JSON.parse(fixtureText) as JupyterNotebook;

    const result = await ns.toMarkdown(
      nb,
      makeToMarkdownOptions({ toIpynb: true }),
    );

    expect(result.cellOutputs.length).toBeGreaterThan(0);
    if (result.notebookOutputs) {
      expect(
        result.notebookOutputs.prefix === undefined ||
          typeof result.notebookOutputs.prefix === "string",
      ).toBe(true);
      expect(
        result.notebookOutputs.suffix === undefined ||
          typeof result.notebookOutputs.suffix === "string",
      ).toBe(true);
    }
  });
});
