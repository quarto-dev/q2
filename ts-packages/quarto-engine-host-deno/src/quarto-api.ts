/**
 * buildQuartoAPI — assemble the QuartoAPI object engines receive via init(quarto).
 *
 * Each of the nine namespaces is built from the corresponding @quarto/api subpath.
 * Two categories of factories exist (see @quarto/api/index.ts for the convention):
 *   - Fully-host namespaces: makeConsole(host), makeSystem(host).
 *   - Mostly-pure namespaces: makePathHost(host) + pure direct exports,
 *     makeMappedStringHost(host) + pure direct exports.
 *
 * CAST NOTE: The vendored QuartoAPI type (@quarto/types/quarto-api.ts) diverges
 * from the impl signatures in a few spots. We use `as unknown as QuartoAPI` at
 * the return to handle these without per-field casting. Divergences are:
 *   1. system.execProcess — vendored type has 6 params (mergeOutput, stderrFilter,
 *      respectStreams, timeout); the impl stub has 2 (options, stdin?). TypeScript
 *      accepts fewer-param functions in general, but makeSystem's return type is
 *      SystemNamespace not QuartoAPI['system'], so the cast is needed anyway.
 *   2. system.runExternalPreviewServer — vendored type returns PreviewServer
 *      (synchronously); the impl stub returns Promise<unknown> (and throws).
 *   3. text.postProcessRestorePreservedHtml — in the vendored type but NOT exported
 *      by @quarto/api/text (deferred, does file I/O). We add a Plan-2 stub here.
 *   4. Namespace return types differ from QuartoAPI's named fields (e.g.
 *      ConsoleNamespace vs QuartoAPI['console']), so a structural match at the
 *      assembled-object level requires the cast.
 *
 * GLOBAL NOTE: `global` is accepted in the signature for forward-compatibility
 * (Plan 2 will thread it into path.runtime, path.resource, path.dataDir, and
 * system.pandoc). The factories don't accept it yet — Plan 2 work. A comment
 * marks each future injection site.
 */

import type { QuartoAPI } from "@quarto/types";
import type { PlatformHost } from "@quarto/api/platform";
import type { HostGlobalConfig } from "./types.js";

// ── Pure text imports ─────────────────────────────────────────────────────────
import {
  lines,
  trimEmptyLines,
  lineColToIndex,
  executeInlineCodeHandler,
  asYamlText,
} from "@quarto/api/text";

// ── Pure markdownRegex imports ────────────────────────────────────────────────
import {
  extractYaml,
  partition,
  getLanguages,
  getLanguagesWithClasses,
  breakQuartoMd,
} from "@quarto/api/markdownRegex";

// ── Pure crypto import ────────────────────────────────────────────────────────
import { md5Hash } from "@quarto/api/crypto";

// ── Pure format imports ───────────────────────────────────────────────────────
import {
  isHtmlCompatible,
  isIpynbOutput,
  isLatexOutput,
  isMarkdownOutput,
  isPresentationOutput,
  isHtmlDashboardOutput,
  isServerShiny,
  isServerShinyPython,
} from "@quarto/api/format";

// ── Host-only console factory ─────────────────────────────────────────────────
import { makeConsole } from "@quarto/api/console";

// ── Mixed path: factory + pure exports ───────────────────────────────────────
import {
  makePathHost,
  toForwardSlashes,
  dirAndStem,
  isQmdFile,
  inputFilesDir,
} from "@quarto/api/path";

// ── Host-only system factory ──────────────────────────────────────────────────
import { makeSystem } from "@quarto/api/system";

// ── Mixed mappedString: factory + pure exports ────────────────────────────────
import {
  makeMappedStringHost,
  fromString,
  normalizeNewlines,
  splitLines,
  indexToLineCol,
  mappedStringFromChunks,
} from "@quarto/api/mappedString";

// ── Local Plan-2 stub helper ──────────────────────────────────────────────────
//
// Not exported by @quarto/api (it's an internal detail of each namespace module).
// We define our own for the two stubs that belong to THIS assembler:
//   - text.postProcessRestorePreservedHtml (in the vendored type, not in the impl)
//   - jupyter namespace (Plan 3 — entire namespace deferred)
//
function notYetImplementedError(method: string, plan: "Plan 2" | "Plan 3"): Error {
  return new Error(
    `@quarto/engine-host-deno: ${method}() is not yet implemented (${plan})`,
  );
}

/**
 * Build the QuartoAPI object that engines receive via their `init(quarto)` hook.
 *
 * @param global  Process-stable host config. Accepted for forward-compatibility
 *                (Plan 2 threads it into path.runtime / system.pandoc). Currently
 *                UNCONSUMED by any factory — do not thread it until Plan 2.
 * @param host    The PlatformHost that backs all I/O-touching namespaces.
 * @returns       A fully assembled QuartoAPI record (stable object identity for
 *                the lifetime of the harness — no registry, no lazy init).
 */
export function buildQuartoAPI(
  // global is currently unconsumed by factories; Plan 2 threads it into
  // path.runtime, path.resource, path.dataDir, and system.pandoc.
  _global: HostGlobalConfig,
  host: PlatformHost,
): QuartoAPI {
  // ── text namespace ──────────────────────────────────────────────────────────
  //
  // All five impl exports are real + callable. `postProcessRestorePreservedHtml`
  // is in the vendored QuartoAPI type but NOT in @quarto/api/text (it does file
  // I/O — deferred to Plan 2). We provide a Plan-2 stub here.
  const text = {
    lines,
    trimEmptyLines,
    lineColToIndex,
    executeInlineCodeHandler,
    asYamlText,
    // Divergence #3: postProcessRestorePreservedHtml is in the vendored type but
    // not exported by @quarto/api/text. Plan-2 stub.
    postProcessRestorePreservedHtml: (..._args: unknown[]): void => {
      throw notYetImplementedError(
        "text.postProcessRestorePreservedHtml",
        "Plan 2",
      );
    },
  };

  // ── markdownRegex namespace ─────────────────────────────────────────────────
  const markdownRegex = {
    extractYaml,
    partition,
    getLanguages,
    getLanguagesWithClasses,
    breakQuartoMd,
  };

  // ── crypto namespace ────────────────────────────────────────────────────────
  const crypto = { md5Hash };

  // ── format namespace ────────────────────────────────────────────────────────
  const format = {
    isHtmlCompatible,
    isIpynbOutput,
    isLatexOutput,
    isMarkdownOutput,
    isPresentationOutput,
    isHtmlDashboardOutput,
    isServerShiny,
    isServerShinyPython,
  };

  // ── console namespace ───────────────────────────────────────────────────────
  const consoleNs = makeConsole(host);

  // ── path namespace ──────────────────────────────────────────────────────────
  //
  // makePathHost provides absolute() (working) plus runtime/resource/dataDir
  // (Plan-2 stubs that throw). The pure exports are mixed in directly.
  // Plan 2: pass _global to makePathHost once the factory accepts it so that
  // resourceDir/runtimeDir/dataDir from HostGlobalConfig can back the stubs.
  const pathNs = {
    ...makePathHost(host),
    toForwardSlashes,
    dirAndStem,
    isQmdFile,
    inputFilesDir,
  };

  // ── system namespace ────────────────────────────────────────────────────────
  //
  // makeSystem provides execProcess, isInteractiveSession, runningInCI,
  // tempContext, onCleanup (working) + pandoc/checkRender/runExternalPreviewServer
  // (Plan-2 stubs that throw/reject).
  // Plan 2: pass _global to makeSystem once the factory accepts it so that
  // pandocPath from HostGlobalConfig can back system.pandoc().
  const systemNs = makeSystem(host);

  // ── mappedString namespace ──────────────────────────────────────────────────
  //
  // makeMappedStringHost provides fromFile (host-reads the file).
  // The pure exports are mixed in directly.
  const mappedStringNs = {
    ...makeMappedStringHost(host),
    fromString,
    normalizeNewlines,
    splitLines,
    indexToLineCol,
    mappedStringFromChunks,
  };

  // ── jupyter namespace (Plan 3 stub) ─────────────────────────────────────────
  //
  // No jupyter/ implementation exists in @quarto/api yet (Plan 3). We expose
  // a Proxy whose `get` trap returns a throwing function for every property
  // access, so engines that call any jupyter method receive a clear error.
  // Cast: `as unknown as QuartoAPI['jupyter']` — the Proxy's runtime shape is
  // intentionally opaque; the type cast is safe because all calls throw.
  const jupyterStub = new Proxy(
    {},
    {
      get(_target, prop) {
        return (..._args: unknown[]): never => {
          throw notYetImplementedError(`jupyter.${String(prop)}`, "Plan 3");
        };
      },
    },
  ) as unknown as QuartoAPI["jupyter"];

  // ── Assemble and return ─────────────────────────────────────────────────────
  //
  // Cast: `as unknown as QuartoAPI` — required because several namespace types
  // differ between the vendored QuartoAPI and the impl's own interfaces:
  //   1. system.execProcess arity (see file header)
  //   2. system.runExternalPreviewServer return type (see file header)
  //   3. text.postProcessRestorePreservedHtml (see file header)
  //   4. Namespace interface names (ConsoleNamespace, SystemNamespace, etc.) are
  //      structurally compatible but declared separately from QuartoAPI's inline types.
  return {
    text,
    markdownRegex,
    crypto,
    format,
    console: consoleNs,
    path: pathNs,
    system: systemNs,
    mappedString: mappedStringNs,
    jupyter: jupyterStub,
  } as unknown as QuartoAPI;
}
