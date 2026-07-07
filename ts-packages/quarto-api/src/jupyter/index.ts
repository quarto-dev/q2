/**
 * @quarto/api/jupyter — public entry point
 *
 * Plan 3 Task 12 (Phase 3D). Assembles the full 23-member
 * `QuartoAPI["jupyter"]` namespace from the composed modules landed in
 * Plan 3: `to-markdown.ts` (`jupyterToMarkdown`), `percent-script.ts`
 * (`isPercentScript`, `percentScriptToMarkdown`), `assets.ts` (`assets`),
 * `widgets.ts` (`widgetDependencyIncludes`), `result-helpers.ts`
 * (`resultIncludes`, `resultEngineDependencies`).
 *
 * That is 7 real, host-bound (or pure) methods. The remaining members of
 * the namespace are:
 *   - `notebookExtensions` — a REAL value (`[".ipynb"]`, Q1's
 *     `kJupyterNotebookExtensions`), not a stub.
 *   - 15 `NotImplemented` throwers (Phase 3E carries their real
 *     implementations) — detection/introspection, conversion, processing,
 *     and runtime/environment methods not yet ported.
 *
 * The factory return is annotated `QuartoAPI["jupyter"]` — this annotation
 * IS the Row 19 conformance gate: deleting any one of the 23 members (real
 * or stub) makes `tsc --noEmit` fail with a missing-property error. Do NOT
 * narrow the namespace type to only the implemented members.
 */

import type { PlatformHost } from "../platform/index.js";
import type { QuartoAPI } from "@quarto/types";

import { jupyterToMarkdown } from "./to-markdown.js";
import { isPercentScript, percentScriptToMarkdown } from "./percent-script.js";
import { assets } from "./assets.js";
import { widgetDependencyIncludes } from "./widgets.js";
import { resultIncludes, resultEngineDependencies } from "./result-helpers.js";

/** Q1 `kJupyterNotebookExtensions` (`jupyter.ts:191`). */
const kJupyterNotebookExtensions = [".ipynb"];

/**
 * Deferred-seam thrower. All 15 stubbed namespace members share this shape: a
 * function that unconditionally throws. `QuartoAPI["jupyter"]` declares its
 * members with arrow-function property syntax, so TypeScript checks
 * assignability with strict (contravariant) parameter variance — a bare
 * `(...args: never[]) => never` is therefore NOT assignable to e.g.
 * `(file: string) => boolean` (`string` is not assignable to `never`). The
 * generic + `as T` cast below is the narrow, per-call-site escape hatch:
 * the thrower's *runtime* shape (accept anything, always throw) is
 * assignable to any function type, sync or async — only the static checker
 * needs the assist.
 */
// accepted-untested (Plan 4b-E, record 5): no q2 TS runtime consumer needs
// any of the 15 throwers below — Julia (the current in-tree jupyter
// consumer) only calls the 7 real methods + `notebookExtensions`. The loose
// guard binding this is `index.test.ts`'s `it.each(STUB_KEYS)("%s throws
// (NotImplemented stub)", ...)`: it asserts each stub FAILS LOUD (throws,
// namespace stays total, no silent no-op can slip in), NOT that throwing is
// the desired permanent state — a future real implementation is free to stop
// throwing without that reading as a regression.
function notImplemented<T extends (...args: never[]) => unknown>(
  method: string,
): T {
  return ((..._args: unknown[]) => {
    throw new Error(
      `quarto.jupyter.${method} is not implemented (Plan 3 ships 7 methods; this is a deferred seam).`,
    );
  }) as unknown as T;
}

/**
 * Build the `quarto.jupyter` namespace: binds `host` into the 7 real
 * (host-dependent or pure) methods landed in Plan 3, provides the real
 * `notebookExtensions` value, and fills the remaining 15 members with
 * `NotImplemented` throwers pending Phase 3E.
 */
export function makeJupyter(
  host: Pick<PlatformHost, "fs">,
): QuartoAPI["jupyter"] {
  return {
    // ── 7 real methods (host bound, or pure) ──────────────────────────
    toMarkdown: (nb, options) => jupyterToMarkdown(host, nb, options),
    isPercentScript: (file, extensions) =>
      isPercentScript(host, file, extensions),
    percentScriptToMarkdown: (file) => percentScriptToMarkdown(host, file),
    assets: (input, to) => assets(host, input, to),
    resultIncludes: (tempDir, dependencies) =>
      resultIncludes(host, tempDir, dependencies),
    widgetDependencyIncludes: (deps, tempDir) =>
      widgetDependencyIncludes(host, deps, tempDir),
    resultEngineDependencies, // pure, no host

    // ── 1 real VALUE (NOT a stub) ──────────────────────────────────────
    notebookExtensions: kJupyterNotebookExtensions,

    // ── 15 NotImplemented throwers (deferred to Phase 3E) ─────────────
    // Detection/introspection
    isJupyterNotebook: notImplemented("isJupyterNotebook"),
    kernelspecFromMarkdown: notImplemented("kernelspecFromMarkdown"),
    kernelspecForLanguage: notImplemented("kernelspecForLanguage"),
    fromJSON: notImplemented("fromJSON"),
    // Conversion
    markdownFromNotebookFile: notImplemented("markdownFromNotebookFile"),
    markdownFromNotebookJSON: notImplemented("markdownFromNotebookJSON"),
    quartoMdToJupyter: notImplemented("quartoMdToJupyter"),
    // Processing
    notebookFiltered: notImplemented("notebookFiltered"),
    // Runtime & Environment
    pythonExec: notImplemented("pythonExec"),
    capabilities: notImplemented("capabilities"),
    capabilitiesMessage: notImplemented("capabilitiesMessage"),
    capabilitiesJson: notImplemented("capabilitiesJson"),
    installationMessage: notImplemented("installationMessage"),
    unactivatedEnvMessage: notImplemented("unactivatedEnvMessage"),
    pythonInstallationMessage: notImplemented("pythonInstallationMessage"),
  };
}

export type {
  JupyterNotebook,
  JupyterCell,
  JupyterToMarkdownOptions,
  JupyterToMarkdownResult,
  JupyterCellOutput,
  JupyterNotebookAssetPaths,
  JupyterWidgetDependencies,
} from "@quarto/types";
