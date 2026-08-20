/**
 * @quarto/api/jupyter — percent-script
 *
 * Detect and convert Jupyter "percent" scripts (`.py`/`.jl`/`.r`/`.q` files
 * using `# %%`-style cell markers) into markdown. **Host-dependent** (reads
 * files via `host.fs.readTextFileSync`) and **couples to `to-markdown.ts`**
 * (imports `mdRawOutput`/`mdFormatOutput` for raw-cell output formatting, per
 * Q1 `percent.ts:12` — this module does not duplicate that logic).
 *
 * Ported (REWRITE, not extract) from:
 *   external-sources/quarto-cli/src/core/jupyter/percent.ts
 *   (`isJupyterPercentScript` :32-45, `markdownFromJupyterPercentScript`
 *    :47-107, `percentCellHeader` :123-137, `parsePercentAttribs` :139-151)
 *
 * DETECTION (P3-14, corrected): a bare `# %%` code marker is NOT a percent
 * script by itself — detection requires a `[markdown]`/`[raw]` marker:
 * `^\s*${cms}\s*%%+\s+\[(markdown|raw)\]` where `cms` is the file's
 * language-specific comment char (from `kLangCommentChars`, open string of a
 * `[open, close]` tuple).
 */

import type { Metadata } from "@quarto/types";
import type { PlatformHost } from "../platform/index.js";
import { kLangCommentChars } from "./constants.js";
import { mdFormatOutput, mdRawOutput } from "./to-markdown.js";
import { pandocAttrKeyvalueFromText } from "../markdownRegex/index.js";
import { lines, trimEmptyLines, asYamlText } from "../text/index.js";

/** Matches Q1's local (unexported) `kCellRawMimeType` in `jupyter.ts`. */
const kCellRawMimeType = "raw_mimetype";

/** Default extensions searched when `isPercentScript`'s `exts` is omitted. */
export const kJupyterPercentScriptExtensions = [".py", ".jl", ".r", ".q"];

const kLanguageExtensions: Record<string, string> = {
  ".jl": "julia",
  ".r": "r",
  ".py": "python",
  ".q": "q",
};

function extname(file: string): string {
  const base = file.split(/[\\/]/).pop() ?? file;
  const idx = base.lastIndexOf(".");
  return idx > 0 ? base.slice(idx) : "";
}

function commentOpen(cms: string | [string, string]): string {
  return Array.isArray(cms) ? cms[0] : cms;
}

/**
 * Is `file` a Jupyter "percent" script? Checks the extension against `exts`
 * (default `kJupyterPercentScriptExtensions`, which includes `.q`), then
 * reads the file and requires at least one `[markdown]`/`[raw]` percent
 * marker — a code-only `# %%` script is NOT detected.
 */
export function isPercentScript(
  host: Pick<PlatformHost, "fs">,
  file: string,
  exts?: string[],
): boolean {
  const ext = extname(file).toLowerCase();
  const availableExtensions = exts ?? kJupyterPercentScriptExtensions;
  if (!availableExtensions.includes(ext)) {
    return false;
  }
  const text = host.fs.readTextFileSync(file);
  const cms = commentOpen(kLangCommentChars[kLanguageExtensions[ext]]);
  const pat = new RegExp(`^\\s*${cms}\\s*%%+\\s+\\[(markdown|raw)\\]`, "m");
  return pat.test(text);
}

interface PercentCellHeader {
  type: "code" | "raw" | "markdown";
  metadata?: Metadata;
}

interface PercentCell {
  header: PercentCellHeader;
  lines: string[];
}

function parsePercentAttribs(attribs: string): Metadata | undefined {
  // skip over an optional leading title
  const match = attribs.match(/[\w-]+=.*$/);
  if (!match) {
    return undefined;
  }
  const metadata: Metadata = {};
  // separator " " reproduces this module's prior local copy: it always
  // applied the space→newline conversion before splitting.
  for (const [key, value] of pandocAttrKeyvalueFromText(match[0], " ")) {
    metadata[key] = value;
  }
  return metadata;
}

function percentCellHeader(
  line: string,
  language: string,
): PercentCellHeader | undefined {
  const cms = commentOpen(kLangCommentChars[language]);
  const match = line.match(
    new RegExp(`^\\s*${cms}\\s*%%+\\s*(?:\\[(markdown|raw)\\])?\\s*(.*)?$`),
  );
  if (!match) {
    return undefined;
  }
  const type = (match[1] || "code") as PercentCellHeader["type"];
  return { type, metadata: parsePercentAttribs(match[2] || "") };
}

/**
 * Convert a percent-format script at `file` to markdown: `%%+ [markdown]`
 * sections become markdown cells, `[raw]` sections become raw cells
 * (formatted via `mdRawOutput`/`mdFormatOutput` when metadata identifies a
 * MIME type or format), and any other `%%`-delimited content becomes a code
 * cell fenced with the file's language.
 */
export function percentScriptToMarkdown(
  host: Pick<PlatformHost, "fs">,
  file: string,
): string {
  const ext = extname(file).toLowerCase();
  const language = kLanguageExtensions[ext];
  if (!language) {
    throw new Error(`Could not determine language from file extension ${ext}`);
  }
  const cms = commentOpen(kLangCommentChars[language]);

  const cells: PercentCell[] = [];
  const activeCell = () => cells[cells.length - 1];
  for (const line of lines(host.fs.readTextFileSync(file).trim())) {
    const header = percentCellHeader(line, language);
    if (header) {
      cells.push({ header, lines: [] });
    } else {
      activeCell()?.lines.push(line);
    }
  }

  const isTripleQuote = (line: string) => /^"{3,}\s*$/.test(line);
  const asCell = (cellLines: string[]) => cellLines.join("\n") + "\n\n";
  const stripPrefix = (line: string) =>
    line.replace(new RegExp(`^${cms}\\s?`), "");
  const cellContent = (cellLines: string[]) => {
    if (
      cellLines.length > 2 &&
      isTripleQuote(cellLines[0]) &&
      isTripleQuote(cellLines[cellLines.length - 1])
    ) {
      return asCell(cellLines.slice(1, cellLines.length - 1));
    }
    return asCell(cellLines.map(stripPrefix));
  };

  return cells.reduce((markdown, cell) => {
    const cellLines = trimEmptyLines(cell.lines);
    if (cell.header.type === "code") {
      if (cell.header.metadata) {
        const yamlText = asYamlText(cell.header.metadata);
        cellLines.unshift(...lines(yamlText).map((line) => `${cms}| ${line}`));
      }
      return markdown + asCell(["```{" + language + "}", ...cellLines, "```"]);
    }
    if (cell.header.type === "markdown") {
      return markdown + cellContent(cellLines);
    }
    // raw
    let rawContent = cellContent(cellLines);
    const format = cell.header.metadata?.["format"];
    const mimeType = cell.header.metadata?.[kCellRawMimeType];
    if (typeof mimeType === "string") {
      const rawBlock = mdRawOutput(mimeType, lines(rawContent));
      rawContent = rawBlock || rawContent;
    } else if (typeof format === "string") {
      rawContent = mdFormatOutput(format, lines(rawContent));
    }
    return markdown + rawContent;
  }, "");
}
