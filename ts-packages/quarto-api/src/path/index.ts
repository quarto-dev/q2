/**
 * @quarto/api — path namespace (MIXED: pure exports + host-only factory)
 *
 * Ported from Q1:
 *   - toForwardSlashes (pure): core/path.ts:199 (pathWithForwardSlashes)
 *   - dirAndStem (pure): core/path.ts:88
 *   - isQmdFile (pure): core/path.ts:95
 *   - inputFilesDir (pure): core/render.ts:13
 *   - absolute (host-only, via factory): core/path.ts:319 (normalizePath)
 *   - runtime, resource, dataDir (stubs, throw "not yet implemented (Plan 2)")
 *
 * No Deno.* / node:* used anywhere — pure ops use JS string/path logic,
 * and host-only ops go through the injected PlatformHost.cwd().
 *
 * Usage:
 *   // Pure exports — import and call directly:
 *   import { toForwardSlashes, dirAndStem, isQmdFile, inputFilesDir } from "@quarto/api/path";
 *
 *   // Host-dependent — build with makePathHost:
 *   import { makePathHost } from "@quarto/api/path";
 *   const pathHost = makePathHost(host);
 *   pathHost.absolute("./relative/path");  // → resolved via host.cwd()
 *   pathHost.runtime();                    // → throws "not yet implemented (Plan 2)"
 */

import type { PlatformHost } from "../platform/index.js";

// ─── Stub error helper (§2aa stub contract) ───────────────────────────────────

function notYetImplementedError(method: string): Error {
  return new Error(
    `@quarto/api: path.${method}() is not yet implemented (Plan 2)`,
  );
}

// ─── Pure direct exports ──────────────────────────────────────────────────────

/**
 * Replace all backslashes with forward slashes.
 * Mirrors Q1 `core/path.ts:199` (`pathWithForwardSlashes`).
 */
export function toForwardSlashes(path: string): string {
  return path.replace(/\\/g, "/");
}

/**
 * Split a file path into `[dir, stem]` where stem is the base name without
 * extension.  Mirrors Q1 `core/path.ts:88` (`dirAndStem`).
 *
 * Uses only string operations compatible with POSIX and Windows paths.
 */
export function dirAndStem(file: string): [string, string] {
  // dirname: everything up to the last separator
  const lastSep = Math.max(file.lastIndexOf("/"), file.lastIndexOf("\\"));
  const dir = lastSep >= 0 ? file.slice(0, lastSep) : ".";

  // basename
  const base = lastSep >= 0 ? file.slice(lastSep + 1) : file;

  // stem: remove extension (last "." portion)
  const dotIdx = base.lastIndexOf(".");
  const stem = dotIdx > 0 ? base.slice(0, dotIdx) : base;

  return [dir, stem];
}

/**
 * Return true iff `file` has a `.qmd` extension (case-insensitive).
 * Mirrors Q1 `core/path.ts:95` (`isQmdFile`).
 */
export function isQmdFile(file: string): boolean {
  const dotIdx = file.lastIndexOf(".");
  if (dotIdx < 0) return false;
  const ext = file.slice(dotIdx).toLowerCase();
  return ext === ".qmd";
}

/**
 * Return the `_files` directory name for a given input file.
 * Mirrors Q1 `core/render.ts:13` (`inputFilesDir`).
 *
 * @example
 * inputFilesDir("report.qmd") // → "report_files"
 */
export function inputFilesDir(input: string): string {
  const [, stem] = dirAndStem(input);
  return stem + "_files";
}

// ─── Host-only interface ──────────────────────────────────────────────────────

/** The host-dependent path methods returned by makePathHost. */
export interface PathHostNamespace {
  /**
   * Resolve a (possibly relative) path to an absolute path using `host.cwd()`.
   * Mirrors Q1 `core/path.ts:319` (`normalizePath`): if path is already
   * absolute it is returned as-is (after normalizing separators); otherwise it
   * is joined with cwd. Windows drive-letter normalisation (uppercase) is also
   * applied to match Q1.
   */
  absolute(path: string | URL): string;

  /**
   * Return the platform-specific runtime directory for quarto.
   * STUB — throws until the engine host is initialized at launchEngine.
   */
  runtime(subdir?: string): string;

  /**
   * Return a resource path within quarto's share directory.
   * STUB — throws until the engine host is initialized at launchEngine.
   */
  resource(...parts: string[]): string;

  /**
   * Return the platform-specific data directory for quarto.
   * STUB — throws until the engine host is initialized at launchEngine.
   */
  dataDir(subdir?: string, roaming?: boolean): string;
}

/**
 * Build the host-dependent path methods.
 *
 * @param host - A PlatformHost (or minimal fake) with a `cwd()` method.
 */
export function makePathHost(
  host: Pick<PlatformHost, "cwd">,
): PathHostNamespace {
  function absolute(path: string | URL): string {
    // Handle URL objects (mirrors Q1's fromFileUrl branch)
    let file: string;
    if (path instanceof URL) {
      // Convert file:// URL to local path
      file = path.pathname;
      // On Windows, strip leading slash from /C:/... paths
      if (/^\/[A-Za-z]:\//.test(file)) {
        file = file.slice(1);
      }
    } else {
      file = path;
    }

    // If not absolute, join with cwd
    const isAbs =
      file.startsWith("/") || // POSIX
      /^[A-Za-z]:[\\/]/.test(file) || // Windows drive
      file.startsWith("\\\\"); // Windows UNC

    if (!isAbs) {
      const cwd = host.cwd();
      file = cwd.endsWith("/") || cwd.endsWith("\\")
        ? cwd + file
        : cwd + "/" + file;
    }

    // Normalize separators and redundant segments
    // Normalise to forward slashes then resolve . and ..
    file = normalizeSegments(file);

    // Uppercase Windows drive letter (Q1 compatibility)
    file = file.replace(/^[a-z]:([\\/])/, (m, sep) => m[0].toUpperCase() + ":" + sep);

    return file;
  }

  function runtime(_subdir?: string): string {
    throw notYetImplementedError("runtime");
  }

  function resource(..._parts: string[]): string {
    throw notYetImplementedError("resource");
  }

  function dataDir(_subdir?: string, _roaming?: boolean): string {
    throw notYetImplementedError("dataDir");
  }

  return { absolute, runtime, resource, dataDir };
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/**
 * Normalize path segments: resolve `.` and `..`, collapse duplicate
 * separators, and return the canonical form (forward slashes on POSIX,
 * with a trailing slash only for the root).
 */
function normalizeSegments(path: string): string {
  // Detect and preserve leading UNC or drive prefix
  let prefix = "";
  let rest = path;

  if (/^[A-Za-z]:/.test(path)) {
    // Windows drive: "C:/"
    prefix = path.slice(0, 2);
    rest = path.slice(2);
  } else if (path.startsWith("\\\\")) {
    // UNC — keep as-is; normalise below
    prefix = "\\\\";
    rest = path.slice(2);
  }

  // Normalise separators to "/"
  rest = rest.replace(/\\/g, "/");

  // Determine leading slash(es)
  const leading = rest.startsWith("/") ? "/" : "";

  const parts = rest.split("/").filter((p) => p !== "" && p !== ".");
  const resolved: string[] = [];
  for (const part of parts) {
    if (part === "..") {
      resolved.pop();
    } else {
      resolved.push(part);
    }
  }

  return prefix + leading + resolved.join("/");
}
