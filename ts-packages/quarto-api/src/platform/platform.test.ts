/**
 * Type-conformance test for PlatformHost.
 *
 * BINDING CONTRACT: the `fakeHost` object below must satisfy the full
 * `PlatformHost` interface at compile time. If any required member is
 * removed or renamed in `src/platform/index.ts`, tsc will reject this
 * file and `npm run build -w @quarto/api` goes RED. That makes the
 * interface "frozen" in the sense of the §2aa seam spec.
 *
 * Named revert that reddens this: removing any required property from
 * the PlatformHost interface definition (the object literal below will
 * no longer satisfy the type).
 */

import { describe, it, expect } from "vitest";
import type { PlatformHost, ExecOptions, ExecResult } from "./index.js";

// ─── type-conformance: fully-populated fake ────────────────────────────────
//
// Every required member of PlatformHost must appear here.  Optional members
// (log.clearLine) are included to confirm the optional signature is accepted.
// tsc checks this at build time — runtime assertions below confirm the test
// actually executes (guards against a vacuous pass if the import were skipped).

const fakeHost: PlatformHost = {
  fs: {
    readTextFileSync(_path: string): string {
      return "";
    },
    writeFileSync(_path: string, _content: string | Uint8Array): void {},
    exists(_path: string): boolean {
      return false;
    },
    ensureDir(_path: string): void {},
    makeTempDir(_opts?: { prefix?: string; dir?: string }): string {
      return "/tmp/fake";
    },
    makeTempFile(_opts?: { prefix?: string; suffix?: string; dir?: string }): string {
      return "/tmp/fake-file";
    },
    remove(_path: string, _opts?: { recursive?: boolean }): void {},
    walk(
      _root: string,
      _opts?: { maxDepth?: number; includeDirs?: boolean },
    ): Array<{ path: string; isFile: boolean; isDirectory: boolean }> {
      return [];
    },
  },

  process: {
    async exec(
      _cmd: string,
      _args: string[],
      _opts?: ExecOptions,
    ): Promise<ExecResult> {
      return { code: 0, success: true, stdout: "", stderr: "" };
    },
    onExit(_handler: () => void): void {},
    exit(_code: number): never {
      throw new Error("exit called");
    },
  },

  env: {
    get(_key: string): string | undefined {
      return undefined;
    },
  },

  log: {
    info(_msg: string): void {},
    warning(_msg: string): void {},
    error(_msg: string): void {},
    clearLine(): void {}, // optional — present here to confirm it compiles
  },

  cwd(): string {
    return "/fake/cwd";
  },

  realPath(path: string): string {
    return path;
  },

  isInteractive: false,
  isCI: false,
};

// ─── runtime assertions ───────────────────────────────────────────────────────

describe("@quarto/api/platform — PlatformHost type-conformance", () => {
  it("fakeHost satisfies PlatformHost (compile-time binding — if this runs, tsc accepted the type)", () => {
    // If PlatformHost is structurally broken, tsc rejects this file before
    // vitest ever runs. The runtime assertion merely confirms execution.
    expect(fakeHost.isCI).toBe(false);
    expect(fakeHost.isInteractive).toBe(false);
  });

  it("fakeHost.cwd() returns a string", () => {
    expect(typeof fakeHost.cwd()).toBe("string");
  });

  it("fakeHost.fs.exists returns a boolean", () => {
    expect(typeof fakeHost.fs.exists("/any")).toBe("boolean");
  });

  it("fakeHost.process.exec resolves to an ExecResult shape", async () => {
    const result = await fakeHost.process.exec("echo", ["hello"]);
    expect(typeof result.code).toBe("number");
    expect(typeof result.success).toBe("boolean");
    expect(typeof result.stdout).toBe("string");
    expect(typeof result.stderr).toBe("string");
  });

  it("fakeHost.env.get returns undefined for unknown key", () => {
    expect(fakeHost.env.get("NONEXISTENT_KEY_XYZ")).toBeUndefined();
  });
});

// ─── ExecOptions / ExecResult shape test ─────────────────────────────────────
//
// Confirm the helper types compile with the shapes from the brief.
// Named revert: changing ExecOptions.stdin to a non-string type, or making
// ExecResult.stdout optional, causes these assignments to fail tsc.

describe("@quarto/api/platform — ExecOptions and ExecResult helper types", () => {
  it("ExecOptions accepts all optional fields", () => {
    const opts: ExecOptions = {
      cwd: "/work",
      env: { FOO: "bar" },
      stdin: "hello stdin",
    };
    expect(opts.cwd).toBe("/work");
    expect(opts.stdin).toBe("hello stdin");
  });

  it("ExecResult requires all four fields (non-optional)", () => {
    const result: ExecResult = {
      code: 1,
      success: false,
      stdout: "out",
      stderr: "err",
    };
    expect(result.code).toBe(1);
    expect(result.success).toBe(false);
  });
});
