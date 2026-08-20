/**
 * @quarto/api/jupyter — display-data
 *
 * Selects the best MIME type from a Jupyter output's `data` bundle for a
 * given target format, and the predicates that classify a selected MIME
 * type. `to-markdown.ts` (Task 8) drives its output switch through this
 * module.
 *
 * Ported (REWRITE, not extract) from:
 *   external-sources/quarto-cli/src/core/jupyter/display-data.ts
 *
 * The priority order used by `displayDataMimeType` is computed DYNAMICALLY
 * from the target format flags on `JupyterToMarkdownOptions` — it is not a
 * fixed list. See the function body for the exact splice logic; it mirrors
 * Q1's `displayDataMimeType` (`display-data.ts:45-106`) including a
 * source-side quirk: when `toHtml` is set, the widget/html cluster is
 * unconditionally unshifted to the very front of the priority list AFTER
 * the toMarkdown-conditional insert, so (because the selection loop below
 * takes the first matching MIME type and duplicate entries are harmless)
 * the effective priority order is identical whether or not `toMarkdown` is
 * also set: the widget/html cluster always outranks the base order when
 * `toHtml` is set. We reproduce that effective behavior directly rather
 * than re-deriving duplicate array entries.
 */

import type {
  JupyterCellOutputData,
  JupyterOutput,
  JupyterOutputDisplayData,
  JupyterToMarkdownOptions,
} from "@quarto/types";

import {
  kApplicationJavascript,
  kApplicationJupyterWidgetState,
  kApplicationJupyterWidgetView,
  kApplicationPdf,
  kImageJpeg,
  kImagePng,
  kImageSvg,
  kTextHtml,
  kTextLatex,
  kTextMarkdown,
  kTextPlain,
} from "./constants.js";

/**
 * Is `output` a display-data-shaped output (`display_data` or
 * `execute_result`)? A type guard: narrows `output` to
 * `JupyterOutputDisplayData` at call sites (e.g. `to-markdown.ts`'s output
 * switch relies on this narrowing).
 *
 * Ported from Q1 `isDisplayData` (`display-data.ts:30-34`). Previously
 * duplicated locally (unexported) in `to-markdown.ts`, `labels.ts`,
 * `widgets.ts`, and `preserve.ts` — consolidated here to match Q1's layout.
 */
export function isDisplayData(output: JupyterOutput): output is JupyterOutputDisplayData {
  return ["display_data", "execute_result"].includes(output.output_type);
}

/**
 * Is `output` a display-data output that should receive a caption (i.e. not
 * marked `noCaption`)?
 *
 * Ported from Q1 `isCaptionableData` (`display-data.ts:36-41`). Previously
 * duplicated locally (unexported) in `to-markdown.ts` and `labels.ts` —
 * consolidated here to match Q1's layout.
 */
export function isCaptionableData(output: JupyterOutput): boolean {
  return isDisplayData(output) && !output.noCaption;
}

/**
 * Select the best MIME type to render from `output.data`, given the target
 * format flags in `options`. Returns `null` if none of the candidate MIME
 * types are present in the bundle.
 *
 * Ported from Q1 `displayDataMimeType` (`display-data.ts:45-106`).
 */
export function displayDataMimeType(
  output: JupyterCellOutputData,
  options: Pick<JupyterToMarkdownOptions, "toHtml" | "toLatex" | "toMarkdown">,
): string | null {
  const displayPriority: string[] = [kTextMarkdown, kImageSvg, kImagePng, kImageJpeg];

  if (options.toHtml) {
    const htmlFormats = [
      kApplicationJupyterWidgetState,
      kApplicationJupyterWidgetView,
      kApplicationJavascript,
      kTextHtml,
    ];
    // If we are targeting markdown w/ html then prioritize the html formats
    // (this is b/c jupyter widgets also provide a text/markdown
    // representation that we don't want to have "win" over the widget).
    // Otherwise put them after markdown. NOTE: per the source-quirk in the
    // doc comment above, the unconditional unshift immediately below makes
    // this branch's placement irrelevant to the *effective* priority order
    // (the widget/html cluster always ends up ranked first when toHtml is
    // set) — kept here to mirror the source's documented intent.
    if (options.toMarkdown) {
      displayPriority.unshift(...htmlFormats);
    } else {
      displayPriority.push(...htmlFormats);
    }
    displayPriority.unshift(
      kApplicationJupyterWidgetState,
      kApplicationJupyterWidgetView,
      kApplicationJavascript,
      kTextHtml,
    );
  } else if (options.toLatex) {
    displayPriority.push(kTextLatex, kApplicationPdf);
  } else if (options.toMarkdown) {
    displayPriority.push(kTextHtml);
  }

  // If there is an html table then add html (as we can read this directly
  // into the pandoc AST in our lua filters).
  if (displayDataHasHtmlTable(output) && !displayPriority.includes(kTextHtml)) {
    displayPriority.push(kTextHtml);
  }

  // Always add text/plain.
  displayPriority.push(kTextPlain);

  const availDisplay = Object.keys(output.data);
  for (const display of displayPriority) {
    if (availDisplay.includes(display)) {
      return display;
    }
  }
  return null;
}

/**
 * Pure predicate: does this latex source look like math (as opposed to,
 * say, a `\newcommand` preamble or other non-math latex)?
 *
 * Ported from Q1 `displayDataLatexIsMath` (`display-data.ts:108-120`).
 * Does NOT route or emit anything — see `displayDataWithMarkdownMath` for
 * the pre-transform that consumes this predicate.
 */
export function displayDataLatexIsMath(latex: string[]): boolean {
  if (latex.length > 0) {
    const first = latex[0];
    const last = latex[latex.length - 1];
    return (
      // Inline or display math
      (first.startsWith("$") && last.endsWith("$")) ||
      // Latex environment
      (first.startsWith("\\begin{") && last.includes("\\end{"))
    );
  }
  return false;
}

/**
 * Pre-transform on a display-data output: when the output's `text/latex`
 * slot holds math (per `displayDataLatexIsMath`) and there is no existing
 * `text/markdown` slot, hoist the latex into `data["text/markdown"]` so it
 * later renders as math. For non-math latex (or when a `text/markdown`
 * slot already exists), the output is returned UNCHANGED — this function
 * does not emit a `{=tex}` raw block; that is emitted downstream in
 * `to-markdown.ts` (Task 8) for the non-math case.
 *
 * Ported from Q1 `displayDataWithMarkdownMath` (`display-data.ts:122-137`).
 */
export function displayDataWithMarkdownMath(
  output: JupyterOutputDisplayData,
): JupyterOutputDisplayData {
  if (Array.isArray(output.data[kTextLatex]) && !output.data[kTextMarkdown]) {
    const latex = output.data[kTextLatex] as string[];
    if (displayDataLatexIsMath(latex)) {
      return {
        ...output,
        data: {
          ...output.data,
          [kTextMarkdown]: latex,
        },
      };
    }
  }
  return output;
}

/**
 * Does `output.data["text/html"]` contain an html `<table>`?
 *
 * Ported from Q1 `displayDataHasHtmlTable` (`display-data.ts:139-154`).
 */
export function displayDataHasHtmlTable(output: JupyterCellOutputData): boolean {
  const raw = output.data[kTextHtml];
  const html = Array.isArray(raw)
    ? (raw as string[])
    : typeof raw === "string"
      ? [raw]
      : undefined;
  if (html) {
    const htmlLower = html.map((line) => line.toLowerCase());
    return (
      htmlLower.some((line) => /<table/.test(line)) &&
      htmlLower.some((line) => /<\/table/.test(line))
    );
  }
  return false;
}

/**
 * Is the selected MIME type an image (png/jpeg/svg/pdf)?
 *
 * Ported from Q1 `displayDataIsImage` (`display-data.ts:156`).
 */
export function displayDataIsImage(mimeType: string): boolean {
  return [kImagePng, kImageJpeg, kImageSvg, kApplicationPdf].includes(mimeType);
}

/**
 * Is the selected MIME type `text/markdown`?
 *
 * Ported from Q1 `displayDataIsMarkdown` (`display-data.ts:164-166`).
 */
export function displayDataIsMarkdown(mimeType: string): boolean {
  return mimeType === kTextMarkdown;
}

/**
 * Is the selected MIME type `text/latex`?
 *
 * Ported from Q1 `displayDataIsLatex` (`display-data.ts:168-170`).
 */
export function displayDataIsLatex(mimeType: string): boolean {
  return mimeType === kTextLatex;
}

/**
 * Is the selected MIME type `text/html`?
 *
 * Ported from Q1 `displayDataIsHtml` (`display-data.ts:172-174`).
 */
export function displayDataIsHtml(mimeType: string): boolean {
  return mimeType === kTextHtml;
}

/**
 * Is the selected MIME type one of the two Jupyter widget MIME types
 * (`application/vnd.jupyter.widget-state+json` /
 * `…widget-view+json`)? There is NO generic `application/json` path in Q1
 * — a bare `application/json` or `text/html` MIME type is NOT json per
 * this predicate.
 *
 * Ported from Q1 `displayDataIsJson` (`display-data.ts:176-179`).
 */
export function displayDataIsJson(mimeType: string): boolean {
  return [kApplicationJupyterWidgetState, kApplicationJupyterWidgetView].includes(mimeType);
}

/**
 * Is the selected MIME type `application/javascript`?
 *
 * Ported from Q1 `displayDataIsJavascript` (`display-data.ts:181-183`).
 */
export function displayDataIsJavascript(mimeType: string): boolean {
  return mimeType === kApplicationJavascript;
}
