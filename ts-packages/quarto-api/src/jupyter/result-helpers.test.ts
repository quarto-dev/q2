/**
 * @quarto/api/jupyter — result-helpers tests
 *
 * Mirrors Q1 semantics from:
 *   external-sources/quarto-cli/src/core/jupyter/jupyter.ts
 *   (`executeResultEngineDependencies` :2177-2185)
 *
 * No frozen Test Seam Spec row is assigned to this module (Plan 3 Task 11
 * brief). Both functions are bound with non-vacuous units + named reverts:
 *
 *   - `resultEngineDependencies`: array-wrap. Named revert: `return deps`
 *     (bare, no wrap) => `Array.isArray(result)` is false => RED.
 *   - `resultIncludes`: undefined => `{}` with NO host calls. Named revert:
 *     remove the `undefined => {}` guard (always call the builder) => either
 *     throws or writes temp files for undefined deps => RED.
 *   - `resultIncludes`: deps present => delegates to `widgetDependencyIncludes`
 *     (non-empty `PandocIncludes`, recorded host writes). Named revert: make
 *     it always return `{}` (ignore deps) => the deps-present case RED.
 */

import { describe, it, expect } from "vitest";
import type { JupyterWidgetDependencies } from "@quarto/types";
import type { PlatformHost } from "../platform/index.js";
import { resultIncludes, resultEngineDependencies } from "./result-helpers.js";

// ─── recording in-memory fs host (mirrors widgets.test.ts's fake) ──────────

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

// ─── resultEngineDependencies — pure array-wrap ────────────────────────────

describe("resultEngineDependencies", () => {
  it("array-wraps a present deps object (named revert: bare `return deps` => Array.isArray is false)", () => {
    const deps: JupyterWidgetDependencies = {
      jsWidgets: true,
      jupyterWidgets: false,
      htmlLibraries: [],
    };

    const result = resultEngineDependencies(deps);

    expect(Array.isArray(result)).toBe(true);
    expect(result!.length).toBe(1);
    expect(result![0]).toBe(deps);
  });

  it("returns undefined when there are no dependencies", () => {
    expect(resultEngineDependencies(undefined)).toBeUndefined();
  });
});

// ─── resultIncludes — host-dependent, delegates to widgetDependencyIncludes ─

describe("resultIncludes", () => {
  it("returns {} and makes no host calls when dependencies is undefined (named revert: remove the guard => RED)", () => {
    const { host, calls } = makeRecordingHost();

    const result = resultIncludes(host, "/tmp/quarto-jupyter", undefined);

    expect(result).toEqual({});
    expect(calls.makeTempFile.length).toBe(0);
    expect(calls.writeFileSync.length).toBe(0);
  });

  it("delegates to widgetDependencyIncludes when dependencies is present (named revert: always return {} => RED)", () => {
    const { host, calls } = makeRecordingHost();
    const deps: JupyterWidgetDependencies = {
      jsWidgets: true,
      jupyterWidgets: false,
      htmlLibraries: [],
    };

    const result = resultIncludes(host, "/tmp/quarto-jupyter", deps);

    expect(result["include-in-header"]).toBeDefined();
    expect(Array.isArray(result["include-in-header"])).toBe(true);
    expect(result["include-in-header"]!.length).toBe(1);
    expect(calls.makeTempFile.length).toBeGreaterThan(0);
    expect(calls.writeFileSync.length).toBeGreaterThan(0);
  });
});
