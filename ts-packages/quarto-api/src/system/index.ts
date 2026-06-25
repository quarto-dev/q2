/**
 * @quarto/api — system namespace (HOST-ONLY)
 *
 * Ported from Q1 `core/api/system.ts` + backing modules:
 *   - execProcess:          core/process.ts:46 — marshals to host.process.exec
 *   - isInteractiveSession: core/platform.ts:101 — from host.isInteractive
 *   - runningInCI:          core/ci-info.ts:10 — from host.isCI
 *   - tempContext:          core/temp.ts — via host.fs.makeTempDir/makeTempFile
 *   - onCleanup:            core/cleanup.ts — via host.process.onExit
 *   - pandoc:               STUB (not yet implemented (Plan 2))
 *   - checkRender:          STUB (Plan 2)
 *   - runExternalPreviewServer: STUB (Plan 2)
 *
 * All I/O goes through the injected PlatformHost — never Deno.* / node:*.
 *
 * Factory: makeSystem(host: Pick<PlatformHost, "process" | "fs" | "isInteractive" | "isCI">)
 */

import type { ExecOptions, ExecResult, PlatformHost } from "../platform/index.js";

// ─── Re-export types used by callers ────────────────────────────────────────

export type { ExecOptions, ExecResult };

// ─── Q1 return types ─────────────────────────────────────────────────────────

/**
 * Result of executing an external process.
 * Mirrors Q1 `core/process-types.ts::ProcessResult`.
 * stdout/stderr are optional strings (undefined if not captured).
 */
export interface ProcessResult {
  success: boolean;
  code: number;
  stdout?: string;
  stderr?: string;
}

/**
 * Options for execProcess.
 * Mirrors the subset of Q1's `ExecProcessOptions` that is platform-neutral.
 */
export interface ExecProcessOptions {
  /** The command to execute. */
  cmd: string;
  /** Arguments to the command. */
  args?: string[];
  /** Working directory for the child process. */
  cwd?: string;
  /** Environment variables to set for the child process. */
  env?: Record<string, string>;
  /** Text content to write to stdin (if any). */
  stdin?: string;
  /** stdout mode: "piped" captures output, "inherit" passes through, "null" discards. */
  stdout?: "piped" | "inherit" | "null";
  /** stderr mode: "piped" captures output, "inherit" passes through, "null" discards. */
  stderr?: "piped" | "inherit" | "null";
}

/**
 * A context for creating temporary files and directories.
 * Mirrors Q1 `core/temp-types.ts::TempContext`.
 */
export interface TempContext {
  /** Base directory for all temps created by this context. */
  baseDir: string;
  /**
   * Create a temporary file with string content and return its path.
   * @param content - Text to write to the file.
   * @param opts - Optional prefix/suffix for the temp file.
   */
  createFileFromString(content: string, opts?: { prefix?: string; suffix?: string }): string;
  /**
   * Create an empty temporary file and return its path.
   * @param opts - Optional prefix/suffix for the temp file.
   */
  createFile(opts?: { prefix?: string; suffix?: string }): string;
  /**
   * Create a temporary directory and return its path.
   * @param opts - Optional prefix for the temp dir.
   */
  createDir(opts?: { prefix?: string }): string;
  /** Remove all temporary files/directories created by this context. */
  cleanup(): void;
  /** Register a handler called during cleanup. */
  onCleanup(handler: () => void): void;
}

// ─── SystemNamespace interface ────────────────────────────────────────────────

/** The system namespace interface returned by makeSystem. */
export interface SystemNamespace {
  /**
   * Execute an external process and return its result.
   * Marshals to host.process.exec.
   */
  execProcess(
    options: ExecProcessOptions,
    stdin?: string,
  ): Promise<ProcessResult>;

  /** Return true iff the process is running interactively. */
  isInteractiveSession(): boolean;

  /** Return true iff the process is running inside a CI environment. */
  runningInCI(): boolean;

  /**
   * Create and return a TempContext backed by the host's fs.
   * Registers cleanup with host.process.onExit.
   */
  tempContext(): TempContext;

  /**
   * Register a cleanup handler to be called when the process exits.
   * Routes to host.process.onExit.
   */
  onCleanup(handler: () => void): void;

  /**
   * Execute pandoc with the given arguments.
   * STUB — throws until the engine host provides the pandoc binary path.
   */
  pandoc(args: string[], stdin?: string): Promise<ProcessResult>;

  /**
   * Run a check render.
   * STUB — not yet implemented (Plan 2).
   */
  checkRender(...args: unknown[]): Promise<unknown>;

  /**
   * Run an external preview server.
   * STUB — not yet implemented (Plan 2).
   */
  runExternalPreviewServer(...args: unknown[]): Promise<unknown>;
}

// ─── Stub error helpers ───────────────────────────────────────────────────────

function notYetImplementedError(method: string): Error {
  return new Error(
    `@quarto/api: system.${method}() is not yet implemented (Plan 2)`,
  );
}

// ─── Factory ─────────────────────────────────────────────────────────────────

/**
 * Build the system namespace backed by the given host.
 *
 * @param host - A PlatformHost (or minimal fake) with process, fs,
 *               isInteractive, and isCI.
 */
export function makeSystem(
  host: Pick<PlatformHost, "process" | "fs" | "isInteractive" | "isCI">,
): SystemNamespace {
  // ── execProcess ────────────────────────────────────────────────────────────
  //
  // Marshal Q1's ExecProcessOptions into PlatformHost's ExecOptions, delegate
  // to host.process.exec, then map the raw ExecResult into Q1's ProcessResult.
  //
  // Mapping:
  //   ExecProcessOptions.cmd        → first positional arg (cmd)
  //   ExecProcessOptions.args       → second positional arg (args)
  //   ExecProcessOptions.{cwd,env,stdin} → ExecOptions
  //   ExecProcessOptions.stdout/stderr (mode strings) are advisory; the host
  //     always captures stdout/stderr as strings — the caller uses stdout/stderr
  //     fields of ProcessResult iff they requested "piped" mode.
  //
  // ProcessResult.stdout/stderr are optional (undefined unless mode was "piped").
  async function execProcess(
    options: ExecProcessOptions,
    stdin?: string,
  ): Promise<ProcessResult> {
    const execOpts: ExecOptions = {
      cwd: options.cwd,
      env: options.env,
      stdin: stdin ?? options.stdin,
    };

    const raw: ExecResult = await host.process.exec(
      options.cmd,
      options.args ?? [],
      execOpts,
    );

    // Map ExecResult → ProcessResult
    // stdout/stderr are included iff the caller requested "piped" capture.
    return {
      success: raw.success,
      code: raw.code,
      stdout: options.stdout === "piped" ? raw.stdout : undefined,
      stderr: options.stderr === "piped" ? raw.stderr : undefined,
    };
  }

  // ── isInteractiveSession ───────────────────────────────────────────────────
  function isInteractiveSession(): boolean {
    return host.isInteractive;
  }

  // ── runningInCI ────────────────────────────────────────────────────────────
  function runningInCI(): boolean {
    return host.isCI;
  }

  // ── tempContext ────────────────────────────────────────────────────────────
  //
  // Creates a TempContext backed by host.fs. Cleanup handlers are registered
  // with host.process.onExit so they run on process exit.
  //
  // Q1 model (core/temp.ts): temps are nested under baseDir via `dir` option
  // so a single recursive remove of baseDir reclaims everything. Cleanup is
  // guarded by a `cleaned` flag (like Q1's `if (dir) { … dir = undefined; }`)
  // so calling cleanup() manually AND via the onExit handler is idempotent —
  // each user handler and each remove runs ONCE, regardless of how many times
  // cleanup() is invoked.
  function tempContext(): TempContext {
    // Create the base directory for this context
    const baseDir = host.fs.makeTempDir({ prefix: "quarto-" });

    // Registry of cleanup handlers for this context
    const cleanupHandlers: Array<() => void> = [];

    // Idempotency guard (Q1: `if (dir) { … dir = undefined; }`)
    let cleaned = false;

    // Register a process-exit cleanup for this context
    host.process.onExit(() => {
      cleanup();
    });

    function createFileFromString(
      content: string,
      opts?: { prefix?: string; suffix?: string },
    ): string {
      // Nest under baseDir (Q1 model: `dir` option on makeTempFileSync)
      const path = host.fs.makeTempFile({ prefix: opts?.prefix, suffix: opts?.suffix, dir: baseDir });
      host.fs.writeFileSync(path, content);
      return path;
    }

    function createFile(opts?: { prefix?: string; suffix?: string }): string {
      // Nest under baseDir (Q1 model)
      return host.fs.makeTempFile({ prefix: opts?.prefix, suffix: opts?.suffix, dir: baseDir });
    }

    function createDir(opts?: { prefix?: string }): string {
      // Nest under baseDir (Q1 model)
      return host.fs.makeTempDir({ prefix: opts?.prefix, dir: baseDir });
    }

    function cleanup(): void {
      // Idempotency guard — second call is a no-op (Q1: `if (dir) { … dir = undefined; }`)
      if (cleaned) return;
      cleaned = true;

      // Run registered handlers in LIFO order (matches Q1 core/temp.ts)
      for (let i = cleanupHandlers.length - 1; i >= 0; i--) {
        try {
          cleanupHandlers[i]();
        } catch {
          // Suppress cleanup errors (matches Q1 behavior)
        }
      }
      // Remove baseDir recursively — since all temps are nested under it,
      // this single remove reclaims everything (Q1 model: `safeRemoveIfExists(dir)`).
      try {
        host.fs.remove(baseDir, { recursive: true });
      } catch {
        // Suppress
      }
    }

    function onCleanupLocal(handler: () => void): void {
      cleanupHandlers.push(handler);
    }

    return {
      baseDir,
      createFileFromString,
      createFile,
      createDir,
      cleanup,
      onCleanup: onCleanupLocal,
    };
  }

  // ── onCleanup ──────────────────────────────────────────────────────────────
  function onCleanup(handler: () => void): void {
    host.process.onExit(handler);
  }

  // ── pandoc (STUB) ──────────────────────────────────────────────────────────
  // `async` so the function returns a REJECTED Promise — callers using
  // `.catch()` or `await` will see the error; a synchronous `throw` would
  // fire before `.catch` attaches (unhandled-rejection risk).
  async function pandoc(_args: string[], _stdin?: string): Promise<ProcessResult> {
    throw notYetImplementedError("pandoc");
  }

  // ── checkRender (STUB) ─────────────────────────────────────────────────────
  // `async` for the same reason as pandoc above.
  async function checkRender(..._args: unknown[]): Promise<unknown> {
    throw notYetImplementedError("checkRender");
  }

  // ── runExternalPreviewServer (STUB) ────────────────────────────────────────
  // `async` for the same reason as pandoc above.
  async function runExternalPreviewServer(..._args: unknown[]): Promise<unknown> {
    throw notYetImplementedError("runExternalPreviewServer");
  }

  return {
    execProcess,
    isInteractiveSession,
    runningInCI,
    tempContext,
    onCleanup,
    pandoc,
    checkRender,
    runExternalPreviewServer,
  };
}
