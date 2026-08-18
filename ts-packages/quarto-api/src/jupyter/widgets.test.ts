/**
 * @quarto/api/jupyter — widgets tests
 *
 * Mirrors Q1 semantics from:
 *   external-sources/quarto-cli/src/core/jupyter/widgets.ts
 *
 * Frozen Test Seam Spec Row 11 covers `widgetDependencyIncludes`: (a) the
 * host's `fs.makeTempFile`/`fs.writeFileSync` were called, (b) the return
 * has a kebab, ARRAY-valued `"include-in-header"` key, (c) the return is
 * assignable to `DependenciesResult["includes"]` (both are `PandocIncludes`).
 *
 * Named reverts (both must redden this suite — see report for the RED
 * transcript):
 *   (i) emit camelCase scalar `{ inHeader: <path> }` instead of the kebab
 *       array `{ "include-in-header": [<path>] }` => assertion (b) fails
 *       (`"include-in-header"` absent).
 *   (ii) replace the whole `widgetDependencyIncludes` body with
 *       `throw new Error("NotImplemented")` => the call itself throws,
 *       failing every `it` block below that invokes it.
 *
 * A second unit binds the Task 8 Row 10 in-place strip (P3-17):
 * `widgetDependencies` must mutate `cell.outputs` in place, removing the
 * hoisted HTML-library `<script>` it captured into `htmlLibraries`. Named
 * revert: stop reassigning `cell.outputs` (scan without stripping) =>
 * `cell.outputs` still contains the original `<script>` => RED.
 */

import { describe, it, expect } from "vitest";
import type {
  DependenciesResult,
  JupyterNotebook,
  JupyterOutputDisplayData,
  JupyterWidgetDependencies,
  PandocIncludes,
} from "@quarto/types";
import type { PlatformHost } from "../platform/index.js";
import { widgetDependencies, widgetDependencyIncludes } from "./widgets.js";
import { kTextHtml } from "./constants.js";

// ─── recording in-memory fs host (local to this test file) ────────────────

function makeRecordingHost(): {
  host: Pick<PlatformHost, "fs">;
  calls: {
    makeTempFile: Array<{ dir?: string; prefix?: string; suffix?: string } | undefined>;
    writeFileSync: Array<{ path: string; content: string }>;
  };
} {
  const calls: {
    makeTempFile: Array<{ dir?: string; prefix?: string; suffix?: string } | undefined>;
    writeFileSync: Array<{ path: string; content: string }>;
  } = {
    makeTempFile: [],
    writeFileSync: [],
  };
  let counter = 0;

  const host: Pick<PlatformHost, "fs"> = {
    fs: {
      readTextFileSync: () => {
        throw new Error("readTextFileSync: not implemented in this fake");
      },
      writeFileSync: (path: string, content: string | Uint8Array) => {
        calls.writeFileSync.push({ path, content: content as string });
      },
      exists: () => false,
      ensureDir: () => {},
      makeTempDir: () => {
        throw new Error("makeTempDir: not implemented in this fake");
      },
      makeTempFile: (opts) => {
        calls.makeTempFile.push(opts);
        counter += 1;
        return `${opts?.dir ?? "/tmp"}/${opts?.prefix ?? ""}${counter}${opts?.suffix ?? ""}`;
      },
      remove: () => {},
      walk: () => [],
    },
  };

  return { host, calls };
}

// ─── Row 11 — widgetDependencyIncludes ─────────────────────────────────────

describe("widgetDependencyIncludes — Row 11", () => {
  it("calls host.fs.makeTempFile and host.fs.writeFileSync, and returns a kebab/array PandocIncludes", () => {
    const { host, calls } = makeRecordingHost();
    const deps: JupyterWidgetDependencies = {
      jsWidgets: true,
      jupyterWidgets: false,
      htmlLibraries: [],
    };

    const result = widgetDependencyIncludes(host, deps, "/tmp/quarto-jupyter");

    // (a) the host's fs methods were actually invoked
    expect(calls.makeTempFile.length).toBeGreaterThan(0);
    expect(calls.writeFileSync.length).toBeGreaterThan(0);

    // (b) kebab key present, ARRAY value (not a camelCase scalar)
    expect(result["include-in-header"]).toBeDefined();
    expect(Array.isArray(result["include-in-header"])).toBe(true);
    expect(result["include-in-header"]!.length).toBe(1);
    expect(typeof result["include-in-header"]![0]).toBe("string");

    // no camelCase leakage
    expect((result as Record<string, unknown>)["inHeader"]).toBeUndefined();

    // the written fragment ended up on the recorded host
    const written = calls.writeFileSync.find(
      (call) => call.path === result["include-in-header"]![0],
    );
    expect(written).toBeDefined();
    expect(written!.content).toContain("<script");
  });

  it("omits include-after-body when there is no widget state to write", () => {
    const { host } = makeRecordingHost();
    const deps: JupyterWidgetDependencies = {
      jsWidgets: false,
      jupyterWidgets: false,
      htmlLibraries: [],
    };

    const result = widgetDependencyIncludes(host, deps, "/tmp/quarto-jupyter");

    expect(result["include-in-header"]).toBeUndefined();
    expect(result["include-after-body"]).toBeUndefined();
  });

  it("writes include-after-body as a kebab array when jupyter widget state is present", () => {
    const { host, calls } = makeRecordingHost();
    const deps: JupyterWidgetDependencies = {
      jsWidgets: false,
      jupyterWidgets: true,
      htmlLibraries: [],
      widgetsState: {
        state: { "1": { model_name: "IntSliderModel" } },
        version_major: 2,
        version_minor: 0,
      },
    };

    const result = widgetDependencyIncludes(host, deps, "/tmp/quarto-jupyter");

    expect(Array.isArray(result["include-after-body"])).toBe(true);
    expect(result["include-after-body"]!.length).toBe(1);
    const written = calls.writeFileSync.find(
      (call) => call.path === result["include-after-body"]![0],
    );
    expect(written).toBeDefined();
    expect(written!.content).toContain("application/vnd.jupyter.widget-state+json");
  });

  it("(c) is assignable to DependenciesResult['includes'] (PandocIncludes)", () => {
    const { host } = makeRecordingHost();
    const deps: JupyterWidgetDependencies = {
      jsWidgets: true,
      jupyterWidgets: false,
      htmlLibraries: [],
    };

    const result = widgetDependencyIncludes(host, deps, "/tmp/quarto-jupyter");

    // Compile-time assignability checks — these lines fail to typecheck
    // (not just fail at runtime) if the return shape drifts from the
    // vendored PandocIncludes.
    const asPandocIncludes: PandocIncludes = result;
    const asDependenciesResultIncludes: DependenciesResult["includes"] = result;

    expect(asPandocIncludes).toBe(result);
    expect(asDependenciesResultIncludes).toBe(result);
  });
});

// ─── widgetDependencies — in-place strip (binds Task 8 Row 10 / P3-17) ─────

function makeNotebookWithHoistedLibrary(): JupyterNotebook {
  const plotlyScript =
    "<script type=\"text/javascript\">require.undef('plotly');define('plotly', function() { return window.Plotly; });</script>";
  const output: JupyterOutputDisplayData = {
    output_type: "display_data",
    data: {
      [kTextHtml]: [plotlyScript],
    },
    metadata: {},
  };
  return {
    cells: [
      {
        cell_type: "code",
        metadata: {},
        source: ["import plotly.express as px"],
        outputs: [output],
      },
    ],
    metadata: {
      kernelspec: { name: "python3", language: "python", display_name: "Python 3" },
    },
    nbformat: 4,
    nbformat_minor: 5,
  };
}

describe("widgetDependencies — in-place strip", () => {
  it("captures the hoisted html library into htmlLibraries", () => {
    const nb = makeNotebookWithHoistedLibrary();
    const deps = widgetDependencies(nb);

    expect(deps).toBeDefined();
    expect(deps!.htmlLibraries.length).toBe(1);
    expect(deps!.htmlLibraries[0]).toContain("plotly");
  });

  it("mutates cell.outputs in place — the hoisted <script> is gone from the output bundle", () => {
    const nb = makeNotebookWithHoistedLibrary();
    widgetDependencies(nb);

    // The cell had exactly one output, and it was the hoisted library —
    // after the strip, the outputs array must no longer contain it.
    expect(nb.cells[0].outputs).toBeDefined();
    expect(nb.cells[0].outputs!.length).toBe(0);
  });

  it("returns undefined when the notebook has no widget signals at all", () => {
    const nb: JupyterNotebook = {
      cells: [
        {
          cell_type: "code",
          metadata: {},
          source: ["1 + 1"],
          outputs: [
            {
              output_type: "execute_result",
              data: { "text/plain": ["2"] },
              metadata: {},
            } as JupyterOutputDisplayData,
          ],
        },
      ],
      metadata: {
        kernelspec: { name: "python3", language: "python", display_name: "Python 3" },
      },
      nbformat: 4,
      nbformat_minor: 5,
    };

    expect(widgetDependencies(nb)).toBeUndefined();
  });
});
