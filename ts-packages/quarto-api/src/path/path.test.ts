/**
 * @quarto/api — path namespace tests (MIXED: pure + host-only)
 *
 * Seam-spec binding:
 *   - Pure ops: real value assertions (not "returns a string").
 *   - path.absolute: binding is the JOIN logic (cwd + relative), not the
 *     injected cwd value. Revert join → RED.
 *   - Stubs (runtime/resource/dataDir): assert they THROW with the exact
 *     "requires launch context" substring.
 *
 * Named reverts that reden each test are noted inline.
 * No Deno.* / node:* anywhere.
 */

import { describe, it, expect } from "vitest";
import {
  toForwardSlashes,
  dirAndStem,
  isQmdFile,
  inputFilesDir,
  makePathHost,
} from "./index.js";

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
  const host = { cwd: () => "/work" };
  const pathHost = makePathHost(host);

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
    const otherHost = { cwd: () => "/other" };
    const otherPath = makePathHost(otherHost);
    expect(otherPath.absolute("x")).toBe("/other/x");
  });
});

// ─── path stubs (runtime / resource / dataDir) ────────────────────────────────

describe("path stubs — requires launch context", () => {
  const host = { cwd: () => "/work" };
  const pathHost = makePathHost(host);

  it("runtime() throws with 'requires launch context'", () => {
    // Revert to returning a string → RED
    expect(() => pathHost.runtime()).toThrow(/requires launch context/);
  });

  it("resource() throws with 'requires launch context'", () => {
    expect(() => pathHost.resource()).toThrow(/requires launch context/);
  });

  it("dataDir() throws with 'requires launch context'", () => {
    expect(() => pathHost.dataDir()).toThrow(/requires launch context/);
  });

  it("runtime() error message names the method", () => {
    // Revert to a generic message → RED
    expect(() => pathHost.runtime()).toThrow(/path\.runtime/);
  });

  it("resource() error message names the method", () => {
    expect(() => pathHost.resource()).toThrow(/path\.resource/);
  });

  it("dataDir() error message names the method", () => {
    expect(() => pathHost.dataDir()).toThrow(/path\.dataDir/);
  });
});
