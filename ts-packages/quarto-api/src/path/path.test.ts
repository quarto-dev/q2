/**
 * @quarto/api — path namespace tests (MIXED: pure + host-only)
 *
 * Seam-spec binding:
 *   - Pure ops: real value assertions (not "returns a string").
 *   - path.absolute: binding is the JOIN logic (cwd + relative), not the
 *     injected cwd value. Revert join → RED.
 *   - runtime/dataDir: assert ensureDir is called with the joined path.
 *   - resource: assert ensureDir is NOT called (read-only).
 *   - global-seam: assert runtime/dataDir/resource resolve from global config.
 *
 * Named reverts that turn each test RED are noted inline.
 * No Deno.* / node:* anywhere.
 */

import { describe, it, expect, vi } from "vitest";
import type { HostGlobalConfig } from "../platform/index.js";
import {
  toForwardSlashes,
  dirAndStem,
  isQmdFile,
  inputFilesDir,
  makePathHost,
} from "./index.js";

// ─── Shared fake builders ─────────────────────────────────────────────────────

/** Minimal HostGlobalConfig for tests that don't exercise pandoc. */
const fakeGlobal: HostGlobalConfig = {
  resourceDir: "/rs",
  runtimeDir: "/rt",
  dataDir: "/dt",
};

/** Build a fake host with a full fs shape (all methods) and a cwd spy. */
function makeFakePathHost(cwd = "/work") {
  const ensureDirSpy = vi.fn((_p: string): void => {});
  return {
    host: {
      cwd: () => cwd,
      fs: {
        readTextFileSync: vi.fn((_p: string) => ""),
        writeFileSync: vi.fn((_p: string, _c: string | Uint8Array) => {}),
        exists: vi.fn((_p: string) => false),
        ensureDir: ensureDirSpy,
        makeTempDir: vi.fn((_o?: { prefix?: string; dir?: string }) => "/tmp/d"),
        makeTempFile: vi.fn((_o?: { prefix?: string; suffix?: string; dir?: string }) => "/tmp/f"),
        remove: vi.fn((_p: string, _o?: { recursive?: boolean }) => {}),
        walk: vi.fn(
          (_r: string, _o?: { maxDepth?: number; includeDirs?: boolean }) =>
            [] as Array<{ path: string; isFile: boolean; isDirectory: boolean }>,
        ),
      },
    },
    spies: { ensureDir: ensureDirSpy },
  };
}

// ─── toForwardSlashes ─────────────────────────────────────────────────────────

describe("path.toForwardSlashes", () => {
  it("replaces backslashes with forward slashes", () => {
    // Revert replace to no-op → RED
    expect(toForwardSlashes("C:\\Users\\foo\\bar.txt")).toBe(
      "C:/Users/foo/bar.txt",
    );
  });

  it("leaves paths with only forward slashes unchanged", () => {
    expect(toForwardSlashes("/usr/local/bin")).toBe("/usr/local/bin");
  });

  it("handles mixed separators", () => {
    expect(toForwardSlashes("a/b\\c/d\\e")).toBe("a/b/c/d/e");
  });

  it("handles empty string", () => {
    expect(toForwardSlashes("")).toBe("");
  });
});

// ─── dirAndStem ───────────────────────────────────────────────────────────────

describe("path.dirAndStem", () => {
  it("returns [dir, stem] for a simple file path", () => {
    // Revert to returning [".", filename] → RED
    expect(dirAndStem("/home/user/report.qmd")).toEqual([
      "/home/user",
      "report",
    ]);
  });

  it("handles a Windows-style path", () => {
    expect(dirAndStem("C:\\Users\\foo\\doc.qmd")).toEqual([
      "C:\\Users\\foo",
      "doc",
    ]);
  });

  it("handles a filename with no directory component", () => {
    expect(dirAndStem("myfile.qmd")).toEqual([".", "myfile"]);
  });

  it("handles a filename with multiple dots (only last is extension)", () => {
    expect(dirAndStem("my.report.final.qmd")).toEqual([
      ".",
      "my.report.final",
    ]);
  });

  it("handles a file with no extension", () => {
    expect(dirAndStem("/a/b/Makefile")).toEqual(["/a/b", "Makefile"]);
  });
});

// ─── isQmdFile ────────────────────────────────────────────────────────────────

describe("path.isQmdFile", () => {
  it("returns true for a .qmd file", () => {
    // Revert to `return false` → RED
    expect(isQmdFile("report.qmd")).toBe(true);
  });

  it("returns true for .QMD (case-insensitive)", () => {
    expect(isQmdFile("REPORT.QMD")).toBe(true);
  });

  it("returns false for a .md file", () => {
    // Revert to `return true` → RED
    expect(isQmdFile("readme.md")).toBe(false);
  });

  it("returns false for a .rmd file", () => {
    expect(isQmdFile("analysis.rmd")).toBe(false);
  });

  it("returns false for a file with no extension", () => {
    expect(isQmdFile("Makefile")).toBe(false);
  });

  it("returns false for empty string", () => {
    expect(isQmdFile("")).toBe(false);
  });
});

// ─── inputFilesDir ────────────────────────────────────────────────────────────

describe("path.inputFilesDir", () => {
  it("returns stem + '_files' for a simple file", () => {
    // Revert to `return input + '_files'` (wrong — includes extension) → RED
    expect(inputFilesDir("report.qmd")).toBe("report_files");
  });

  it("works for a path with directory component", () => {
    expect(inputFilesDir("/docs/my-doc.qmd")).toBe("my-doc_files");
  });

  it("works for a file with multiple dots", () => {
    // stem of "a.b.qmd" is "a.b"
    expect(inputFilesDir("a.b.qmd")).toBe("a.b_files");
  });
});

// ─── path.absolute (host-only) ────────────────────────────────────────────────

describe("path.absolute", () => {
  const { host } = makeFakePathHost("/work");
  const pathHost = makePathHost(host, fakeGlobal);

  it("joins cwd with a relative path (the JOIN is the binding)", () => {
    // Revert join to return just cwd → RED (result would be "/work")
    expect(pathHost.absolute("a/b")).toBe("/work/a/b");
  });

  it("returns an already-absolute path unchanged", () => {
    // Revert to always prepending cwd → RED ("/work/already/abs")
    expect(pathHost.absolute("/already/absolute")).toBe("/already/absolute");
  });

  it("resolves dot-segments in a relative path", () => {
    expect(pathHost.absolute("a/../b")).toBe("/work/b");
  });

  it("resolves dot-segments in an absolute path", () => {
    expect(pathHost.absolute("/a/b/../c")).toBe("/a/c");
  });

  it("uses the host's cwd (not a hardcoded default)", () => {
    // Inject a different cwd to confirm the join uses the host value
    const { host: otherHost } = makeFakePathHost("/other");
    const otherPath = makePathHost(otherHost, fakeGlobal);
    expect(otherPath.absolute("x")).toBe("/other/x");
  });
});

// ─── path.runtime / path.dataDir — ensureDir dispatch ────────────────────────

describe("path.runtime / path.dataDir — ensureDir dispatch (Plan 2 Phase A)", () => {
  it("runtime(subdir) calls fs.ensureDir with the joined path (ensureDir-dispatch binding)", () => {
    const { host, spies } = makeFakePathHost();
    const pathHost = makePathHost(host, fakeGlobal);

    pathHost.runtime("julia");

    // Revert → RED: remove the ensureDir call from runtime
    expect(spies.ensureDir).toHaveBeenCalledWith("/rt/julia");
  });

  it("dataDir(subdir) calls fs.ensureDir with the joined path (ensureDir-dispatch binding)", () => {
    const { host, spies } = makeFakePathHost();
    const pathHost = makePathHost(host, fakeGlobal);

    pathHost.dataDir("x");

    // Revert → RED: remove the ensureDir call from dataDir
    expect(spies.ensureDir).toHaveBeenCalledWith("/dt/x");
  });

  it("resource(...parts) does NOT call fs.ensureDir (read-only — no dir creation)", () => {
    const { host, spies } = makeFakePathHost();
    const pathHost = makePathHost(host, fakeGlobal);

    pathHost.resource("a", "b");

    // Revert → RED: add ensureDir call to resource body
    expect(spies.ensureDir).not.toHaveBeenCalled();
  });
});

// ─── path.runtime — ensureDir error propagation ───────────────────────────────

describe("path.runtime — ensureDir error propagation (Plan 2 Phase A)", () => {
  it("propagates ensureDir errors — no try/catch (error-propagation binding)", () => {
    const err = new Error("permission denied");
    const { host, spies } = makeFakePathHost();
    spies.ensureDir.mockImplementation((_p: string): void => { throw err; });
    const pathHost = makePathHost(host, fakeGlobal);

    // Revert → RED: wrap the ensureDir call in try/catch so error is swallowed
    expect(() => pathHost.runtime("x")).toThrow("permission denied");
  });
});

// ─── path — global-seam (config-derived paths) ────────────────────────────────

describe("path — global-seam (config-derived paths) (Plan 2 Phase A)", () => {
  it("runtime/dataDir/resource resolve from global config values (global-seam binding)", () => {
    const { host } = makeFakePathHost();
    const pathHost = makePathHost(host, { runtimeDir: "/rt", dataDir: "/dt", resourceDir: "/rs" });

    // Revert → RED: don't read global (body reads undefined base → throws or wrong path)
    expect(pathHost.runtime("j")).toBe("/rt/j");
    expect(pathHost.dataDir("d")).toBe("/dt/d");
    expect(pathHost.resource("a")).toBe("/rs/a");
  });

  it("runtime() with no subdir returns the base runtimeDir (edge-case binding)", () => {
    const { host } = makeFakePathHost();
    const pathHost = makePathHost(host, fakeGlobal);

    // Revert → RED: make pathJoin require ≥2 parts (so no-subdir returns "" or throws) → RED
    expect(pathHost.runtime()).toBe("/rt");
  });

  it("resource with multiple parts joins them all (multi-part binding)", () => {
    const { host } = makeFakePathHost();
    const pathHost = makePathHost(host, fakeGlobal);

    // Revert → RED: join only the first part (drop the rest) → returns /rs/a not /rs/a/b/c → RED
    expect(pathHost.resource("a", "b", "c")).toBe("/rs/a/b/c");
  });
});
