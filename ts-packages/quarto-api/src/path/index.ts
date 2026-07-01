/**
 * @quarto/api — path namespace (MIXED: pure exports + host-only factory)
 *
 * Ported from Q1:
 *   - toForwardSlashes (pure): core/path.ts:199 (pathWithForwardSlashes)
 *   - dirAndStem (pure): core/path.ts:88
 *   - isQmdFile (pure): core/path.ts:95
 *   - inputFilesDir (pure): core/render.ts:13
 *   - absolute (host-only, via factory): core/path.ts:319 (normalizePath)
 *   - runtime, resource, dataDir (config-derived, via factory with HostGlobalConfig)
 *
 * No Deno.* / node:* used anywhere — pure ops use JS string/path logic,
 * and host-only ops go through the injected PlatformHost and HostGlobalConfig.
 *
 * Usage:
 *   // Pure exports — import and call directly:
 *   import { toForwardSlashes, dirAndStem, isQmdFile, inputFilesDir } from "@quarto/api/path";
 *
 *   // Host-dependent — build with makePathHost:
 *   import { makePathHost } from "@quarto/api/path";
 *   const pathHost = makePathHost(host, global);
 *   pathHost.absolute("./relative/path");  // → resolved via host.cwd()
 *   pathHost.runtime("subdir");            // → global.runtimeDir + "/subdir", dir created
 *   pathHost.resource("a", "b");           // → global.resourceDir + "/a/b" (no dir creation)
 *   pathHost.dataDir("subdir");            // → global.dataDir + "/subdir", dir created
 */

import type { HostGlobalConfig, PlatformHost } from "../platform/index.js";
import type { QuartoAPI } from "@quarto/types";

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

// ─── Host-only subset (derived from the SDK contract) ─────────────────────────

/**
 * The host-dependent path methods returned by `makePathHost`.
 *
 * Mostly-pure namespace: the factory returns only the HOST SUBSET
 * (`absolute`/`runtime`/`resource`/`dataDir`); the pure functions
 * (`toForwardSlashes`/`dirAndStem`/`isQmdFile`/`inputFilesDir`) are direct
 * exports mixed in by `buildQuartoAPI`. So we derive the SUBSET via `Pick`
 * (Plan 2 B2, Fix B) — deriving the whole `QuartoAPI["path"]` here would be a
 * category error (it would force the factory to also return the pure functions).
 * `buildQuartoAPI` enforces the full shape with `... satisfies QuartoAPI["path"]`.
 *
 * Behavioural notes (unchanged from the prior hand-written interface):
 *   - `absolute`: mirrors Q1 `core/path.ts:319` (`normalizePath`) — absolute
 *     paths pass through (separators normalised); relative paths join `cwd()`;
 *     Windows drive letters are uppercased.
 *   - `runtime`/`dataDir`: create the dir (recursively) before returning;
 *     `ensureDir` errors propagate. `resource`: does NOT create (read-only).
 *   - `dataDir`'s `roaming` is a no-op — `global` ships one resolved dir.
 */
export type PathHostNamespace = Pick<
  QuartoAPI["path"],
  "absolute" | "runtime" | "resource" | "dataDir"
>;

/**
 * Build the host-dependent path methods.
 *
 * @param host   - A PlatformHost (or minimal fake) with `cwd()` and `fs`.
 * @param global - Process-stable host config (resource/runtime/data dirs, pandoc path).
 */
export function makePathHost(
  host: Pick<PlatformHost, "cwd" | "fs">,
  global: HostGlobalConfig,
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

  // ── runtime ────────────────────────────────────────────────────────────────
  // Resolves from global.runtimeDir, creates the dir (mirrors Q1 quartoDir).
  // Errors from ensureDir propagate — no try/catch (Q1: throw = "permission issue").
  function runtime(subdir?: string): string {
    const p = pathJoin(global.runtimeDir, subdir);
    host.fs.ensureDir(p);
    return p;
  }

  // ── resource ───────────────────────────────────────────────────────────────
  // Resolves from global.resourceDir, does NOT create (resources are read-only;
  // creating one would mask a missing-resource bug — Q1 parity).
  function resource(...parts: string[]): string {
    return pathJoin(global.resourceDir, ...parts);
  }

  // ── dataDir ────────────────────────────────────────────────────────────────
  // Resolves from global.dataDir, creates the dir (mirrors Q1 quartoDir).
  // `roaming` is a no-op: global ships one resolved dataDir (#6 Q1-source compat).
  function dataDir(subdir?: string, _roaming?: boolean): string {
    const p = pathJoin(global.dataDir, subdir);
    host.fs.ensureDir(p);
    return p;
  }

  return { absolute, runtime, resource, dataDir };
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/**
 * Join non-empty path parts with "/" (portability: no node:path or @std/path).
 * Filters out undefined and empty-string parts before joining.
 */
function pathJoin(...parts: (string | undefined)[]): string {
  return parts.filter((p): p is string => p !== undefined && p !== "").join("/");
}

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
