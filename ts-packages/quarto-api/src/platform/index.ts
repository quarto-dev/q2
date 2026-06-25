/**
 * @quarto/api — platform seam (PlatformHost interface)
 *
 * q2-ORIGINAL ABSTRACTION — Q1 (quarto-cli) has no equivalent seam;
 * it calls Deno.* APIs directly at module scope. This interface is the
 * q2 replacement: every host-only namespace receives a PlatformHost at
 * construction time and routes all I/O through it. This keeps the
 * namespace logic platform-neutral and trivially testable with fakes.
 *
 * Design decisions:
 *   - Generic platform abstraction only — no quarto-specific paths
 *     (quartoSharePath, pandocBinaryPath) on this interface.  Those
 *     belong in the resource/system namespace stubs (§2aa plan decision #2).
 *   - ExecOptions.stdin is a string (piped content), not a Deno stream mode.
 *     PlatformHost is a higher-level seam than Deno.Command; callers pass
 *     text content, and the host impl decides how to pipe it.
 *   - ExecResult fields stdout/stderr are non-optional strings (empty string
 *     if the process produced no output) for simpler downstream handling.
 *   - process.onExit handler is `() => void` (synchronous). The port-map
 *     notes Q1's cleanup registry also supports async handlers; if async
 *     cleanup is needed in a later task, this signature can be widened then.
 */

/** Options for executing an external process. */
export interface ExecOptions {
  /** Working directory for the child process. */
  cwd?: string;
  /** Environment variables to set for the child process. */
  env?: Record<string, string>;
  /** Text to write to the child process's stdin. */
  stdin?: string;
}

/** Result returned after an external process completes. */
export interface ExecResult {
  /** Process exit code. */
  code: number;
  /** `true` iff `code === 0`. */
  success: boolean;
  /** Captured stdout (empty string if nothing was written). */
  stdout: string;
  /** Captured stderr (empty string if nothing was written). */
  stderr: string;
}

/**
 * PlatformHost — the single injection point for all host I/O.
 *
 * Implementations: a `denoHost` for the real CLI (routes to Deno.*),
 * a `nodeHost` for the Node/MCP context, and fake implementations
 * in tests. Every host-only namespace factory accepts a
 * `Pick<PlatformHost, ...>` of the subset it actually needs.
 */
export interface PlatformHost {
  /** File-system operations. */
  fs: {
    /** Read a file's entire contents as a UTF-8 string (synchronous). */
    readTextFileSync(path: string): string;
    /** Write text or raw bytes to a file (synchronous). */
    writeFileSync(path: string, content: string | Uint8Array): void;
    /** Return true iff the path exists (file or directory). */
    exists(path: string): boolean;
    /** Create a directory and all parent directories (synchronous). */
    ensureDir(path: string): void;
    /** Create a temporary directory and return its path (synchronous). */
    makeTempDir(opts?: { prefix?: string; dir?: string }): string;
    /** Create a temporary file and return its path (synchronous). */
    makeTempFile(opts?: { prefix?: string; suffix?: string; dir?: string }): string;
    /** Remove a file or directory (synchronous). */
    remove(path: string, opts?: { recursive?: boolean }): void;
  };

  /** Process / subprocess operations. */
  process: {
    /** Spawn an external process and return its captured output. */
    exec(cmd: string, args: string[], opts?: ExecOptions): Promise<ExecResult>;
    /** Register a handler to run when the host process is about to exit. */
    onExit(handler: () => void): void;
    /** Terminate the host process with the given exit code. */
    exit(code: number): never;
  };

  /**
   * Read environment variables.
   * Reserved seam: `env.get` is the interface point for the real bodies of
   * `path.runtime` and `path.dataDir`, which are deferred to Plan 2 Phase A.
   * Those bodies will use it to locate Quarto's share/data directories.
   * Host authors must implement it even though nothing calls it yet.
   */
  env: {
    get(key: string): string | undefined;
  };

  /** Logging (maps to the host's output channel, e.g. stderr). */
  log: {
    info(msg: string): void;
    warning(msg: string): void;
    error(msg: string): void;
    /** Clear the current terminal line (optional — implementations may no-op). */
    clearLine?(): void;
  };

  /** Return the current working directory (absolute path). */
  cwd(): string;

  /**
   * Resolve a path to its canonical absolute form (follows symlinks).
   * Equivalent to `Deno.realPathSync` / `fs.realpathSync`.
   *
   * Reserved seam: `realPath` is the interface point for the real body of
   * `path.runtime` (locating the Quarto binary's own directory), which is
   * deferred to Plan 2 Phase A. Host authors must implement it even though
   * nothing calls it yet.
   */
  realPath(path: string): string;

  /**
   * True iff the process is running in an interactive terminal session.
   * Pre-computed by the host at startup (avoids repeated tty probing).
   */
  isInteractive: boolean;

  /**
   * True iff the process is running inside a CI environment.
   * Pre-computed from the environment at startup.
   */
  isCI: boolean;
}
