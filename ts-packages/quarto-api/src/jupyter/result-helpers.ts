/**
 * @quarto/api/jupyter — result-helpers
 *
 * Two small namespace methods that operate on an execution result's widget
 * `dependencies` field: `resultIncludes` (host-dependent — reuses the widget
 * temp-file builder) and `resultEngineDependencies` (pure, array-wrap only).
 *
 * Ported (REWRITE, not extract) from:
 *   external-sources/quarto-cli/src/core/jupyter/jupyter.ts
 *   (`executeResultEngineDependencies` :2177-2185)
 *   external-sources/quarto-cli/src/core/jupyter/widgets.ts (includes path)
 */

import type { JupyterWidgetDependencies, PandocIncludes } from "@quarto/types";
import type { PlatformHost } from "../platform/index.js";

import { widgetDependencyIncludes } from "./widgets.js";

/**
 * Build the `PandocIncludes` for an execution result's widget dependencies.
 *
 * When `dependencies` is `undefined` (the common widget hot-path call, e.g.
 * Julia — `julia:256`), return an empty `PandocIncludes` WITHOUT invoking the
 * temp-file builder. When `dependencies` is present, delegate to
 * `widgetDependencyIncludes` (do NOT duplicate its logic here).
 */
export function resultIncludes(
  host: Pick<PlatformHost, "fs">,
  tempDir: string,
  dependencies?: JupyterWidgetDependencies,
): PandocIncludes {
  if (!dependencies) {
    return {};
  }
  return widgetDependencyIncludes(host, dependencies, tempDir);
}

/**
 * Array-wrap a single execution result's widget dependencies, or `undefined`
 * if there are none.
 *
 * Ported from Q1 `executeResultEngineDependencies` (`jupyter.ts:2177-2185`).
 */
export function resultEngineDependencies(
  dependencies?: JupyterWidgetDependencies,
): Array<JupyterWidgetDependencies> | undefined {
  return dependencies ? [dependencies] : undefined;
}
