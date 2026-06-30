/**
 * @quarto/engine-host-deno — Deno-native PlatformHost implementation.
 *
 * This module is DENO-ONLY: it imports from `jsr:@std/fs` and calls
 * `Deno.*` APIs directly. It is intentionally excluded from the
 * tsconfig.json compile graph and the vitest test runner.
 *
 * Typecheck with:  deno check ts-packages/quarto-engine-host-deno/src/deno-host.ts
 * Test with:       deno test --allow-all ts-packages/quarto-engine-host-deno/src/deno-host.deno-test.ts
 *
 * The `import type` below is erased at runtime; the Deno runtime
 * never resolves `@quarto/api/platform` — only `jsr:@std/fs` and
 * the built-in `Deno.*` namespace are needed at runtime.
 */
import type { PlatformHost } from "@quarto/api/platform";
import { walkSync } from "jsr:@std/fs";

const enc = new TextEncoder();
const dec = new TextDecoder();

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
    exec: async (cmd, args, opts) => {
      if (opts?.stdin !== undefined) {
        // Piped-stdin path: use spawn() so we can write to stdin before
        // collecting output. We know stdin is WritableStream<Uint8Array>
        // because we set stdin: "piped" — the non-null assertion is safe.
        const command = new Deno.Command(cmd, {
          args,
          cwd: opts.cwd,
          env: opts.env,
          stdin: "piped",
          stdout: "piped",
          stderr: "piped",
        });
        const child = command.spawn();
        const writer = child.stdin!.getWriter();
        await writer.write(enc.encode(opts.stdin));
        await writer.close();
        const output = await child.output();
        return {
          code: output.code,
          success: output.success,
          stdout: dec.decode(output.stdout),
          stderr: dec.decode(output.stderr),
        };
      } else {
        const command = new Deno.Command(cmd, {
          args,
          cwd: opts?.cwd,
          env: opts?.env,
          stdout: "piped",
          stderr: "piped",
        });
        const { code, success, stdout, stderr } = await command.output();
        return {
          code,
          success,
          stdout: dec.decode(stdout),
          stderr: dec.decode(stderr),
        };
      }
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
