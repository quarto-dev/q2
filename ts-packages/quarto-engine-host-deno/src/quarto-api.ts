/**
 * buildQuartoAPI — assemble the QuartoAPI object engines receive via init(quarto).
 *
 * Each of the nine namespaces is built from the corresponding @quarto/api subpath.
 * Two categories of factories exist (see @quarto/api/index.ts for the convention):
 *   - Fully-host namespaces: makeConsole(host), makeSystem(host, global).
 *   - Mostly-pure namespaces: makePathHost(host, global) + pure direct exports,
 *     makeMappedStringHost(host) + pure direct exports.
 *
 * CAST NOTE (Plan 3 Task 13): `buildQuartoAPI` is now CAST-FREE — ZERO casts,
 * no `as unknown as` escape hatch anywhere in this file. Every @quarto/api
 * namespace type DERIVES from the vendored QuartoAPI (@quarto/types/quarto-api.ts):
 * console/system/jupyter alias `QuartoAPI[ns]`, path/mappedString `Pick` the host
 * subset and are enforced with `satisfies` where they are mixed with pure exports.
 * The assembled object is therefore genuinely assignable to the `: QuartoAPI`
 * return, so a future mis-wired namespace is a compile error again. The prior
 * divergences were closed as follows:
 *   1. system.execProcess — six-positional (B1); stdin is Q1-faithful stream MODE
 *      sourced from @quarto/types (B2 finding M-B2-stdin resolved). No carve-out.
 *   2. system.runExternalPreviewServer — now a SYNCHRONOUS throwing stub returning
 *      PreviewServer (was async), matching Q1.
 *   3. text.postProcessRestorePreservedHtml — now a real-typed stub exported from
 *      @quarto/api/text; no local stub here.
 *   4. system.onCleanup — vendored type pruned to sync `() => void` (Q1-faithful).
 *
 * JUPYTER (Plan 3 Task 13): the jupyter namespace is now the REAL
 * `makeJupyter(host)` from @quarto/api/jupyter — no Proxy, no cast. It binds the
 * host into the 7 implemented methods + notebookExtensions; the 15 deferred
 * members are typed NotImplemented throwers inside @quarto/api/jupyter, not here.
 *
 * GLOBAL NOTE: `global` (Plan 2 Phase A landed) is threaded into the two factories
 * that need it: makePathHost(host, global) provides runtime/resource/dataDir, and
 * makeSystem(host, global) provides system.pandoc. The wire HostGlobalConfig
 * (7 fields from ./types.ts) is structurally compatible with @quarto/api's own
 * 4-field HostGlobalConfig (width subtyping — extra fields are ignored).
 */

import type { QuartoAPI } from "@quarto/types";
import type { PlatformHost } from "@quarto/api/platform";
import type { HostGlobalConfig } from "./types.js";

// ── text imports (all real-typed; postProcessRestorePreservedHtml is a stub) ──
import {
  lines,
  trimEmptyLines,
  lineColToIndex,
  executeInlineCodeHandler,
  asYamlText,
  postProcessRestorePreservedHtml,
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

// ── Host-bound jupyter factory (Plan 3 — real) ────────────────────────────────
import { makeJupyter } from "@quarto/api/jupyter";

/**
 * Build the QuartoAPI object that engines receive via their `init(quarto)` hook.
 *
 * @param global  Process-stable host config. Threaded into `makePathHost` and
 *                `makeSystem` (Plan 2 Phase A landed — it is consumed by both).
 * @param host    The PlatformHost that backs all I/O-touching namespaces.
 * @returns       A fully assembled QuartoAPI record (stable object identity for
 *                the lifetime of the harness — no registry, no lazy init).
 */
export function buildQuartoAPI(
  global: HostGlobalConfig,
  host: PlatformHost,
): QuartoAPI {
  // ── text namespace ──────────────────────────────────────────────────────────
  //
  // All six exports come straight from @quarto/api/text. Five are real + callable;
  // `postProcessRestorePreservedHtml` is a real-TYPED stub there (it does file I/O
  // via PlatformHost in its future body) — so no local stub is needed here.
  const text = {
    lines,
    trimEmptyLines,
    lineColToIndex,
    executeInlineCodeHandler,
    asYamlText,
    postProcessRestorePreservedHtml,
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
  // makePathHost provides absolute() plus runtime/resource/dataDir (now real,
  // Plan 2 Phase A landed: global is threaded in). The pure exports are mixed
  // in directly.
  const pathNs = {
    ...makePathHost(host, global),
    toForwardSlashes,
    dirAndStem,
    isQmdFile,
    inputFilesDir,
  } satisfies QuartoAPI["path"];

  // ── system namespace ────────────────────────────────────────────────────────
  //
  // makeSystem provides execProcess, isInteractiveSession, runningInCI,
  // tempContext, onCleanup (working) + pandoc (now real, Plan 2 Phase A landed:
  // global is threaded in) + checkRender/runExternalPreviewServer (still stubs).
  const systemNs = makeSystem(host, global);

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
  } satisfies QuartoAPI["mappedString"];

  // ── jupyter namespace (Plan 3 — real) ───────────────────────────────────────
  // makeJupyter binds the host into the 7 implemented methods + notebookExtensions;
  // the 15 deferred members are NotImplemented throwers inside @quarto/api/jupyter.
  const jupyterNs = makeJupyter(host);

  // ── Assemble and return ─────────────────────────────────────────────────────
  //
  // ZERO CASTS (Plan 3 Task 13 — the payoff). Every namespace, jupyter included,
  // now genuinely conforms to the vendored QuartoAPI (the @quarto/api namespace
  // types derive from it), so the plain object is assignable to the `: QuartoAPI`
  // return annotation with no escape hatch and a future mis-wiring is a compile
  // error. The pathNs / mappedStringNs objects carry `satisfies QuartoAPI[ns]`
  // (above) as local backstops; the return annotation is the end-to-end one.
  return {
    text,
    markdownRegex,
    crypto,
    format,
    console: consoleNs,
    path: pathNs,
    system: systemNs,
    mappedString: mappedStringNs,
    jupyter: jupyterNs,
  };
}
