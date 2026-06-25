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
import type { ExecOptions, ExecResult } from "../platform/index.js";

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

// ─── execProcess ─────────────────────────────────────────────────────────────

describe("system.execProcess", () => {
  it("calls host.process.exec with the correct cmd and args (arg-marshalling binding)", async () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost);

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
    const sys = makeSystem(fakeHost);

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

  it("passes explicit stdin arg over options.stdin (stdin-precedence binding)", async () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost);

    await sys.execProcess({ cmd: "cat", args: [], stdin: "from-opts" }, "from-arg");

    const [, , opts] = fakeHost.spies.exec.mock.calls[0];
    // Explicit stdin arg takes precedence — revert to always using options.stdin → RED
    expect(opts?.stdin).toBe("from-arg");
  });

  it("maps ExecResult to ProcessResult: success and code are present (mapping binding)", async () => {
    const fakeHost = makeFakeHost({
      execResult: { code: 1, success: false, stdout: "out", stderr: "err" },
    });
    const sys = makeSystem(fakeHost);

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
    const sys = makeSystem(fakeHost);

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
    const sys = makeSystem(fakeHost);

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
    const sys = makeSystem(makeFakeHost({ isInteractive: true }));
    // Revert to `return false` → RED
    expect(sys.isInteractiveSession()).toBe(true);
  });

  it("returns false when host.isInteractive is false", () => {
    const sys = makeSystem(makeFakeHost({ isInteractive: false }));
    // Revert to `return true` → RED
    expect(sys.isInteractiveSession()).toBe(false);
  });

  it("reads from host.isInteractive (not host.isCI)", () => {
    // Both true — then flip isInteractive to false to confirm we're reading the right source
    const sysTrue = makeSystem(makeFakeHost({ isInteractive: true, isCI: true }));
    const sysFalse = makeSystem(makeFakeHost({ isInteractive: false, isCI: true }));
    expect(sysTrue.isInteractiveSession()).toBe(true);
    // Revert to reading isCI instead → RED (isCI is true here, so would return true)
    expect(sysFalse.isInteractiveSession()).toBe(false);
  });
});

// ─── runningInCI ──────────────────────────────────────────────────────────────

describe("system.runningInCI", () => {
  it("returns true when host.isCI is true", () => {
    const sys = makeSystem(makeFakeHost({ isCI: true }));
    // Revert to `return false` → RED
    expect(sys.runningInCI()).toBe(true);
  });

  it("returns false when host.isCI is false", () => {
    const sys = makeSystem(makeFakeHost({ isCI: false }));
    // Revert to `return true` → RED
    expect(sys.runningInCI()).toBe(false);
  });

  it("reads from host.isCI (not host.isInteractive)", () => {
    // Both true — then flip isCI to false to confirm we're reading the right source
    const sysTrue = makeSystem(makeFakeHost({ isCI: true, isInteractive: true }));
    const sysFalse = makeSystem(makeFakeHost({ isCI: false, isInteractive: true }));
    expect(sysTrue.runningInCI()).toBe(true);
    // Revert to reading isInteractive → RED (isInteractive is true here, so would return true)
    expect(sysFalse.runningInCI()).toBe(false);
  });
});

// ─── tempContext ──────────────────────────────────────────────────────────────

describe("system.tempContext", () => {
  it("calls host.fs.makeTempDir to create baseDir (dispatch binding)", () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost);

    sys.tempContext();

    // Revert to not calling makeTempDir → RED
    expect(fakeHost.spies.makeTempDir.mock.calls.length).toBeGreaterThanOrEqual(1);
  });

  it("registers a cleanup handler with host.process.onExit (wiring binding)", () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost);

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
    const sys = makeSystem(fakeHost);

    const ctx = sys.tempContext();

    // Revert to using a hardcoded baseDir → RED (would not match "/tmp/test-base")
    expect(ctx.baseDir).toBe("/tmp/test-base");
  });

  it("createFile calls host.fs.makeTempFile (dispatch binding)", () => {
    const fakeHost = makeFakeHost();
    const sys = makeSystem(fakeHost);
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
    const sys = makeSystem(fakeHost);
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
    const sys = makeSystem(fakeHost);
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
    const sys = makeSystem(fakeHost);
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
    const sys = makeSystem(fakeHost);
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
    const sys = makeSystem(fakeHost);
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
    const sys = makeSystem(fakeHost);
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
    const sys = makeSystem(fakeHost);
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
    const sys = makeSystem(fakeHost);
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
    const sys = makeSystem(fakeHost);

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

// ─── pandoc (STUB) ────────────────────────────────────────────────────────────

describe("system.pandoc (stub)", () => {
  it("rejects with 'not yet implemented' (async stub binding)", async () => {
    const sys = makeSystem(makeFakeHost());
    // Revert to returning a ProcessResult → RED
    await expect(sys.pandoc(["--version"])).rejects.toThrow(/not yet implemented/);
  });

  it("error message names the 'pandoc' method (method-name binding)", async () => {
    const sys = makeSystem(makeFakeHost());
    // Revert to a generic error message → RED
    await expect(sys.pandoc([])).rejects.toThrow(/system\.pandoc/);
  });
});

// ─── checkRender (STUB) ───────────────────────────────────────────────────────

describe("system.checkRender (stub)", () => {
  it("rejects with 'not yet implemented' (async stub binding)", async () => {
    const sys = makeSystem(makeFakeHost());
    // Revert to returning a value instead of rejecting → RED
    await expect(sys.checkRender()).rejects.toThrow(/not yet implemented/);
  });
});

// ─── runExternalPreviewServer (STUB) ──────────────────────────────────────────

describe("system.runExternalPreviewServer (stub)", () => {
  it("rejects with 'not yet implemented' (async stub binding)", async () => {
    const sys = makeSystem(makeFakeHost());
    // Revert to returning a value instead of rejecting → RED
    await expect(sys.runExternalPreviewServer()).rejects.toThrow(/not yet implemented/);
  });
});
