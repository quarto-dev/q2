/**
 * @quarto/api — system namespace tests (HOST-ONLY)
 *
 * Seam-spec binding (2aa-seam-spec.md §system):
 *   - execProcess: assert (a) host was called with the right (cmd,args,opts),
 *     (b) the namespace MAPS the raw ExecResult into Q1's ProcessResult shape
 *     (assert the TRANSFORMATION, not just pass-through), (c) spy count ≥ 1.
 *   - isInteractiveSession/runningInCI: inject both ways; assert reads the right source.
 *   - tempContext/onCleanup: spy on fs.makeTempDir, fs.makeTempFile,
 *     fs.writeFileSync, process.onExit — assert dispatch (count ≥ 1).
 *   - pandoc: throws + /not yet implemented/.
 *   - checkRender/runExternalPreviewServer: assert they throw.
 *
 * ECHO TRAP AVOIDED: tests assert the TRANSFORMATION / DISPATCH, not that
 * the fake's return value flowed through unchanged.
 * No Deno.* / node:* anywhere.
 */

import { describe, it, expect, vi } from "vitest";
import { makeSystem } from "./index.js";
import type { ExecOptions, ExecResult, HostGlobalConfig } from "../platform/index.js";
import type { CheckRenderOptions } from "@quarto/types";

// ─── Minimal fake host builder ────────────────────────────────────────────────

function makeFakeHost(overrides?: {
  isInteractive?: boolean;
  isCI?: boolean;
  execResult?: Partial<ExecResult>;
}) {
  const isInteractive = overrides?.isInteractive ?? false;
  const isCI = overrides?.isCI ?? false;
  const baseExecResult: ExecResult = {
    code: overrides?.execResult?.code ?? 0,
    success: overrides?.execResult?.success ?? true,
    stdout: overrides?.execResult?.stdout ?? "stdout-output",
    stderr: overrides?.execResult?.stderr ?? "stderr-output",
  };

  // Spies
  const execSpy = vi.fn(
    (_cmd: string, _args: string[], _opts?: ExecOptions): Promise<ExecResult> =>
      Promise.resolve(baseExecResult),
  );
  const onExitSpy = vi.fn((_handler: () => void): void => {});
  const exitSpy = vi.fn((_code: number): never => { throw new Error("exit"); });
  const makeTempDirSpy = vi.fn((_opts?: { prefix?: string; dir?: string }): string => "/tmp/fake-dir");
  const makeTempFileSpy = vi.fn((_opts?: { prefix?: string; suffix?: string; dir?: string }): string => "/tmp/fake-file");
  const writeFileSyncSpy = vi.fn((_path: string, _content: string | Uint8Array): void => {});
  const removeSpy = vi.fn((_path: string, _opts?: { recursive?: boolean }): void => {});

  return {
    isInteractive,
    isCI,
    process: {
      exec: execSpy,
      onExit: onExitSpy,
      exit: exitSpy,
    },
    fs: {
      readTextFileSync: vi.fn((_path: string) => ""),
      writeFileSync: writeFileSyncSpy,
      exists: vi.fn((_path: string) => false),
      ensureDir: vi.fn((_path: string) => {}),
      makeTempDir: makeTempDirSpy,
      makeTempFile: makeTempFileSpy,
      remove: removeSpy,
      walk: vi.fn(
        (_root: string, _opts?: { maxDepth?: number; includeDirs?: boolean }) =>
          [] as Array<{ path: string; isFile: boolean; isDirectory: boolean }>,
      ),
    },
    // Expose spies for assertion
    spies: {
      exec: execSpy,
      onExit: onExitSpy,
      makeTempDir: makeTempDirSpy,
      makeTempFile: makeTempFileSpy,
      writeFileSync: writeFileSyncSpy,
      remove: removeSpy,
    },
  };
}

// ─── Shared fake global config ────────────────────────────────────────────────

/** Minimal HostGlobalConfig for tests that don't exercise pandoc. */
const fakeGlobal: HostGlobalConfig = {
  resourceDir: "/rs",
  runtimeDir: "/rt",
  dataDir: "/dt",
  pandocPath: undefined,
};

// ─── execProcess ─────────────────────────────────────────────────────────────

describe("system.execProcess", () => {
  it("calls host.process.exec with the correct cmd and args (arg-marshalling binding)", async () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, fakeGlobal);

    await sys.execProcess({ cmd: "echo", args: ["hello", "world"] });

    // Revert arg-marshalling (e.g. pass [] instead of args) → RED
    expect(fakeHost.spies.exec).toHaveBeenCalledWith(
      "echo",
      ["hello", "world"],
      expect.objectContaining({}),
    );
    // Spy count binding: revert to no-op body → RED
    expect(fakeHost.spies.exec.mock.calls.length).toBeGreaterThanOrEqual(1);
  });

  it("passes cwd and env through ExecOptions (opts-marshalling binding)", async () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, fakeGlobal);

    await sys.execProcess({
      cmd: "tool",
      args: [],
      cwd: "/project",
      env: { FOO: "bar" },
    });

    const [, , opts] = fakeHost.spies.exec.mock.calls[0];
    // Revert to not passing cwd/env → RED
    expect(opts?.cwd).toBe("/project");
    expect(opts?.env).toEqual({ FOO: "bar" });
  });

  // §2aa — ratified test change (user-approved 2026-07-01, return-to-Q1 stdin fix):
  // options.stdin is a stream MODE, not content; content comes only from the positional param.
  it("Q1 semantics: content from positional param only; options.stdin mode is NOT piped as content (Q1-stdin-semantics binding)", async () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, fakeGlobal);

    // (1) positional stdin arg → ExecOptions.stdin (content from positional param)
    //     options.stdin="null" is a mode — must NOT override the positional content
    await sys.execProcess({ cmd: "cat", stdin: "null" }, "from-arg");
    const [, , optsWithArg] = fakeHost.spies.exec.mock.calls[0];
    expect(optsWithArg?.stdin).toBe("from-arg");

    // (2) no positional stdin; options.stdin="piped" is a mode — must NOT become content
    // Named revert → RED: re-introduce `stdin: stdin ?? options.stdin` →
    //   ExecOptions.stdin becomes "piped" (the mode string) instead of undefined
    fakeHost.spies.exec.mockClear();
    await sys.execProcess({ cmd: "cat", stdin: "piped" }); // no positional content
    const [, , optsWithMode] = fakeHost.spies.exec.mock.calls[0];
    expect(optsWithMode?.stdin).toBeUndefined(); // mode is NOT content
  });

  it("maps ExecResult to ProcessResult: success and code are present (mapping binding)", async () => {
    const fakeHost = makeFakeHost({
      execResult: { code: 1, success: false, stdout: "out", stderr: "err" },
    });
    const sys = makeSystem(fakeHost, fakeGlobal);

    const result = await sys.execProcess({ cmd: "fail", args: [] });

    // Revert result-mapping (e.g. return raw ExecResult) — the shape differs: ProcessResult has
    // optional stdout/stderr vs. ExecResult's mandatory fields → assert the MAPPED shape
    expect(result.success).toBe(false);
    expect(result.code).toBe(1);
  });

  it("maps stdout only when stdout mode is 'piped' (conditional-mapping binding)", async () => {
    const fakeHost = makeFakeHost({
      execResult: { code: 0, success: true, stdout: "captured", stderr: "err" },
    });
    const sys = makeSystem(fakeHost, fakeGlobal);

    const piped = await sys.execProcess({ cmd: "cmd", args: [], stdout: "piped" });
    const inherit = await sys.execProcess({ cmd: "cmd", args: [], stdout: "inherit" });
    const noMode = await sys.execProcess({ cmd: "cmd", args: [] });

    // Revert to always including stdout → RED (inherit/noMode would have stdout defined)
    expect(piped.stdout).toBe("captured");
    expect(inherit.stdout).toBeUndefined();
    expect(noMode.stdout).toBeUndefined();
  });

  it("maps stderr only when stderr mode is 'piped' (conditional-mapping binding)", async () => {
    const fakeHost = makeFakeHost({
      execResult: { code: 0, success: true, stdout: "out", stderr: "captured-err" },
    });
    const sys = makeSystem(fakeHost, fakeGlobal);

    const piped = await sys.execProcess({ cmd: "cmd", args: [], stderr: "piped" });
    const inherit = await sys.execProcess({ cmd: "cmd", args: [], stderr: "inherit" });

    // Revert to always including stderr → RED
    expect(piped.stderr).toBe("captured-err");
    expect(inherit.stderr).toBeUndefined();
  });
});

// ─── isInteractiveSession ─────────────────────────────────────────────────────

describe("system.isInteractiveSession", () => {
  it("returns true when host.isInteractive is true", () => {
    const sys = makeSystem(makeFakeHost({ isInteractive: true }), fakeGlobal);
    // Revert to `return false` → RED
    expect(sys.isInteractiveSession()).toBe(true);
  });

  it("returns false when host.isInteractive is false", () => {
    const sys = makeSystem(makeFakeHost({ isInteractive: false }), fakeGlobal);
    // Revert to `return true` → RED
    expect(sys.isInteractiveSession()).toBe(false);
  });

  it("reads from host.isInteractive (not host.isCI)", () => {
    // Both true — then flip isInteractive to false to confirm we're reading the right source
    const sysTrue = makeSystem(makeFakeHost({ isInteractive: true, isCI: true }), fakeGlobal);
    const sysFalse = makeSystem(makeFakeHost({ isInteractive: false, isCI: true }), fakeGlobal);
    expect(sysTrue.isInteractiveSession()).toBe(true);
    // Revert to reading isCI instead → RED (isCI is true here, so would return true)
    expect(sysFalse.isInteractiveSession()).toBe(false);
  });
});

// ─── runningInCI ──────────────────────────────────────────────────────────────

describe("system.runningInCI", () => {
  it("returns true when host.isCI is true", () => {
    const sys = makeSystem(makeFakeHost({ isCI: true }), fakeGlobal);
    // Revert to `return false` → RED
    expect(sys.runningInCI()).toBe(true);
  });

  it("returns false when host.isCI is false", () => {
    const sys = makeSystem(makeFakeHost({ isCI: false }), fakeGlobal);
    // Revert to `return true` → RED
    expect(sys.runningInCI()).toBe(false);
  });

  it("reads from host.isCI (not host.isInteractive)", () => {
    // Both true — then flip isCI to false to confirm we're reading the right source
    const sysTrue = makeSystem(makeFakeHost({ isCI: true, isInteractive: true }), fakeGlobal);
    const sysFalse = makeSystem(makeFakeHost({ isCI: false, isInteractive: true }), fakeGlobal);
    expect(sysTrue.runningInCI()).toBe(true);
    // Revert to reading isInteractive → RED (isInteractive is true here, so would return true)
    expect(sysFalse.runningInCI()).toBe(false);
  });
});

// ─── tempContext ──────────────────────────────────────────────────────────────

describe("system.tempContext", () => {
  it("calls host.fs.makeTempDir to create baseDir (dispatch binding)", () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, fakeGlobal);

    sys.tempContext();

    // Revert to not calling makeTempDir → RED
    expect(fakeHost.spies.makeTempDir.mock.calls.length).toBeGreaterThanOrEqual(1);
  });

  it("registers a cleanup handler with host.process.onExit (wiring binding)", () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, fakeGlobal);

    sys.tempContext();

    // Revert to not registering with onExit → RED
    expect(fakeHost.spies.onExit.mock.calls.length).toBeGreaterThanOrEqual(1);
    // The registered handler must be a function
    const [handler] = fakeHost.spies.onExit.mock.calls[0];
    expect(typeof handler).toBe("function");
  });

  it("exposes the makeTempDir result as baseDir (result-routing binding)", () => {
    // Override the fake to return a known path
    const fakeHost = makeFakeHost();
    fakeHost.spies.makeTempDir.mockReturnValueOnce("/tmp/test-base");
    const sys = makeSystem(fakeHost, fakeGlobal);

    const ctx = sys.tempContext();

    // Revert to using a hardcoded baseDir → RED (would not match "/tmp/test-base")
    expect(ctx.baseDir).toBe("/tmp/test-base");
  });

  it("createFile calls host.fs.makeTempFile (dispatch binding)", () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, fakeGlobal);
    const ctx = sys.tempContext();

    // Reset after tempContext creation
    fakeHost.spies.makeTempFile.mockClear();
    ctx.createFile({ prefix: "test-" });

    // Revert to not calling makeTempFile → RED
    expect(fakeHost.spies.makeTempFile.mock.calls.length).toBeGreaterThanOrEqual(1);
    const [opts] = fakeHost.spies.makeTempFile.mock.calls[0];
    expect(opts?.prefix).toBe("test-");
  });

  it("createFileFromString calls makeTempFile then writeFileSync (dispatch+write binding)", () => {
    const fakeHost = makeFakeHost();
    fakeHost.spies.makeTempFile.mockReturnValue("/tmp/content-file");
    const sys = makeSystem(fakeHost, fakeGlobal);
    const ctx = sys.tempContext();

    fakeHost.spies.makeTempFile.mockClear();
    fakeHost.spies.writeFileSync.mockClear();

    const path = ctx.createFileFromString("hello content");

    // Revert to not writing content → RED
    expect(fakeHost.spies.makeTempFile.mock.calls.length).toBeGreaterThanOrEqual(1);
    expect(fakeHost.spies.writeFileSync).toHaveBeenCalledWith("/tmp/content-file", "hello content");
    expect(path).toBe("/tmp/content-file");
  });

  it("createDir calls host.fs.makeTempDir (dispatch binding)", () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, fakeGlobal);
    const ctx = sys.tempContext();

    fakeHost.spies.makeTempDir.mockClear();
    ctx.createDir({ prefix: "sub-" });

    // Revert to not calling makeTempDir → RED
    expect(fakeHost.spies.makeTempDir.mock.calls.length).toBeGreaterThanOrEqual(1);
    const [opts] = fakeHost.spies.makeTempDir.mock.calls[0];
    expect(opts?.prefix).toBe("sub-");
  });

  it("cleanup runs handlers in LIFO order (reverse registration — matches Q1 core/temp.ts)", () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, fakeGlobal);
    const ctx = sys.tempContext();

    const order: number[] = [];
    ctx.onCleanup(() => order.push(1));
    ctx.onCleanup(() => order.push(2));

    ctx.cleanup();

    // LIFO: handler 2 registered last, must run first
    // Revert to FIFO (for loop without reverse) → order becomes [1,2] → RED
    expect(order).toEqual([2, 1]);
  });

  it("cleanup removes baseDir recursively (Q1 containment: one recursive remove reclaims all nested temps)", () => {
    const fakeHost = makeFakeHost();
    // Return known paths so we can assert exactly what was removed
    fakeHost.spies.makeTempDir.mockReturnValueOnce("/tmp/base-dir");
    fakeHost.spies.makeTempFile.mockReturnValueOnce("/tmp/base-dir/created-file");
    const sys = makeSystem(fakeHost, fakeGlobal);
    const ctx = sys.tempContext();

    ctx.createFile();
    fakeHost.spies.remove.mockClear();

    ctx.cleanup();

    // Q1 model: one recursive baseDir remove suffices (no separate per-path removes)
    // Revert to not calling remove → RED (remove.mock.calls.length === 0)
    const removedPaths = fakeHost.spies.remove.mock.calls.map(([p]) => p);
    expect(removedPaths).toContain("/tmp/base-dir");
    // Must be called with recursive: true
    const baseDirCall = fakeHost.spies.remove.mock.calls.find(([p]) => p === "/tmp/base-dir");
    expect(baseDirCall?.[1]).toEqual({ recursive: true });
  });

  // ── Item 1: idempotency ──────────────────────────────────────────────────

  it("cleanup is idempotent — calling twice runs each handler ONCE and removes baseDir ONCE (idempotency binding)", () => {
    const fakeHost = makeFakeHost();
    fakeHost.spies.makeTempDir.mockReturnValueOnce("/tmp/idem-base");
    const sys = makeSystem(fakeHost, fakeGlobal);
    const ctx = sys.tempContext();

    ctx.createFile(); // nested under baseDir — no separate tracking needed

    const handlerA = vi.fn();
    const handlerB = vi.fn();
    ctx.onCleanup(handlerA);
    ctx.onCleanup(handlerB);

    fakeHost.spies.remove.mockClear();

    // Call cleanup twice
    ctx.cleanup();
    ctx.cleanup();

    // Revert guard (remove idempotency check) → handlers fire twice → RED
    expect(handlerA).toHaveBeenCalledTimes(1);
    expect(handlerB).toHaveBeenCalledTimes(1);

    // baseDir removed exactly once (recursive, covers all nested temps)
    const removeCalls = fakeHost.spies.remove.mock.calls.map(([p]) => p);
    expect(removeCalls.filter((p) => p === "/tmp/idem-base").length).toBe(1);
  });

  // ── Item 2: nested temps under baseDir ──────────────────────────────────

  it("createFile passes dir===baseDir to makeTempFile (nesting binding)", () => {
    const fakeHost = makeFakeHost();
    fakeHost.spies.makeTempDir.mockReturnValueOnce("/tmp/nest-base");
    const sys = makeSystem(fakeHost, fakeGlobal);
    const ctx = sys.tempContext();

    fakeHost.spies.makeTempFile.mockClear();
    ctx.createFile({ prefix: "f-" });

    const [opts] = fakeHost.spies.makeTempFile.mock.calls[0];
    // Revert to not passing dir → opts.dir undefined → RED
    expect(opts?.dir).toBe("/tmp/nest-base");
    expect(opts?.prefix).toBe("f-");
  });

  it("createDir passes dir===baseDir to makeTempDir (nesting binding)", () => {
    const fakeHost = makeFakeHost();
    fakeHost.spies.makeTempDir.mockReturnValueOnce("/tmp/nest-base2");
    const sys = makeSystem(fakeHost, fakeGlobal);
    const ctx = sys.tempContext();

    fakeHost.spies.makeTempDir.mockClear();
    ctx.createDir({ prefix: "d-" });

    const [opts] = fakeHost.spies.makeTempDir.mock.calls[0];
    // Revert to not passing dir → opts.dir undefined → RED
    expect(opts?.dir).toBe("/tmp/nest-base2");
    expect(opts?.prefix).toBe("d-");
  });

  it("cleanup removes baseDir recursively (Q1 containment model: one recursive remove suffices)", () => {
    const fakeHost = makeFakeHost();
    fakeHost.spies.makeTempDir.mockReturnValueOnce("/tmp/contain-base");
    const sys = makeSystem(fakeHost, fakeGlobal);
    const ctx = sys.tempContext();

    fakeHost.spies.remove.mockClear();
    ctx.cleanup();

    const baseDirCall = fakeHost.spies.remove.mock.calls.find(([p]) => p === "/tmp/contain-base");
    // Revert to non-recursive removal → RED
    expect(baseDirCall).toBeDefined();
    expect(baseDirCall?.[1]).toEqual({ recursive: true });
  });
});

// ─── onCleanup ────────────────────────────────────────────────────────────────

describe("system.onCleanup", () => {
  it("delegates to host.process.onExit (dispatch binding)", () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, fakeGlobal);

    const handler = vi.fn();
    sys.onCleanup(handler);

    // Revert to not calling onExit → RED
    expect(fakeHost.spies.onExit.mock.calls.length).toBeGreaterThanOrEqual(1);
    // The registered handler is the one we passed
    const [registeredHandler] = fakeHost.spies.onExit.mock.calls[
      fakeHost.spies.onExit.mock.calls.length - 1
    ];
    expect(typeof registeredHandler).toBe("function");
  });
});

// ─── pandoc — errors-on-None (Plan 2 Phase A) ────────────────────────────────

describe("system.pandoc — pandocPath absent", () => {
  it("rejects with 'pandoc unavailable' when pandocPath is undefined (None-guard binding)", async () => {
    const sys = makeSystem(makeFakeHost(), { ...fakeGlobal, pandocPath: undefined });

    // Revert → RED: remove the None-guard from pandoc
    await expect(sys.pandoc([])).rejects.toThrow(/pandoc unavailable/);
  });

  it("does NOT reject with 'not yet implemented' — pandoc is no longer a stub", async () => {
    const sys = makeSystem(makeFakeHost(), { ...fakeGlobal, pandocPath: undefined });

    // Revert → RED: restore the notYetImplementedError("pandoc") throw → RED
    await expect(sys.pandoc([])).rejects.not.toThrow(/not yet implemented/);
  });

  it("rejects with 'pandoc unavailable' when pandocPath is null (None-guard binding)", async () => {
    const sys = makeSystem(makeFakeHost(), { ...fakeGlobal, pandocPath: null });

    // Revert → RED: remove the None-guard → RED
    await expect(sys.pandoc([])).rejects.toThrow(/pandoc unavailable/);
  });
});

// ─── pandoc — happy path / global-seam (Plan 2 Phase A) ──────────────────────

describe("system.pandoc — pandocPath present (global-seam binding)", () => {
  it("calls host.process.exec with pandocPath, args, and stdin (global-seam binding)", async () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, { ...fakeGlobal, pandocPath: "/usr/bin/pandoc" });

    await sys.pandoc(["--version"], "in");

    // Revert → RED: don't thread/read global.pandocPath
    expect(fakeHost.spies.exec).toHaveBeenCalledWith(
      "/usr/bin/pandoc",
      ["--version"],
      expect.objectContaining({ stdin: "in" }),
    );
    expect(fakeHost.spies.exec.mock.calls.length).toBeGreaterThanOrEqual(1);
  });
});

// ─── T-B1a: positional-wiring (Plan 2 B1) ────────────────────────────────────

describe("system.execProcess — T-B1a positional params thread to ExecOptions", () => {
  it("T-B1a: mergeOutput and stderrFilter positional args reach ExecOptions (positional-wiring binding)", async () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost, fakeGlobal);

    const f = (s: string) => `F:${s}`;
    // Q1 positional call shape: (options, stdin, mergeOutput, stderrFilter, respectStreams, timeout)
    await sys.execProcess({ cmd: "x", args: [] }, "in", "stdout>stderr", f);

    const [, , opts] = fakeHost.spies.exec.mock.calls[0];
    // Revert → RED: drop mergeOutput/stderrFilter from positional→ExecOptions threading
    expect(opts?.mergeOutput).toBe("stdout>stderr");
    expect(opts?.stderrFilter).toBe(f); // same function reference
  });
});

// ─── T-B1c-gate: merge-aware stdout gating (Plan 2 B1) ────────────────────────

describe("system.execProcess — T-B1c-gate merge-aware stdout gating", () => {
  it("T-B1c-gate: stdout>stderr merge — ProcessResult.stdout reflects empty merge-source (merge-gating binding)", async () => {
    const fakeHost = makeFakeHost();
    // Override exec to be merge-aware: when mergeOutput="stdout>stderr" is in opts,
    // the host returns stdout="" (empty — content was merged into stderr).
    fakeHost.spies.exec.mockImplementation(
      (_cmd: string, _args: string[], opts?: ExecOptions) => {
        if (opts?.mergeOutput === "stdout>stderr") {
          return Promise.resolve({ code: 0, success: true, stdout: "", stderr: "MERGED_OUT_AND_ERR" });
        }
        return Promise.resolve({ code: 0, success: true, stdout: "normal-stdout", stderr: "normal-stderr" });
      },
    );

    const sys = makeSystem(fakeHost, fakeGlobal);

    const result = await sys.execProcess(
      { cmd: "x", args: [], stdout: "piped", stderr: "piped" },
      undefined,
      "stdout>stderr",
    );

    // Revert → RED: remove mergeOutput threading in execOpts →
    //   fake host returns "normal-stdout" for stdout → result.stdout !== "" → FAIL
    expect(result.stdout).toBe(""); // merged to stderr → stdout is empty
    expect(result.stderr).toBe("MERGED_OUT_AND_ERR"); // all output here
  });
});

// ─── checkRender (STUB) ───────────────────────────────────────────────────────

describe("system.checkRender (stub)", () => {
  it("rejects with 'not yet implemented' (async stub binding)", async () => {
    const sys = makeSystem(makeFakeHost(), fakeGlobal);
    // checkRender STAYS async — a rejected promise protects `.catch()`-style
    // callers (§2aa stub contract). Minimal valid arg; the body throws first.
    // Revert to returning a value instead of rejecting → RED
    await expect(sys.checkRender({} as CheckRenderOptions)).rejects.toThrow(
      /not yet implemented/,
    );
  });
});

// ─── runExternalPreviewServer (STUB) ──────────────────────────────────────────

describe("system.runExternalPreviewServer (stub)", () => {
  it("throws 'not yet implemented' SYNCHRONOUSLY (sync stub binding)", () => {
    const sys = makeSystem(makeFakeHost(), fakeGlobal);
    // Q1 returns PreviewServer SYNCHRONOUSLY, so the stub throws synchronously
    // (NOT a rejected promise). Minimal valid arg; the body throws first.
    // Revert to an `async` body (rejected promise) → this synchronous throw
    // assertion fails → RED.
    expect(() =>
      sys.runExternalPreviewServer({ cmd: ["echo"], readyPattern: /ready/ }),
    ).toThrow(/not yet implemented/);
  });
});
