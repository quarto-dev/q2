/**
 * @quarto/api/jupyter — widgets
 *
 * Widget dependency extraction (`widgetDependencies`) and the RTQ FC-2
 * deferred-deps producer (`widgetDependencyIncludes`) — the ONLY producer of
 * that wire. `to-markdown.ts` (Task 8) calls `widgetDependencies(nb)` in its
 * pre-walk pass; `result-helpers.ts` (Task 11) imports
 * `widgetDependencyIncludes` from here (both are contracts — do not rename).
 *
 * Ported (REWRITE, not extract) from:
 *   external-sources/quarto-cli/src/core/jupyter/widgets.ts
 *   (`extractJupyterWidgetDependencies` :47-62,
 *    `includesForJupyterWidgetDependencies` :73-152)
 *
 * THREE VERIFIED ADAPTATIONS on `widgetDependencyIncludes` (do not "fix"
 * these back toward literal Q1 parity — see the Plan 3 Task 7 brief):
 *
 *   1. Array-wrap. Q1's `includesForJupyterWidgetDependencies` takes
 *      `JupyterWidgetDependencies[]` (it merges several notebooks' worth of
 *      deps); our namespace method passes a SINGLE `deps`. We array-wrap
 *      (`[deps]`) before running the Q1 merge logic below, rather than
 *      changing the merge logic's arity.
 *   2. Kebab + array return shape. Q1 returns a local
 *      `{ inHeader: string, afterBody: string }` (camelCase, SCALAR path
 *      strings). We translate directly onto the vendored `PandocIncludes`
 *      (kebab keys, ARRAY values): `inHeader` -> `"include-in-header":
 *      [path]`, `afterBody` -> `"include-after-body": [path]`. An empty
 *      fragment omits its key entirely (mirrors Q1's conditional
 *      `if (head.length > 0)` / `if (afterBody.length > 0)` writes).
 *   3. No camelCase rename here. The return type IS the vendored kebab
 *      `PandocIncludes` — identical to `DependenciesResult.includes`. The
 *      kebab -> camel `TsPandocIncludes` rename happens at the wire, in
 *      `engine-host-deno`'s `host.ts`, not in this function.
 */

import type {
  JupyterNotebook,
  JupyterWidgetDependencies,
  JupyterWidgetsState,
  PandocIncludes,
} from "@quarto/types";
import type { PlatformHost } from "../platform/index.js";

import {
  kApplicationJavascript,
  kApplicationJupyterWidgetState,
  kApplicationJupyterWidgetView,
  kTextHtml,
} from "./constants.js";
import { isDisplayData } from "./display-data.js";
import { lines } from "../text/index.js";

/**
 * Scan `nb`'s cell outputs for widget signals — js-widget MIME types, the
 * jupyter-widgets embedding protocol MIME type, hoistable HTML libraries
 * (e.g. plotly), and any `metadata.widgets` state blob — and return the
 * aggregated `JupyterWidgetDependencies`, or `undefined` if the notebook
 * carries none of the above.
 *
 * **LOAD-BEARING MUTATION (P3-17)**: strips each hoisted HTML-library
 * `<script>` bundle out of `cell.outputs` IN PLACE (filters it out of the
 * output array) so a later cell-walk (Task 8's `to-markdown.ts`) does not
 * re-emit it as ordinary output content — it has already been captured into
 * `htmlLibraries` here and belongs in the head, not the cell body. Do NOT
 * drop this strip; without it, plotly/HTML-library `<script>`s double-emit.
 *
 * Ported from Q1 `extractJupyterWidgetDependencies` (`widgets.ts:47-62`).
 */
export function widgetDependencies(
  nb: JupyterNotebook,
): JupyterWidgetDependencies | undefined {
  // a 'javascript' widget doesn't use the jupyter widgets protocol, but
  // rather just injects text/html or application/javascript directly, and
  // often assumes require.js/jquery are available (e.g. itables, plotly).
  const jsWidgets = haveOutputType(nb, [kApplicationJavascript, kTextHtml]);

  // jupyter widgets conform to the jupyter widget embedding protocol:
  // https://ipywidgets.readthedocs.io/en/latest/embedding.html#embeddable-html-snippet
  const jupyterWidgets = haveOutputType(nb, [kApplicationJupyterWidgetView]);

  // see if there are html libraries that need to be hoisted into the head
  const htmlLibraries: string[] = [];
  nb.cells.forEach((cell) => {
    if (cell.cell_type === "code") {
      cell.outputs = cell.outputs?.filter((output) => {
        if (isDisplayData(output)) {
          const html = output.data[kTextHtml];
          const htmlText = Array.isArray(html)
            ? (html as string[]).join("")
            : (html as string | undefined);
          if (html && htmlText && isWidgetIncludeHtml(htmlText)) {
            htmlLibraries.push(htmlLibrariesText(htmlText));
            return false;
          }
        }
        return true;
      });
    }
  });

  const widgetsState = nb.metadata.widgets?.[
    kApplicationJupyterWidgetState
  ] as JupyterWidgetsState | undefined;

  if (
    !jsWidgets &&
    !jupyterWidgets &&
    htmlLibraries.length === 0 &&
    !widgetsState
  ) {
    return undefined;
  }

  return { jsWidgets, jupyterWidgets, htmlLibraries, widgetsState };
}

/**
 * Build the RTQ FC-2 deferred-deps `PandocIncludes` (head + after-body
 * fragments) for a notebook's widget dependencies, writing each non-empty
 * fragment out to a temp file under `tempDir` via `host.fs`.
 *
 * This is the ONLY producer of the deferred-deps wire — see the module doc
 * comment above for the three verified adaptations vs. Q1's
 * `includesForJupyterWidgetDependencies`.
 *
 * Ported from Q1 `includesForJupyterWidgetDependencies` (`widgets.ts:73-152`).
 */
export function widgetDependencyIncludes(
  host: Pick<PlatformHost, "fs">,
  deps: JupyterWidgetDependencies,
  tempDir: string,
): PandocIncludes {
  // Adaptation 1: Q1's merge loop below runs over an ARRAY of dependencies;
  // our namespace method is always called with a single `deps` — array-wrap
  // rather than reshaping the merge loop's arity.
  const dependencies = [deps];

  let haveJavascriptWidgets = false;
  let haveJupyterWidgets = false;
  const htmlLibraries: string[] = [];
  let widgetsState: JupyterWidgetsState | undefined;

  for (const dependency of dependencies) {
    haveJavascriptWidgets = haveJavascriptWidgets || dependency.jsWidgets;
    haveJupyterWidgets = haveJupyterWidgets || dependency.jupyterWidgets;
    for (const htmlLib of dependency.htmlLibraries) {
      if (!htmlLibraries.includes(htmlLib)) {
        htmlLibraries.push(htmlLib);
      }
    }
    if (dependency.widgetsState) {
      // Q1 merges multiple dependencies' widgetsState with `mergeConfigs`
      // when there is more than one entry in the array. With `dependencies`
      // always array-wrapped to length 1 here (Adaptation 1), there is at
      // most one widgetsState to consider, so last-write-wins is equivalent
      // to Q1's merge for this call shape.
      widgetsState = dependency.widgetsState;
    }
  }

  // write required dependencies into head
  const head: string[] = [];
  if (haveJavascriptWidgets || haveJupyterWidgets) {
    head.push(
      '<script src="https://cdn.jsdelivr.net/npm/requirejs@2.3.6/require.min.js" integrity="sha384-c9c+LnTbwQ3aujuU7ULEPVvgLs+Fn6fJUvIGTsuu1ZcCf11fiEubah0ttpca4ntM sha384-6V1/AdqZRWk1KAlWbKBlGhN7VG4iE/yAZcO6NZPMF8od0vukrvr0tg4qY6NSrItx" crossorigin="anonymous"></script>',
    );
    head.push(
      '<script src="https://cdn.jsdelivr.net/npm/jquery@3.5.1/dist/jquery.min.js" integrity="sha384-ZvpUoO/+PpLXR1lu4jmpXWu80pZlYUAfxl5NsBMWOEPSjUn/6Z/hRTt8+pR6L4N2" crossorigin="anonymous" data-relocate-top="true"></script>',
    );
    head.push(
      "<script type=\"application/javascript\">define('jquery', [],function() {return window.jQuery;})</script>",
    );
  }

  // html libraries (e.g. plotly)
  head.push(...htmlLibraries);

  // jupyter widget runtime
  if (haveJupyterWidgets) {
    head.push(
      '<script src="https://cdn.jsdelivr.net/npm/@jupyter-widgets/html-manager@*/dist/embed-amd.js" crossorigin="anonymous"></script>',
    );
  }

  // write jupyter widget state after body if it exists
  const afterBody: string[] = [];
  if (haveJupyterWidgets && widgetsState) {
    const stateStr = JSON.stringify(widgetsState);
    // https://github.com/quarto-dev/quarto-cli/issues/8433
    if (stateStr.includes("</script>")) {
      afterBody.push(`<script>
      (() => {
        const scriptTag = document.createElement("script");
        const base64State = "${btoa(encodeURIComponent(stateStr))}";
        scriptTag.type = "${kApplicationJupyterWidgetState}";
        scriptTag.textContent = decodeURIComponent(atob(base64State));
        document.body.appendChild(scriptTag);
      })();
      </script>`);
    } else {
      afterBody.push(`<script type=${kApplicationJupyterWidgetState}>`);
      afterBody.push(JSON.stringify(widgetsState));
      afterBody.push("</script>");
    }
  }

  // write a fragment's lines to a temp file under tempDir, host-bound.
  const widgetTempFile = (fragmentLines: string[]): string => {
    const tempFile = host.fs.makeTempFile({
      dir: tempDir,
      prefix: "jupyter-widgets-",
      suffix: ".html",
    });
    host.fs.writeFileSync(tempFile, fragmentLines.join("\n") + "\n");
    return tempFile;
  };

  // Adaptations 2 & 3: translate Q1's local camelCase-scalar
  // { inHeader, afterBody } directly onto the vendored kebab-array
  // `PandocIncludes` (no separate rename step, and no camelCase key ever
  // appears in the return value — the harness renames at the wire).
  const result: PandocIncludes = {};
  if (head.length > 0) {
    result["include-in-header"] = [widgetTempFile(head)];
  }
  if (afterBody.length > 0) {
    result["include-after-body"] = [widgetTempFile(afterBody)];
  }
  return result;
}

// ─── internal helpers (unexported — ported from Q1 widgets.ts:169-238) ─────

function haveOutputType(nb: JupyterNotebook, mimeTypes: string[]): boolean {
  return nb.cells.some((cell) => {
    if (cell.cell_type === "code" && cell.outputs) {
      return cell.outputs.some((output) => {
        if (isDisplayData(output)) {
          const outputTypes = Object.keys(output.data);
          return outputTypes.some((type) => mimeTypes.includes(type));
        }
        return false;
      });
    }
    return false;
  });
}

function isWidgetIncludeHtml(html: string): boolean {
  return isPlotlyLibrary(html);
}

function isPlotlyLibrary(html: string): boolean {
  // Plotly before version 6 used require.js to load the library
  const hasRequireScript = (text: string) =>
    /^\s*<script type="text\/javascript">/.test(text) &&
    (/require\.undef\(["']plotly["']\)/.test(text) ||
      /define\('plotly'/.test(text));
  // Plotly 6+ uses the new module syntax
  const hasModuleScript = (text: string) =>
    /\s*<script type="module">import .*plotly.*<\/script>/.test(text);
  // notebook mode non-connected embed emits plotly.js scripts like this:
  //  * plotly.js v3.0.1
  //  * Copyright 2012-2025, Plotly, Inc.
  //  * All rights reserved.
  //  * Licensed under the MIT license
  const hasEmbedScript = (text: string) =>
    /\* plotly\.js v\d+\.\d+\.\d+\s*\n\s*\* Copyright \d{4}-\d{4}, Plotly, Inc\.\s*\n\s*\* All rights reserved\.\s*\n\s*\* Licensed under the MIT license/.test(
      text,
    );
  return (
    hasRequireScript(html) ||
    // also handle new module syntax from plotly.py 6+
    hasModuleScript(html) ||
    // detect plotly by its copyright header
    hasEmbedScript(html)
  );
}

function htmlLibrariesText(htmlText: string): string {
  // strip leading space off of the content so it isn't seen as code by e.g. hugo
  const htmlLines = lines(htmlText);
  const leadingSpace = htmlLines.reduce<number>((leading, line) => {
    const spaces = line.search(/\S/);
    if (spaces !== -1) {
      return Math.min(leading, spaces);
    }
    return leading;
  }, Number.MAX_SAFE_INTEGER);
  if (leadingSpace !== Number.MAX_SAFE_INTEGER) {
    return htmlLines
      .map((line) =>
        line.trim().length === 0 ? "" : line.replace(" ".repeat(leadingSpace), ""),
      )
      .join("\n");
  }
  return htmlText;
}
