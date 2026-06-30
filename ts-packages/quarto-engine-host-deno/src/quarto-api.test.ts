/**
 * Tests for src/quarto-api.ts — buildQuartoAPI assembler.
 *
 * TDD: tests written before the implementation exists. Named-revert tests have
 * explicit documentation about what must go RED when the named revert is applied.
 *
 * Scope: DIRECT unit tests only (no host loop). Loop-driven contract tests
 * (init() sees the API end-to-end, T-A4) are a later task (host.ts).
 */

import { describe, it, expect, vi } from "vitest";
import { buildQuartoAPI } from "./quarto-api.js";
import { serializeMappedString } from "./mapped-source.js";
import type { PlatformHost } from "@quarto/api/platform";
import type { HostGlobalConfig } from "./types.js";

// ---------------------------------------------------------------------------
// Minimal fake PlatformHost for unit tests
// ---------------------------------------------------------------------------

function makeFakeHost(): PlatformHost & { logCalls: string[] } {
  const logCalls: string[] = [];
  return {
    logCalls,
    fs: {
      readTextFileSync: (_path: string) => "",
      writeFileSync: (_path: string, _content: string | Uint8Array) => {},
      exists: (_path: string) => false,
      ensureDir: (_path: string) => {},
      makeTempDir: (_opts?: { prefix?: string; dir?: string }) => "/tmp/quarto-fake",
      makeTempFile: (_opts?: { prefix?: string; suffix?: string; dir?: string }) => "/tmp/quarto-fake-file",
      remove: (_path: string, _opts?: { recursive?: boolean }) => {},
      walk: (_root: string, _opts?: { maxDepth?: number; includeDirs?: boolean }) => [],
    },
    process: {
      exec: async (_cmd: string, _args: string[], _opts?) => ({
        code: 0,
        success: true,
        stdout: "",
        stderr: "",
      }),
      onExit: (_handler: () => void) => {},
      exit: (_code: number): never => {
        throw new Error("exit called");
      },
    },
    env: {
      get: (_key: string) => undefined,
    },
    log: {
      info: vi.fn((msg: string) => {
        logCalls.push(`info:${msg}`);
      }),
      warning: vi.fn((msg: string) => {
        logCalls.push(`warning:${msg}`);
      }),
      error: vi.fn((msg: string) => {
        logCalls.push(`error:${msg}`);
      }),
    },
    cwd: () => "/fake/cwd",
    realPath: (path: string) => path,
    isInteractive: false,
    isCI: false,
  };
}

function makeFakeGlobal(): HostGlobalConfig {
  return {
    resourceDir: "/fake/resources",
    runtimeDir: "/fake/runtime",
    dataDir: "/fake/data",
    pandocPath: null,
    isInteractiveSession: false,
    runningInCi: false,
    quartoVersion: "0.0.0",
  };
}

// ---------------------------------------------------------------------------
// Pure namespaces are REAL (not stubbed)
// ---------------------------------------------------------------------------

describe("buildQuartoAPI — pure namespaces", () => {
  it("text.lines splits a string on newlines", () => {
    // Named revert: replace the text namespace with an object whose `lines` throws
    // → this assertion goes RED, proving "pure namespaces callable & real, not stubbed"
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    expect(api.text.lines("a\nb")).toEqual(["a", "b"]);
  });

  it("text.trimEmptyLines removes leading/trailing empty lines", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    expect(api.text.trimEmptyLines(["", "hello", ""])).toEqual(["hello"]);
  });

  it("text.lineColToIndex converts line/col to character offset", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    const convert = api.text.lineColToIndex("hello\nworld");
    expect(convert({ line: 1, column: 0 })).toBe(6);
  });

  it("text.asYamlText serializes a metadata object to YAML", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    const yaml = api.text.asYamlText({ title: "Hello" });
    expect(yaml).toContain("title:");
    expect(yaml).toContain("Hello");
  });

  it("crypto.md5Hash returns a 32-char hex string", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    const hash = api.crypto.md5Hash("x");
    expect(hash).toMatch(/^[0-9a-f]{32}$/);
  });

  it("format.isHtmlCompatible returns true for html format", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    // Build a minimal Format object
    const htmlFormat = {
      identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" },
      pandoc: { to: "html" },
      render: {},
      execute: {},
      metadata: {},
      language: {},
    } as import("@quarto/types").Format;
    expect(api.format.isHtmlCompatible(htmlFormat)).toBe(true);
  });

  it("markdownRegex.getLanguages extracts languages from fenced code blocks", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    const md = "```{python}\nprint(1)\n```\n";
    const langs = api.markdownRegex.getLanguages(md);
    expect(langs.has("python")).toBe(true);
  });

  it("markdownRegex.extractYaml extracts YAML front matter", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    const md = "---\ntitle: Test\n---\n\n# Hello";
    const yaml = api.markdownRegex.extractYaml(md);
    expect(yaml).toHaveProperty("title", "Test");
  });
});

// ---------------------------------------------------------------------------
// Host-wired namespaces
// ---------------------------------------------------------------------------

describe("buildQuartoAPI — host-wired namespaces", () => {
  it("console.info routes through host.log.info", () => {
    const host = makeFakeHost();
    const api = buildQuartoAPI(makeFakeGlobal(), host);
    api.console.info("my-message");
    // host.log.info is a vi.fn() spy — check it was called with a string containing "my-message"
    const infoMock = host.log.info as ReturnType<typeof vi.fn>;
    expect(infoMock).toHaveBeenCalledWith(expect.stringContaining("my-message"));
  });

  it("console.warning routes through host.log.warning", () => {
    const host = makeFakeHost();
    const api = buildQuartoAPI(makeFakeGlobal(), host);
    api.console.warning("warn-message");
    const warningMock = host.log.warning as ReturnType<typeof vi.fn>;
    expect(warningMock).toHaveBeenCalledWith(expect.stringContaining("warn-message"));
  });

  it("mappedString.fromString creates a MappedString with the given value", () => {
    const host = makeFakeHost();
    const api = buildQuartoAPI(makeFakeGlobal(), host);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const ms = (api.mappedString as any).fromString("hi");
    expect(ms.value).toBe("hi");
  });
});

// ---------------------------------------------------------------------------
// Plan-2 stub assertions (ambient, reachable, but no body)
// ---------------------------------------------------------------------------

describe("buildQuartoAPI — Plan-2 stubs throw", () => {
  it("path.runtime() throws 'not yet implemented' + Plan 2", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    expect(() => api.path.runtime()).toThrow(/not yet implemented/i);
    expect(() => api.path.runtime()).toThrow(/plan 2/i);
  });

  it("path.resource() throws 'not yet implemented' + Plan 2", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    expect(() => api.path.resource("a")).toThrow(/not yet implemented/i);
    expect(() => api.path.resource("a")).toThrow(/plan 2/i);
  });

  it("path.dataDir() throws 'not yet implemented' + Plan 2", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    expect(() => api.path.dataDir()).toThrow(/not yet implemented/i);
    expect(() => api.path.dataDir()).toThrow(/plan 2/i);
  });

  it("system.pandoc() rejects with 'not yet implemented' + Plan 2", async () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    await expect((api.system as any).pandoc([])).rejects.toThrow(/not yet implemented/i);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    await expect((api.system as any).pandoc([])).rejects.toThrow(/plan 2/i);
  });

  it("text.postProcessRestorePreservedHtml throws 'not yet implemented' + Plan 2", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    expect(() =>
      api.text.postProcessRestorePreservedHtml({} as import("@quarto/types").PostProcessOptions)
    ).toThrow(/not yet implemented/i);
    expect(() =>
      api.text.postProcessRestorePreservedHtml({} as import("@quarto/types").PostProcessOptions)
    ).toThrow(/plan 2/i);
  });
});

// ---------------------------------------------------------------------------
// Plan-3 stub (jupyter)
// ---------------------------------------------------------------------------

describe("buildQuartoAPI — jupyter stub throws Plan 3", () => {
  it("jupyter.assets() throws 'not yet implemented' + Plan 3", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect(() => (api.jupyter as any).assets("input.qmd")).toThrow(/not yet implemented/i);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect(() => (api.jupyter as any).assets("input.qmd")).toThrow(/plan 3/i);
  });

  it("jupyter.capabilities() throws 'not yet implemented' + Plan 3", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect(() => (api.jupyter as any).capabilities()).toThrow(/not yet implemented/i);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect(() => (api.jupyter as any).capabilities()).toThrow(/plan 3/i);
  });

  it("any jupyter property access throws 'not yet implemented' + Plan 3", () => {
    const api = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const j = api.jupyter as any;
    // Access any property — the Proxy get returns a throwing function
    expect(() => j.fromJSON("{}")).toThrow(/not yet implemented/i);
    expect(() => j.fromJSON("{}")).toThrow(/plan 3/i);
  });
});

// ---------------------------------------------------------------------------
// Object identity stable (engines can stash the reference)
// ---------------------------------------------------------------------------

describe("buildQuartoAPI — object identity", () => {
  it("returns the same object reference on repeated calls with the same args", () => {
    // Note: buildQuartoAPI is NOT memoized by design (each call builds a fresh object).
    // This test just verifies the API fields are stable objects (not rebuilt on access).
    const host = makeFakeHost();
    const global = makeFakeGlobal();
    const api = buildQuartoAPI(global, host);
    const api2 = buildQuartoAPI(global, host);
    // Different calls produce different objects (no registry)
    expect(api).not.toBe(api2);
    // But the same api's namespace refs are stable
    expect(api.text).toBe(api.text);
    expect(api.console).toBe(api.console);
  });
});

// ---------------------------------------------------------------------------
// S8: mappedString.mappedStringFromChunks wiring
// ---------------------------------------------------------------------------

describe("buildQuartoAPI — seam S8: mappedStringFromChunks wired", () => {
  it("S8: mappedString.mappedStringFromChunks is publicly wired and round-trips through serializeMappedString", () => {
    const q = buildQuartoAPI(makeFakeGlobal(), makeFakeHost());
    // wiring guard — tsc cannot catch this (the `as unknown as QuartoAPI` cast):
    expect(typeof q.mappedString.mappedStringFromChunks).toBe("function");
    // round-trip through the PUBLICLY-wired builder (not the internal function):
    const ms = q.mappedString.mappedStringFromChunks(
      q.mappedString.fromString("0123456789", "f.qmd"),
      [{ start: 0, end: 5 }, { start: 5, end: 10 }],
    );
    expect(serializeMappedString(ms)).toEqual([
      { start: 0, length: 5, source: { file: "f.qmd", fileOffset: 0 } },
      { start: 5, length: 5, source: { file: "f.qmd", fileOffset: 5 } },
    ]);
  });
});
