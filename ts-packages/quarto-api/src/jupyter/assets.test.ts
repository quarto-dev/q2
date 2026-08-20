/**
 * @quarto/api/jupyter — assets tests
 *
 * Frozen Test Seam Spec row covered here: Row 18 (`assets` — snake_case
 * 4-field `JupyterNotebookAssetPaths` + recorded `ensureDir`/`walk` calls).
 * Named revert (see Task 10 report for the RED transcript): remove the
 * `host.fs.ensureDir(figures_dir)` call in `assets.ts` ⇒ the recording host
 * has no `ensureDir` entry ⇒ RED.
 *
 * A recording in-memory `host.fs` records every `ensureDir` call; `walk` is
 * configurable per test (default: no entries, so `supporting_dir ===
 * files_dir`; tests that need the promotion branch configure `walk` to
 * return an "other subdir" entry).
 */

import { describe, it, expect } from "vitest";
import type { PlatformHost } from "../platform/index.js";

import { assets } from "./assets.js";

// ─── recording in-memory fs host ────────────────────────────────────────────

type WalkEntry = { path: string; isFile: boolean; isDirectory: boolean };

interface RecordingHost {
  host: Pick<PlatformHost, "fs">;
  ensureDirCalls: string[];
  walkCalls: Array<{ root: string; opts?: { maxDepth?: number; includeDirs?: boolean } }>;
}

function makeRecordingHost(walkResult: WalkEntry[] = []): RecordingHost {
  const ensureDirCalls: string[] = [];
  const walkCalls: Array<{ root: string; opts?: { maxDepth?: number; includeDirs?: boolean } }> =
    [];
  const host: Pick<PlatformHost, "fs"> = {
    fs: {
      readTextFileSync: () => {
        throw new Error("readTextFileSync: not implemented in this fake");
      },
      writeFileSync: () => {},
      exists: () => false,
      ensureDir: (path) => {
        ensureDirCalls.push(path);
      },
      makeTempDir: () => "/tmp/fake",
      makeTempFile: () => "/tmp/fake-file",
      remove: () => {},
      walk: (root, opts) => {
        walkCalls.push({ root, opts });
        return walkResult;
      },
    },
  };
  return { host, ensureDirCalls, walkCalls };
}

// ─── Row 18 ─────────────────────────────────────────────────────────────────

describe("assets", () => {
  it("Row 18: returns the snake_case 4-field JupyterNotebookAssetPaths, and records an ensureDir + walk call", () => {
    const { host, ensureDirCalls, walkCalls } = makeRecordingHost();

    const result = assets(host, "/proj/notebook.qmd", "html");

    // snake_case 4-field shape
    expect(Object.keys(result).sort()).toEqual(
      ["base_dir", "figures_dir", "files_dir", "supporting_dir"].sort(),
    );
    expect(result).toHaveProperty("base_dir");
    expect(result).toHaveProperty("files_dir");
    expect(result).toHaveProperty("figures_dir");
    expect(result).toHaveProperty("supporting_dir");

    // recorded ensureDir(figures_dir) call, on the ABSOLUTE path
    expect(ensureDirCalls).toContain("/proj/notebook_files/figure-html");

    // recorded walk(...) call (supporting-dir promotion check)
    expect(walkCalls.length).toBeGreaterThan(0);
  });

  it("figures_dir value: relative, forward-slashed, joins files_dir + figure-<to>", () => {
    const { host } = makeRecordingHost();
    const result = assets(host, "/proj/notebook.qmd", "html");
    expect(result.figures_dir).toBe("notebook_files/figure-html");
    expect(result.files_dir).toBe("notebook_files");
    expect(result.base_dir).toBe("/proj");
  });

  it("`to` undefined defaults to figure-html", () => {
    const { host } = makeRecordingHost();
    const result = assets(host, "/proj/notebook.qmd", undefined);
    expect(result.figures_dir).toBe("notebook_files/figure-html");
  });

  it("`to` with a `+` suffix normalizes to figure-html", () => {
    const { host } = makeRecordingHost();
    const result = assets(host, "/proj/notebook.qmd", "html+something");
    expect(result.figures_dir).toBe("notebook_files/figure-html");
  });

  it("`to` of html4 normalizes to figure-html", () => {
    const { host } = makeRecordingHost();
    const result = assets(host, "/proj/notebook.qmd", "html4");
    expect(result.figures_dir).toBe("notebook_files/figure-html");
  });

  it("`to` with a `-` suffix normalizes (e.g. a hypothetical `revealjs-foo`)", () => {
    const { host } = makeRecordingHost();
    const result = assets(host, "/proj/notebook.qmd", "revealjs-foo");
    expect(result.figures_dir).toBe("notebook_files/figure-revealjs");
  });

  it("supporting-dir promotion: no other subdirs under files_dir => supporting_dir === files_dir", () => {
    const { host } = makeRecordingHost([]);
    const result = assets(host, "/proj/notebook.qmd", "html");
    expect(result.supporting_dir).toBe(result.files_dir);
    expect(result.supporting_dir).toBe("notebook_files");
  });

  it("supporting-dir promotion: an other subdir under files_dir => supporting_dir === figures_dir", () => {
    const { host } = makeRecordingHost([
      { path: "/proj/notebook_files", isFile: false, isDirectory: true },
      { path: "/proj/notebook_files/figure-html", isFile: false, isDirectory: true },
      { path: "/proj/notebook_files/other-subdir", isFile: false, isDirectory: true },
    ]);
    const result = assets(host, "/proj/notebook.qmd", "html");
    expect(result.supporting_dir).toBe(result.figures_dir);
    expect(result.supporting_dir).toBe("notebook_files/figure-html");
  });
});
