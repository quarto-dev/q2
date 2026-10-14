/**
 * @quarto/engine-host-node — Node-native PlatformHost implementation.
 *
 * The `PlatformHost` over `node:*` built-ins only. It is the host
 * adapter the Node entry point (`node-main.ts`) hands to `runHost`; all
 * `@quarto/api` I/O goes through this object so namespaces stay platform
 * neutral and testable with fakes.
 *
 * Node specifics that this file absorbs:
 * - `child_process.spawn` replaces the child environment (it does not merge
 *   it the way Deno's `Command` did), so `exec` merges `process.env` explicitly.
 * - `mkdtemp` cannot add a suffix, so `makeTempFile` builds its own name
 *   (marimo needs `.html`).
 * - `process.stdin` is never touched here: reading its getter puts fd 0 into
 *   non-blocking mode, which breaks the synchronous token read in
 *   `node-control-transport.ts`.
 */
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import {
  closeSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import { isatty } from "node:tty";
import type { ExecOptions, PlatformHost } from "@quarto/api/platform";

type WalkEntry = { path: string; isFile: boolean; isDirectory: boolean };

/** Walk a directory tree synchronously, pre-order, like `walkSync` in `@std/fs`. */
function walk(
  root: string,
  opts?: { maxDepth?: number; includeDirs?: boolean },
): WalkEntry[] {
  const maxDepth = opts?.maxDepth ?? Infinity;
  const includeDirs = opts?.includeDirs ?? false;
  const entries: WalkEntry[] = [];
  if (includeDirs) {
    entries.push({ path: root, isFile: false, isDirectory: true });
  }
  const visit = (dir: string, depth: number): void => {
    if (depth > maxDepth) return;
    for (const dirent of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, dirent.name);
      const isDirectory = dirent.isDirectory();
      if (!isDirectory || includeDirs) {
        entries.push({ path, isFile: dirent.isFile(), isDirectory });
      }
      if (isDirectory) visit(path, depth + 1);
    }
  };
  visit(root, 1);
  return entries;
}

/** Create an empty file named `<prefix><random><suffix>` and return its path. */
function makeTempFile(
  opts?: { prefix?: string; suffix?: string; dir?: string },
): string {
  const dir = opts?.dir ?? tmpdir();
  for (let attempt = 0; attempt < 16; attempt++) {
    const name = `${opts?.prefix ?? ""}${randomBytes(6).toString("hex")}${
      opts?.suffix ?? ""
    }`;
    const path = join(dir, name);
    try {
      closeSync(openSync(path, "wx", 0o600));
      return path;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;
    }
  }
  throw new Error(`could not create a temporary file in ${dir}`);
}

/**
 * Spawn an external process and return its captured output.
 *
 * Port of Q1 `execProcess` (core/process.ts:46-215): four knobs —
 * mergeOutput, stderrFilter, respectStreams, timeout.
 */
function exec(cmd: string, args: string[], opts?: ExecOptions) {
  return new Promise<{
    code: number;
    success: boolean;
    stdout: string;
    stderr: string;
  }>((resolve, reject) => {
    const child = spawn(cmd, args, {
      cwd: opts?.cwd,
      env: opts?.env ? { ...process.env, ...opts.env } : process.env,
      stdio: [opts?.stdin !== undefined ? "pipe" : "ignore", "pipe", "pipe"],
    });

    const stdoutDecoder = new TextDecoder();
    const stderrDecoder = new TextDecoder();
    const encoder = new TextEncoder();
    let stdoutText = "";
    let stderrText = "";
    let settled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const settle = (action: () => void): void => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      action();
    };

    // "stderr>stdout" sends every byte to the stdout field, "stdout>stderr"
    // to the stderr field.
    const stdoutTarget = opts?.mergeOutput === "stdout>stderr"
      ? "stderr"
      : "stdout";
    const stderrTarget = opts?.mergeOutput === "stderr>stdout"
      ? "stdout"
      : "stderr";
    const append = (target: "stdout" | "stderr", text: string): void => {
      if (target === "stdout") stdoutText += text;
      else stderrText += text;
    };

    child.stdout!.on("data", (chunk: Buffer) => {
      if (opts?.respectStreams) process.stdout.write(chunk);
      append(stdoutTarget, stdoutDecoder.decode(chunk, { stream: true }));
    });
    child.stderr!.on("data", (chunk: Buffer) => {
      let text = stderrDecoder.decode(chunk, { stream: true });
      if (opts?.stderrFilter) text = opts.stderrFilter(text);
      if (opts?.respectStreams) process.stderr.write(encoder.encode(text));
      append(stderrTarget, text);
    });

    child.on("error", (error) => settle(() => reject(error)));
    child.on("close", (code, signal) => {
      settle(() => {
        append(stdoutTarget, stdoutDecoder.decode());
        append(stderrTarget, stderrDecoder.decode());
        // A signal-terminated child has no exit code; report it as a failure.
        const exitCode = code ?? (signal ? 1 : 0);
        resolve({
          code: exitCode,
          success: exitCode === 0,
          stdout: stdoutText,
          stderr: stderrText,
        });
      });
    });

    if (opts?.timeout) {
      timer = setTimeout(() => {
        child.kill();
        settle(() => reject(new Error("Process timed out")));
      }, opts.timeout);
    }

    if (opts?.stdin !== undefined) {
      // A child that exits without reading stdin raises EPIPE; its exit
      // status already reports what went wrong.
      child.stdin!.on("error", () => {});
      child.stdin!.end(opts.stdin, "utf-8");
    }
  });
}

/** Write a diagnostic to stderr (stdout stays free for the child's own output). */
function writeStderr(msg: string): void {
  process.stderr.write(msg + "\n");
}

export const nodeHost: PlatformHost = {
  fs: {
    readTextFileSync: (path) => readFileSync(path, "utf-8"),
    writeFileSync: (path, content) => writeFileSync(path, content),
    exists: (path) => existsSync(path),
    ensureDir: (path) => {
      mkdirSync(path, { recursive: true });
    },
    // `mkdtemp` appends its random suffix to the *path string*, so the prefix
    // needs a real name: with none, `join(tmpdir(), "")` is `/tmp` and the
    // directory would land beside it as `/tmpXXXXXX` (EACCES on Linux).
    makeTempDir: (opts) =>
      mkdtempSync(join(opts?.dir ?? tmpdir(), opts?.prefix ?? "tmp-")),
    makeTempFile,
    remove: (path, opts) => {
      rmSync(path, { recursive: opts?.recursive ?? false });
    },
    walk,
  },

  process: {
    exec,
    // `exit` fires synchronously on `process.exit()` and on natural exit.
    onExit: (handler) => {
      process.on("exit", () => handler());
    },
    exit: (code) => process.exit(code),
  },

  env: {
    get: (key) => process.env[key],
  },

  log: {
    info: (msg) => writeStderr("[INFO] " + msg),
    warning: (msg) => writeStderr("[WARN] " + msg),
    error: (msg) => writeStderr("[ERROR] " + msg),
    clearLine: () => {},
  },

  cwd: () => process.cwd(),
  realPath: (path) => realpathSync(path),

  // `isatty(0)` probes the fd without creating `process.stdin` (see header).
  isInteractive: isatty(0),
  isCI: !!process.env.CI,
};
