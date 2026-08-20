/**
 * @quarto/api — system namespace (HOST-ONLY)
 *
 * Ported from Q1 `core/api/system.ts` + backing modules:
 *   - execProcess:          core/process.ts:46 — marshals to host.process.exec
 *   - isInteractiveSession: core/platform.ts:101 — from host.isInteractive
 *   - runningInCI:          core/ci-info.ts:10 — from host.isCI
 *   - tempContext:          core/temp.ts — via host.fs.makeTempDir/makeTempFile
 *   - onCleanup:            core/cleanup.ts — via host.process.onExit
 *   - pandoc:               routes through host.process.exec at global.pandocPath
 *   - checkRender:          STUB (Plan 2)
 *   - runExternalPreviewServer: STUB (Plan 2)
 *
 * All I/O goes through the injected PlatformHost — never Deno.* / node:*.
 *
 * Factory: makeSystem(host: Pick<PlatformHost, "process" | "fs" | "isInteractive" | "isCI">, global: HostGlobalConfig)
 */

import type { ExecOptions, ExecResult, HostGlobalConfig, PlatformHost } from "../platform/index.js";
import type {
  QuartoAPI,
  PreviewServer,
  CheckRenderOptions,
  CheckRenderResult,
  ExecProcessOptions,
  ProcessResult,
} from "@quarto/types";

// ─── Re-export types used by callers ────────────────────────────────────────

export type { ExecOptions, ExecResult };

// ExecProcessOptions and ProcessResult are re-exported from @quarto/types
// (single owner — no local duplicate; the vendored definition is the contract).
export type { ExecProcessOptions, ProcessResult };

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

// ─── SystemNamespace (derived from the SDK contract) ──────────────────────────
//
// Fully-host namespace: `makeSystem` returns the WHOLE namespace, so we DERIVE
// it from the vendored SDK contract (`QuartoAPI["system"]`) rather than redefine
// it (Plan 2 B2, Fix B). A future SDK method addition then becomes a compile
// error here until the factory implements it — the whole point of B2.
//
// Clean direct derive (no carve-outs). The B2 finding M-B2-stdin is resolved:
// `ExecProcessOptions.stdin` is now the Q1-faithful stream MODE (`"piped" |
// "inherit" | "null"`), sourced from @quarto/types. Content belongs on the
// 2nd positional param (`stdin?: string`), which is how Q1/knitr calls it.
// Full stdin mode-honoring (inherit/null) has no in-scope caller and is out
// of scope (advisory, same as options.stdout/stderr).
export type SystemNamespace = QuartoAPI["system"];

// ─── Stub error helper ────────────────────────────────────────────────────────

// Used by checkRender and runExternalPreviewServer stubs (pandoc is now real).
function notYetImplementedError(method: string): Error {
  return new Error(
    `@quarto/api: system.${method}() is not yet implemented (Plan 2)`,
  );
}

// ─── Factory ─────────────────────────────────────────────────────────────────

/**
 * Build the system namespace backed by the given host.
 *
 * @param host   - A PlatformHost (or minimal fake) with process, fs,
 *                 isInteractive, and isCI.
 * @param global - Process-stable host config (pandoc path, dirs, etc.).
 */
export function makeSystem(
  host: Pick<PlatformHost, "process" | "fs" | "isInteractive" | "isCI">,
  global: HostGlobalConfig,
): SystemNamespace {
  // ── execProcess ────────────────────────────────────────────────────────────
  //
  // Marshal Q1's six-positional ExecProcessOptions + knobs into PlatformHost's
  // ExecOptions, delegate to host.process.exec, then map the raw ExecResult
  // into Q1's ProcessResult.
  //
  // Mapping:
  //   ExecProcessOptions.cmd           → first positional arg (cmd)
  //   ExecProcessOptions.args          → second positional arg (args)
  //   ExecProcessOptions.{cwd,env}     → ExecOptions
  //   positional stdin param           → ExecOptions.stdin (content only)
  //   mergeOutput / stderrFilter / respectStreams / timeout → ExecOptions
  //   ExecProcessOptions.stdin/stdout/stderr (mode strings) are advisory — the
  //     host always captures both streams; ProcessResult.stdout/stderr are
  //     included iff the caller requested "piped" mode. Full stdin mode-honoring
  //     (inherit/null) has no in-scope caller and is out of scope. When
  //     mergeOutput is active, the host sets the source stream's field to ""
  //     (empty), which passes through the gating correctly.
  //
  // ProcessResult.stdout/stderr are optional (undefined unless mode was "piped").
  // accepted-untested (Plan 4b-D): mergeOutput/stderrFilter/respectStreams/
  // timeout are marshalled through to ExecOptions below but have no in-scope
  // consumer to exercise them end-to-end. Known v1 limitation, improvable
  // later — no test required.
  async function execProcess(
    options: ExecProcessOptions,
    stdin?: string,
    mergeOutput?: "stderr>stdout" | "stdout>stderr",
    stderrFilter?: (output: string) => string,
    respectStreams?: boolean,
    timeout?: number,
  ): Promise<ProcessResult> {
    const execOpts: ExecOptions = {
      cwd: options.cwd,
      env: options.env,
      stdin: stdin,
      mergeOutput,
      stderrFilter,
      respectStreams,
      timeout,
    };

    const raw: ExecResult = await host.process.exec(
      options.cmd,
      options.args ?? [],
      execOpts,
    );

    // Map ExecResult → ProcessResult
    // stdout/stderr are included iff the caller requested "piped" capture.
    // When mergeOutput is active, the host sets the source stream's field to ""
    // (empty) — the gating below correctly surfaces that empty string.
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

  // ── pandoc ─────────────────────────────────────────────────────────────────
  // Routes through host.process.exec at global.pandocPath. When pandocPath is
  // absent (null/undefined), rejects with a distinct "pandoc unavailable" error
  // — q2 requires ambient pandoc resolved in Rust; if Rust couldn't find it, TS
  // won't do better. `async` ensures callers using `.catch()` see the error
  // even on the None path.
  // accepted-untested (Plan 4b-D): pandoc()'s happy path has no in-scope
  // consumer (Plan 2 "no consumer"). The `behave` QUARTO_PANDOC stretch is
  // Phase F/optional, not this task — no test required here.
  async function pandoc(args: string[], stdin?: string): Promise<ProcessResult> {
    if (global.pandocPath == null) {
      throw new Error(
        `@quarto/api: pandoc unavailable (no pandoc path was provided by the host)`,
      );
    }
    const raw = await host.process.exec(global.pandocPath, args, { stdin });
    return { success: raw.success, code: raw.code, stdout: raw.stdout, stderr: raw.stderr };
  }

  // ── checkRender (STUB) ─────────────────────────────────────────────────────
  // STAYS `async` — throws as a rejected promise so `.catch()`-style callers are
  // protected (the §2aa stub contract). A throwing body satisfies any declared
  // return, so the real SDK signature is honored while it remains a stub. The
  // real body (renders a check doc via PlatformHost) lands in a later plan.
  async function checkRender(
    _options: CheckRenderOptions,
  ): Promise<CheckRenderResult> {
    throw notYetImplementedError("checkRender");
  }

  // ── runExternalPreviewServer (STUB) ────────────────────────────────────────
  // SYNCHRONOUS throwing stub. Q1 returns `PreviewServer` synchronously; the
  // Plan-1b async-ization was itself a Q1 divergence that B2 removes — re-adding
  // `async` would re-break conformance under the derive. A throwing body
  // satisfies the declared `PreviewServer` return while it remains a stub.
  function runExternalPreviewServer(_options: {
    cmd: string[];
    readyPattern: RegExp;
    env?: Record<string, string>;
    cwd?: string;
  }): PreviewServer {
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
