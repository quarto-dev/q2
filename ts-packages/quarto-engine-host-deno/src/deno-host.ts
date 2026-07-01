/**
 * @quarto/engine-host-deno — Deno-native PlatformHost implementation.
 *
 * This module is DENO-ONLY: it imports from `jsr:@std/fs`, `jsr:@std/async`,
 * and calls `Deno.*` APIs directly. It is intentionally excluded from the
 * tsconfig.json compile graph and the vitest test runner.
 *
 * Typecheck with:  deno check ts-packages/quarto-engine-host-deno/src/deno-host.ts
 * Test with:       deno test --allow-all ts-packages/quarto-engine-host-deno/src/deno-host.deno-test.ts
 *
 * The `import type` below is erased at runtime; the Deno runtime
 * never resolves `@quarto/api/platform` — only `jsr:@std/fs`, `jsr:@std/async`,
 * and the built-in `Deno.*` namespace are needed at runtime.
 */
import type { ExecOptions, PlatformHost } from "@quarto/api/platform";
import { walkSync } from "jsr:@std/fs";
import { MuxAsyncIterator } from "jsr:@std/async";

const enc = new TextEncoder();
const dec = new TextDecoder();

// ─── exec helpers (Q1 port: core/process.ts:224-259) ──────────────────────

/**
 * Accumulate an async iterable of byte chunks into a string, optionally
 * writing each chunk through to a synchronous Deno stream (for respectStreams).
 * Port of Q1 `processOutput` (process.ts:238-259).
 */
async function processOutput(
  iterator: AsyncIterable<Uint8Array>,
  writeThrough?: { writeSync(p: Uint8Array): number },
): Promise<string> {
  let text = "";
  for await (const chunk of iterator) {
    if (writeThrough) {
      writeThrough.writeSync(chunk);
    }
    text += dec.decode(chunk);
  }
  return text;
}

/**
 * Wrap an async iterable of byte chunks, mapping each through a string filter.
 * Port of Q1 `filteredAsyncIterator` (process.ts:224-235).
 */
async function* filteredAsyncIterator(
  iterator: AsyncIterable<Uint8Array>,
  filter: (output: string) => string,
): AsyncGenerator<Uint8Array> {
  for await (const chunk of iterator) {
    yield enc.encode(filter(dec.decode(chunk)));
  }
}

/** Write a diagnostic message to stderr (never stdout — stdout is the protocol channel). */
function writeStderr(msg: string): void {
  Deno.stderr.writeSync(enc.encode(msg + "\n"));
}

/**
 * `denoHost` — the q2-original `PlatformHost` backed by `Deno.*` APIs.
 *
 * Used by `@quarto/api` factories in the real CLI context. All I/O is
 * routed through this object so namespaces stay platform-neutral and
 * testable with fakes in vitest.
 */
export const denoHost: PlatformHost = {
  // ── File-system operations ──────────────────────────────────────────

  fs: {
    readTextFileSync: (path) => Deno.readTextFileSync(path),

    writeFileSync: (path, content) =>
      Deno.writeFileSync(
        path,
        typeof content === "string" ? enc.encode(content) : content,
      ),

    exists: (path) => {
      try {
        Deno.statSync(path);
        return true;
      } catch {
        return false;
      }
    },

    ensureDir: (path) => Deno.mkdirSync(path, { recursive: true }),

    makeTempDir: (opts) =>
      Deno.makeTempDirSync({ prefix: opts?.prefix, dir: opts?.dir }),

    makeTempFile: (opts) =>
      Deno.makeTempFileSync({
        prefix: opts?.prefix,
        suffix: opts?.suffix,
        dir: opts?.dir,
      }),

    remove: (path, opts) =>
      Deno.removeSync(path, { recursive: opts?.recursive ?? false }),

    /**
     * Walk a directory tree synchronously.
     *
     * Defaults: `includeDirs: false` (files only). Paths in returned
     * entries are absolute when `root` is absolute (which `makeTempDir`
     * always produces).
     *
     * Delegates to `walkSync` from `jsr:@std/fs`; the root entry itself
     * is never emitted when `includeDirs` is false (the library's
     * default for includeDirs is true, but we override it to false).
     */
    walk: (root, opts) =>
      [
        ...walkSync(root, {
          maxDepth: opts?.maxDepth,
          includeDirs: opts?.includeDirs ?? false,
        }),
      ].map((e) => ({
        path: e.path,
        isFile: e.isFile,
        isDirectory: e.isDirectory,
      })),
  },

  // ── Process / subprocess operations ────────────────────────────────

  process: {
    /**
     * Spawn an external process and return its captured output.
     *
     * Port of Q1 `execProcess` (core/process.ts:46-215) — spawn + per-stream
     * processing model with four knobs: mergeOutput, stderrFilter,
     * respectStreams, timeout.  ExecOptions carries all knobs from the
     * marshalling layer (makeSystem) so this host implementation stays thin.
     */
    exec: async (cmd, args, opts?: ExecOptions) => {
      // Always spawn with piped streams; we implement inherit/null behaviour
      // ourselves (via respectStreams write-through) so we can capture text.
      const command = new Deno.Command(cmd, {
        args,
        cwd: opts?.cwd,
        env: opts?.env,
        stdin: opts?.stdin !== undefined ? "piped" : "null",
        stdout: "piped",
        stderr: "piped",
      });
      const child = command.spawn();

      // Write stdin if provided (safe to access child.stdin: mode is "piped").
      if (opts?.stdin !== undefined) {
        const writer = child.stdin!.getWriter();
        await writer.write(enc.encode(opts.stdin));
        await writer.close();
      }

      const mergeOutput = opts?.mergeOutput;
      const stderrFilter = opts?.stderrFilter;
      const respectStreams = opts?.respectStreams;
      const timeout = opts?.timeout;

      // Build a timeout-and-kill wrapper.
      // Port of Q1 `withTimeout` (process.ts:54-63).
      function withTimeout<T>(promise: Promise<T>): Promise<T> {
        if (!timeout) return promise;
        let timerId: ReturnType<typeof setTimeout>;
        return new Promise<T>((resolve, reject) => {
          timerId = setTimeout(() => {
            try {
              child.kill();
            } catch {
              /* child may already be dead */
            }
            reject(new Error("Process timed out"));
          }, timeout);
          promise.then(
            (v) => {
              clearTimeout(timerId);
              resolve(v);
            },
            (e: unknown) => {
              clearTimeout(timerId);
              reject(e);
            },
          );
        });
      }

      let stdoutText = "";
      let stderrText = "";

      if (mergeOutput) {
        // Merge both stdout and stderr into one multiplexed stream.
        // Port of Q1 merge branch (process.ts:115-157).
        const mux = new MuxAsyncIterator<Uint8Array>();
        mux.add(child.stdout);
        const stderrIter: AsyncIterable<Uint8Array> = stderrFilter
          ? filteredAsyncIterator(child.stderr, stderrFilter)
          : child.stderr;
        mux.add(stderrIter);

        const allOutput = await withTimeout(processOutput(mux));

        if (mergeOutput === "stderr>stdout") {
          stdoutText = allOutput; // all output → stdout field
        } else {
          stderrText = allOutput; // "stdout>stderr" → all output → stderr field
        }
      } else {
        // Process stdout and stderr independently (parallel).
        // Port of Q1 independent-streams branch (process.ts:159-191).
        const stderrIter: AsyncIterable<Uint8Array> = stderrFilter
          ? filteredAsyncIterator(child.stderr, stderrFilter)
          : child.stderr;

        [stdoutText, stderrText] = await withTimeout(Promise.all([
          processOutput(child.stdout, respectStreams ? Deno.stdout : undefined),
          processOutput(stderrIter, respectStreams ? Deno.stderr : undefined),
        ]));
      }

      // Await exit status — streams are already fully consumed above.
      const status = await withTimeout(child.status);

      return {
        code: status.code,
        success: status.success,
        stdout: stdoutText,
        stderr: stderrText,
      };
    },

    /** Register a handler to call when the process is about to exit.
     *  Uses the web-standard `unload` event that Deno fires on exit. */
    onExit: (handler) =>
      globalThis.addEventListener("unload", () => handler()),

    exit: (code) => Deno.exit(code),
  },

  // ── Environment ─────────────────────────────────────────────────────

  env: {
    get: (key) => Deno.env.get(key),
  },

  // ── Logging (stderr only — stdout is the protocol channel) ─────────

  log: {
    info: (msg) => writeStderr("[INFO] " + msg),
    warning: (msg) => writeStderr("[WARN] " + msg),
    error: (msg) => writeStderr("[ERROR] " + msg),
    clearLine: () => {}, // no-op; terminal line clearing is UI-layer concern
  },

  // ── Process identity ────────────────────────────────────────────────

  cwd: () => Deno.cwd(),
  realPath: (path) => Deno.realPathSync(path),

  // Pre-computed at module load time (avoids repeated tty/env probing).
  isInteractive: Deno.stdin.isTerminal(),
  isCI: !!Deno.env.get("CI"),
};
