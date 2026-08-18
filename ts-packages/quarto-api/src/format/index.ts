/**
 * @quarto/api — format namespace (PURE)
 *
 * Ported from Q1:
 *   - isHtmlCompatible, isIpynbOutput, isLatexOutput, isMarkdownOutput,
 *     isPresentationOutput, isHtmlDashboardOutput: config/format.ts
 *   - isServerShiny, isServerShinyPython: core/render.ts:28-38
 *
 * Format-key constants (kBaseFormat, kPreferHtml, kServer) are used as
 * their literal string values here because @quarto/api/config exports
 * key-set arrays rather than individual named constants.
 * No Deno.* / node:* used — all pure predicate logic.
 */

import type { Format, FormatPandoc } from "@quarto/types";

// ─── internal helpers ─────────────────────────────────────────────────────────

/** String-or-pandoc format predicate helper. */
function isFormatTo(format: string | FormatPandoc, to: string): boolean {
  const formatStr =
    typeof format === "string" ? format : format?.to ?? "html";
  return formatStr.startsWith(to);
}

function isHtmlDocOutput(format: string | FormatPandoc): boolean {
  return ["html", "html4", "html5"].some((fmt) => isFormatTo(format, fmt));
}

function isHtmlSlideOutput(format: string | FormatPandoc): boolean {
  return ["s5", "dzslides", "slidy", "slideous", "revealjs"].some((fmt) =>
    isFormatTo(format, fmt),
  );
}

function isHtmlOutput(format?: string | FormatPandoc): boolean {
  if (typeof format !== "string") {
    format = format?.to;
  }
  format = format ?? "html";
  return (
    isHtmlDocOutput(format) ||
    isHtmlDashboardOutput(format) ||
    isHtmlSlideOutput(format) ||
    isEpubOutput(format)
  );
}

function isEpubOutput(format: string | FormatPandoc): boolean {
  if (typeof format !== "string") {
    format = format?.to ?? "html";
  }
  return ["epub", "epub2", "epub3"].some((fmt) => isFormatTo(format, fmt));
}

// ─── exported predicates ──────────────────────────────────────────────────────

/**
 * True iff the format is HTML-compatible (html, revealjs, markdown+prefer-html,
 * or ipynb outputs).
 * Mirrors Q1 `config/format.ts:170`.
 */
export function isHtmlCompatible(format: Format): boolean {
  return (
    isHtmlOutput(format.pandoc) ||
    (isMarkdownOutput(format) && !!format.render["prefer-html"]) ||
    isIpynbOutput(format.pandoc)
  );
}

/**
 * True iff the pandoc `to` field targets ipynb output.
 * Mirrors Q1 `config/format.ts:141`.
 */
export function isIpynbOutput(format: FormatPandoc): boolean {
  return isFormatTo(format, "ipynb");
}

/**
 * True iff the pandoc `to` field targets LaTeX/PDF output.
 * Mirrors Q1 `config/format.ts:16`.
 */
export function isLatexOutput(format: FormatPandoc): boolean {
  return ["pdf", "latex", "beamer"].some((fmt) => isFormatTo(format, fmt));
}

/**
 * True iff the format's base-format (or pandoc.to) is a Markdown variant
 * (or ipynb). `flavors` defaults to the Q1 set.
 * Mirrors Q1 `config/format.ts:152`.
 */
export function isMarkdownOutput(
  format: Format,
  flavors = [
    "markdown",
    "markdown_github",
    "markdown_mmd",
    "markdown_phpextra",
    "markdown_strict",
    "gfm",
    "commonmark",
    "commonmark_x",
    "markua",
  ],
): boolean {
  // kBaseFormat = "base-format"
  const to = (format.identifier["base-format"] as string | undefined) ?? "html";
  return flavors.includes(to) || isIpynbOutput(format.pandoc);
}

/**
 * True iff the pandoc `to` field targets a presentation format.
 * Mirrors Q1 `config/format.ts:110`.
 */
export function isPresentationOutput(format: FormatPandoc): boolean {
  if (format.to) {
    return ["s5", "dzslides", "slidy", "slideous", "revealjs", "beamer", "pptx"].some(
      (to) => format.to?.startsWith(to),
    );
  }
  return false;
}

/**
 * True iff `format` is a dashboard format string (ends with "-dashboard" or
 * equals "dashboard").
 * Mirrors Q1 `config/format.ts:91`.
 * Note: takes a string (the base-format identifier), not a Format object.
 */
export function isHtmlDashboardOutput(format?: string): boolean {
  return format === "dashboard" || (format?.endsWith("-dashboard") ?? false);
}

/**
 * True iff the format's metadata `server.type` is `"shiny"`.
 * Mirrors Q1 `core/render.ts:28`.
 */
export function isServerShiny(format?: Format): boolean {
  // kServer = "server"
  const server = format?.metadata["server"] as
    | { type?: string }
    | undefined;
  return server?.["type"] === "shiny";
}

/**
 * True iff the format is shiny-server AND the engine is the jupyter engine.
 * Mirrors Q1 `core/render.ts:33`.
 * Note: kJupyterEngine = "jupyter"
 */
export function isServerShinyPython(
  format: Format,
  engine: string | undefined,
): boolean {
  return isServerShiny(format) && engine === "jupyter";
}
