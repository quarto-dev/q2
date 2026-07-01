/**
 * @quarto/api — markdownRegex namespace (PURE, no I/O)
 *
 * Ported from Q1:
 *   - extractYaml:              core/yaml.ts:80    (readYamlFromMarkdown)
 *   - partition:                core/pandoc/pandoc-partition.ts:50  (partitionMarkdown)
 *   - getLanguages:             core/pandoc/pandoc-partition.ts:141 (languagesInMarkdown)
 *   - getLanguagesWithClasses:  core/pandoc/pandoc-partition.ts:117 (languagesWithClasses)
 *   - breakQuartoMd:            core/lib/break-quarto-md.ts:28
 *
 * All functions are pure (no Deno.* / node:* / I/O).
 * breakQuartoMd is async with no actual I/O — the async signature is preserved
 * for interface compatibility with Q1.
 *
 * Dependencies:
 *   - mappedString pure exports (EitherString, MappedString, fromString,
 *     rangedLines, etc.) from @quarto/api/mappedString
 *   - yaml package (for extractYaml's YAML parsing)
 *
 * No Deno.* / node:* used anywhere in this file.
 */

import { parse as yamlLoad } from "yaml";
import {
  type EitherString,
  type MappedString,
  type Range,
  type RangedSubstring,
  fromString,
  rangedLines,
} from "../mappedString/index.js";

// ── Re-exported types (consumers may need them) ──────────────────────────────

export interface PandocAttr {
  id: string;
  classes: string[];
  keyvalue: Array<[string, string]>;
}

export interface PartitionedMarkdown {
  yaml?: Record<string, unknown>;
  headingText?: string;
  headingAttr?: PandocAttr;
  containsRefs: boolean;
  markdown: string;
  srcMarkdownNoYaml: string;
}

export interface CodeCellType {
  language: string;
}

export interface DirectiveCellType {
  language: "_directive";
  name: string;
  shortcode: Shortcode;
}

export interface Shortcode {
  name: string;
  rawParams: { name?: string; value: string }[];
  namedParams: Record<string, string>;
  params: string[];
}

export interface QuartoMdCell {
  id?: string;
  // deno-lint-ignore camelcase (mirroring Q1 name)
  cell_type: CodeCellType | DirectiveCellType | "markdown" | "raw";
  options?: Record<string, unknown>;
  source: MappedString;
  sourceVerbatim: MappedString;
  sourceWithYaml?: MappedString;
  sourceOffset: number;
  sourceStartLine: number;
  cellStartLine: number;
}

export interface QuartoMdChunks {
  cells: QuartoMdCell[];
}

// ── Internal helpers ─────────────────────────────────────────────────────────

function textLines(text: string): string[] {
  return text.split(/\r\n?|\n/);
}

function normalizeNewlinesStr(text: string): string {
  return textLines(text).join("\n");
}

// ── extractYaml (readYamlFromMarkdown) ───────────────────────────────────────
//
// Ported from Q1 core/yaml.ts:80.
// Strips HTML comments and fenced code blocks, then extracts all YAML front-matter
// / YAML blocks, and parses them into a plain object.

const kRegExYAML =
  /(^)(---[ \t]*[\r\n]+(?![ \t]*[\r\n]+)[\W\w]*?[\r\n]+(?:---|\.\.\.))([ \t]*)$/gm;
const kRegxHTMLComment = /<!--[\W\w]*?-->/gm;
const kRegexFencedCode = /^([\t >]*`{3,})[^`\n]*\n[\W\w]*?\n\1\s*$/gm;

function removeYamlDelimiters(yaml: string): string {
  return yaml.replace(/^---/, "").replace(/---\s*$/, "");
}

/**
 * Extract all YAML metadata from a markdown document.
 *
 * Mirrors Q1 `readYamlFromMarkdown` (core/yaml.ts:80).
 */
export function extractYaml(markdown: string): Record<string, unknown> {
  if (!markdown) return {};

  markdown = normalizeNewlinesStr(markdown);
  markdown = markdown.replaceAll(kRegxHTMLComment, "");
  markdown = markdown.replaceAll(kRegexFencedCode, "");

  let yaml = "";
  kRegExYAML.lastIndex = 0;
  let match = kRegExYAML.exec(markdown);
  while (match != null) {
    let yamlBlock = removeYamlDelimiters(match[2]);
    yamlBlock = textLines(yamlBlock)
      .map((x) => x.trimEnd())
      .join("\n");

    if (
      !yamlBlock.match(/^\n\s*\n/) &&
      !yamlBlock.match(/^\n\s*\n---/m) &&
      yamlBlock.trim().length > 0
    ) {
      yaml += yamlBlock;
    }

    match = kRegExYAML.exec(markdown);
  }
  kRegExYAML.lastIndex = 0;

  const metadata = yamlLoad(yaml) as
    | Record<string, unknown>
    | null
    | undefined;
  return (metadata ?? {}) as Record<string, unknown>;
}

// ── partitionYamlFrontMatter (internal) ──────────────────────────────────────

const kRegExBeginYAML = /^---[ \t]*$/;
const kRegExEndYAML = /^(?:---|\.\.\.)([ \t]*)$/;

function partitionYamlFrontMatter(
  markdown: string,
): { yaml: string; markdown: string } | null {
  const mdLines = textLines(markdown.trimStart());
  if (mdLines.length < 3 || !mdLines[0].match(kRegExBeginYAML)) {
    return null;
  }
  if (
    mdLines[1].trim().length === 0 ||
    mdLines[1].match(kRegExEndYAML)
  ) {
    return null;
  }
  const endYamlPos = mdLines.findIndex(
    (line, index) => index > 0 && line.match(kRegExEndYAML),
  );
  if (endYamlPos === -1) return null;

  return {
    yaml: mdLines.slice(0, endYamlPos + 1).join("\n"),
    markdown: "\n" + mdLines.slice(endYamlPos + 1).join("\n"),
  };
}

// ── pandocAttrParseText (internal) ───────────────────────────────────────────
// Ported from Q1 core/pandoc/pandoc-attr.ts

export function pandocAttrKeyvalueFromText(
  text: string,
  separator: " " | "\n",
): Array<[string, string]> {
  if (separator === " ") {
    let convertedText = "";
    let inQuotes = false;
    for (let i = 0; i < text.length; i++) {
      let ch = text.charAt(i);
      if (ch === '"') {
        inQuotes = !inQuotes;
      } else if (ch === " " && !inQuotes) {
        ch = "\n";
      }
      convertedText += ch;
    }
    text = convertedText;
  }
  const lines = text.trim().split("\n");
  return lines.map((line) => {
    const parts = line.trim().split("=");
    return [parts[0], (parts[1] || "").replace(/^"/, "").replace(/"$/, "")];
  });
}

function pandocAttrParseText(attr: string): PandocAttr | null {
  attr = attr.trim();
  let id = "";
  const classes: string[] = [];
  let remainder = "";
  let current = "";

  const resolveCurrent = () => {
    const resolve = current;
    current = "";
    if (resolve.length === 0) return true;
    if (resolve.startsWith("#")) {
      if (id.length === 0 && resolve.length > 1) {
        id = resolve.slice(1);
        return true;
      }
      return false;
    }
    if (resolve.startsWith(".")) {
      if (resolve.length > 1) {
        classes.push(resolve.slice(1));
        return true;
      }
      return false;
    }
    if (resolve === "-") {
      classes.push("unnumbered");
      return true;
    }
    remainder = resolve;
    return true;
  };

  for (let i = 0; i < attr.length; i++) {
    let inQuotes = false;
    const ch = attr[i];
    inQuotes = ch === '"' ? !inQuotes : inQuotes;
    if (ch !== " " && !inQuotes) {
      current += ch;
    } else if (resolveCurrent()) {
      if (remainder.length > 0) {
        remainder = remainder + attr.slice(i);
        break;
      }
    } else {
      return null;
    }
  }

  if (resolveCurrent()) {
    if (id.length === 0 && classes.length === 0) {
      remainder = attr;
    }
    return {
      id,
      classes,
      keyvalue:
        remainder.length > 0
          ? pandocAttrKeyvalueFromText(remainder, " ")
          : [],
    };
  }
  return null;
}

// ── parsePandocTitle (internal) ───────────────────────────────────────────────

const kPandocTitleRegex = /^\#{1,}\s(?:(.*)\s)?\{(.*)\}$/;
const kRemoveHeadingRegex = /^#{1,}\s*/;

function parsePandocTitle(title: string): {
  heading: string;
  attr?: PandocAttr;
} {
  title = title ? title.trim() : title;
  const match = title.match(kPandocTitleRegex);
  if (match) {
    const titleRaw = match[1];
    const attrRaw = match[2];
    const parsed = pandocAttrParseText(attrRaw);
    return parsed ? { heading: titleRaw, attr: parsed } : { heading: titleRaw };
  }
  return { heading: title.replace(kRemoveHeadingRegex, "").trim() };
}

// ── markdownWithExtractedHeading (internal) ────────────────────────────────

function markdownWithExtractedHeading(markdown: string): {
  lines: string[];
  headingText?: string;
  headingAttr?: PandocAttr;
  contentBeforeHeading: boolean;
} {
  const mdLines: string[] = [];
  let headingText: string | undefined;
  let headingAttr: PandocAttr | undefined;
  let contentBeforeHeading = false;

  for (const line of textLines(markdown)) {
    if (!headingText) {
      if (line.match(/^\#{1,}\s/)) {
        const parsedHeading = parsePandocTitle(line);
        headingText = parsedHeading.heading;
        headingAttr = parsedHeading.attr;
        contentBeforeHeading = mdLines.length !== 0;
      } else if (line.match(/^=+\s*$/) || line.match(/^-+\s*$/)) {
        const prevLine = mdLines[mdLines.length - 1];
        if (prevLine) {
          headingText = prevLine;
          mdLines.splice(mdLines.length - 1);
          contentBeforeHeading = mdLines.length !== 0;
        } else {
          mdLines.push(line);
        }
      } else {
        mdLines.push(line);
      }
    } else {
      mdLines.push(line);
    }
  }

  return { lines: mdLines, headingText, headingAttr, contentBeforeHeading };
}

// ── partition (partitionMarkdown) ─────────────────────────────────────────────
//
// Ported from Q1 core/pandoc/pandoc-partition.ts:50.

/**
 * Partition a markdown document into YAML front-matter, first heading,
 * and remaining body.
 *
 * Mirrors Q1 `partitionMarkdown` (core/pandoc/pandoc-partition.ts:50).
 */
export function partition(markdown: string): PartitionedMarkdown {
  const partitioned = partitionYamlFrontMatter(markdown);
  const body = partitioned ? partitioned.markdown : markdown;

  const { lines, headingText, headingAttr } =
    markdownWithExtractedHeading(body);

  const containsRefs = lines.some((line) =>
    /^:::\s*{#refs([\s}]|.*?})\s*$/.test(line),
  );

  return {
    yaml: partitioned ? extractYaml(partitioned.yaml) : undefined,
    headingText,
    headingAttr,
    containsRefs,
    markdown: lines.join("\n"),
    srcMarkdownNoYaml: partitioned?.markdown ?? "",
  };
}

// ── getLanguagesWithClasses (languagesWithClasses) ───────────────────────────
//
// Ported from Q1 core/pandoc/pandoc-partition.ts:117.

/**
 * Extract all code cell languages from a markdown document, along with
 * the first class attached to each language.
 *
 * Mirrors Q1 `languagesWithClasses` (core/pandoc/pandoc-partition.ts:117).
 *
 * @returns Map from lower-cased language name → first class (or undefined)
 */
export function getLanguagesWithClasses(
  markdown: string,
): Map<string, string | undefined> {
  const result = new Map<string, string | undefined>();
  const kChunkRegex =
    /^[\t >]*```+\s*\{([a-zA-Z][a-zA-Z0-9_.]*)([^}]*)?\}\s*$/gm;
  kChunkRegex.lastIndex = 0;
  let match = kChunkRegex.exec(markdown);
  while (match) {
    const language = match[1].toLowerCase();
    if (!result.has(language)) {
      const attrs = match[2];
      const firstClass = attrs?.match(/\.([a-zA-Z][a-zA-Z0-9_-]*)/)?.[1];
      result.set(language, firstClass);
    }
    match = kChunkRegex.exec(markdown);
  }
  kChunkRegex.lastIndex = 0;
  return result;
}

// ── getLanguages (languagesInMarkdown) ────────────────────────────────────────
//
// Ported from Q1 core/pandoc/pandoc-partition.ts:141.

/**
 * Extract the set of code cell languages from a markdown document.
 *
 * Mirrors Q1 `languagesInMarkdown` (core/pandoc/pandoc-partition.ts:141).
 */
export function getLanguages(markdown: string): Set<string> {
  return new Set(getLanguagesWithClasses(markdown).keys());
}

// ── Shortcode parser (internal, ported from Q1 core/lib/parse-shortcode.ts) ──

class InvalidShortcodeError extends Error {
  constructor(msg: string) {
    super(msg);
  }
}

function parseShortcodeCapture(capture: string): Shortcode | undefined {
  const nameMatch = capture.match(/^\/?[a-zA-Z0-9_]+/);
  if (!nameMatch) return undefined;

  const params: string[] = [];
  const namedParams: Record<string, string> = {};
  const rawParams: Shortcode["rawParams"] = [];

  const name = nameMatch[0];
  let paramStr = capture.slice(name.length).trim();

  while (paramStr.length) {
    let paramMatch: RegExpMatchArray | null;

    paramMatch = paramStr.match(/^[a-zA-Z0-9_-]+="[^"]*"/);
    if (!paramMatch) paramMatch = paramStr.match(/^[a-zA-Z0-9_-]+='[^']*'/);
    if (!paramMatch) paramMatch = paramStr.match(/^[a-zA-Z0-9_-]+=[^"'\s]+/);

    if (paramMatch) {
      const eqIdx = paramMatch[0].indexOf("=");
      const pName = paramMatch[0].slice(0, eqIdx);
      const pValue = paramMatch[0].slice(eqIdx + 1);
      namedParams[pName] = pValue;
      rawParams.push({ name: pName, value: pValue });
      paramStr = paramStr.slice(paramMatch[0].length).trim();
      continue;
    }

    paramMatch = paramStr.match(/^[^"'\s]+/);
    if (paramMatch) {
      params.push(paramMatch[0]);
      rawParams.push({ value: paramMatch[0] });
      paramStr = paramStr.slice(paramMatch[0].length).trim();
      continue;
    }

    paramMatch =
      paramStr.match(/^"[^"]*"/) || paramStr.match(/^'[^']*'/);
    if (paramMatch) {
      const v = paramMatch[0].slice(1, -1);
      params.push(v);
      rawParams.push({ value: v });
      paramStr = paramStr.slice(paramMatch[0].length).trim();
      continue;
    }

    throw new InvalidShortcodeError("invalid shortcode: " + capture);
  }

  return { name, params, namedParams, rawParams };
}

function parseShortcode(shortCodeCapture: string): Shortcode {
  const result = parseShortcodeCapture(shortCodeCapture);
  if (!result) {
    throw new InvalidShortcodeError("invalid shortcode: " + shortCodeCapture);
  }
  return result;
}

function isBlockShortcode(
  content: string,
  lenient?: boolean,
): Shortcode | false {
  const m = content.match(/^\s*{{< (?!\/\*)(.+?)(?<!\*\/) >}}\s*$/);
  if (m) {
    try {
      return parseShortcode(m[1]);
    } catch {
      if (lenient) return false;
      throw new InvalidShortcodeError("invalid shortcode: " + m[1]);
    }
  }
  return false;
}

// ── guessChunkOptionsFormat (internal) ───────────────────────────────────────
//
// Ported from Q1 core/lib/guess-chunk-options-format.ts.
// Heuristic: detects knitr-style options (key=value, key2=value2) versus
// YAML-style options (key: value). Returns "knitr" or "yaml".
//
// Q1 reference: external-sources/quarto-cli/src/core/lib/guess-chunk-options-format.ts

function guessChunkOptionsFormat(options: string): "knitr" | "yaml" {
  // Find all lines without indentation and without a colon
  const noIndentOrColon = /^[^:\s]+[^:]+$/;
  const chunkLines = textLines(options);

  // if there are no lines without indentation and colons, this must be yaml
  if (chunkLines.filter((l) => l.match(noIndentOrColon)).length === 0) {
    return "yaml";
  }

  // If there is a non-empty line that does not end with a comma and
  // does not have an equals, then this is actually yaml (with
  // possibly errors, so we want to report them)
  if (
    chunkLines.some(
      (l) =>
        l.trim() !== "" &&
        !l.trimEnd().endsWith(",") &&
        l.indexOf("=") === -1,
    )
  ) {
    return "yaml";
  }

  // this is likely knitr.
  return "knitr";
}

// ── partitionCellOptionsMapped stubs (internal) ──────────────────────────────
// breakQuartoMd calls partitionCellOptionsMapped to parse inline cell YAML.
// We port a simplified pure version: it reads "# | key: value" comment lines,
// parses them as YAML, and returns the rest.

function langCommentChars(lang: string): string[] {
  const kLangCommentChars: Record<string, string | [string, string]> = {
    r: "#",
    python: "#",
    julia: "#",
    scala: "//",
    matlab: "%",
    csharp: "//",
    fsharp: "//",
    c: ["/*", "*/"],
    css: ["/*", "*/"],
    sas: ["*", ";"],
    powershell: "#",
    bash: "#",
    sql: "--",
    mysql: "--",
    psql: "--",
    lua: "--",
    cpp: "//",
    cc: "//",
    stan: "#",
    octave: "#",
    fortran: "!",
    fortran95: "!",
    awk: "#",
    gawk: "#",
    stata: "*",
    java: "//",
    groovy: "//",
    kotlin: "//",
    sed: "#",
    perl: "#",
    prql: "#",
    ruby: "#",
    tikz: "%",
    js: "//",
    d3: "//",
    node: "//",
    sass: "//",
    scss: "//",
    coffee: "#",
    go: "//",
    asy: "//",
    haskell: "--",
    dot: "//",
    ojs: "//",
    apl: "⍝",
    ocaml: ["(*", "*)"],
    q: "/",
    rust: "//",
  };
  const chars = kLangCommentChars[lang] || "#";
  return Array.isArray(chars) ? chars : [chars];
}

function escapeRegExp(str: string): string {
  return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function optionCommentPattern(comment: string): RegExp {
  return new RegExp("^" + escapeRegExp(comment) + "\\s*\\| ?");
}

/**
 * Inline mapped-string helpers (mirrors Q1 core/lib/mapped-text.ts).
 * We need `mappedString` and `mappedSubstring` internally.
 */

/** Internal: build MappedString from source + array of Range | string chunks */
function mappedStringFromSource(
  source: MappedString,
  pieces: (Range | string)[],
  fileName?: string,
): MappedString {
  type StringMapResult =
    | { index: number; originalString: MappedString }
    | undefined;

  // helper: make substring MappedString
  const subMs = (start: number, end: number): MappedString => {
    const value = source.value.substring(start, end);
    return {
      value,
      fileName: fileName ?? source.fileName,
      map: (index: number, closest?: boolean): StringMapResult => {
        if (closest) index = Math.max(0, Math.min(value.length - 1, index));
        if (index < 0 || index >= value.length) return undefined;
        return source.map(index + start, closest);
      },
    };
  };

  const mapped = pieces.map((p) => {
    if (typeof p === "string") {
      const s = p;
      return fromString(s);
    }
    return subMs(p.start, p.end);
  });

  if (mapped.length === 0) {
    return fromString("", fileName ?? source.fileName);
  }

  // concat
  let currentOffset = 0;
  const offsets: number[] = [0];
  for (const m of mapped) {
    currentOffset += m.value.length;
    offsets.push(currentOffset);
  }
  const value = mapped.map((m) => m.value).join("");

  return {
    value,
    fileName: fileName ?? source.fileName,
    map: (offset: number, closest?: boolean): StringMapResult => {
      type GLBResult = number;
      const glb = (arr: number[], val: number): GLBResult => {
        if (arr.length === 0) return -1;
        if (arr.length === 1) return val < arr[0] ? -1 : 0;
        if (val >= arr[arr.length - 1]) return arr.length - 1;
        if (val < arr[0]) return -1;
        let lo = 0,
          hi = arr.length - 1;
        while (hi - lo > 1) {
          const mid = lo + ((hi - lo) >> 1);
          if (val < arr[mid]) hi = mid;
          else lo = mid;
        }
        return lo;
      };

      if (closest) offset = Math.max(0, Math.min(offset, value.length - 1));
      if (offset === 0 && offset === value.length && mapped.length)
        return mapped[0].map(0, closest);
      if (offset < 0 || offset >= value.length) return undefined;
      const ix = glb(offsets, offset);
      return mapped[ix].map(offset - offsets[ix]);
    },
  };
}

/** Internal: mappedSubstring equivalent */
function mappedSubstringOf(
  source: MappedString,
  start: number,
  end?: number,
): MappedString {
  return mappedStringFromSource(source, [{ start, end: end ?? source.value.length }]);
}

// ── partitionCellOptionsMapped (internal pure port) ─────────────────────────

async function partitionCellOptionsMapped(
  language: string,
  outerSource: MappedString,
  _validate = false,
  _engine = "",
  _lenient = false,
): Promise<{
  yaml: Record<string, unknown> | undefined;
  source: MappedString;
  sourceStartLine: number;
}> {
  const commentChars = langCommentChars(language);
  const optionPattern = optionCommentPattern(commentChars[0]);

  const srcLines = rangedLines(outerSource.value, true);
  const yamlLines: string[] = [];
  let endOfYamlOffset = 0;

  for (const line of srcLines) {
    const optionMatch = line.substring.match(optionPattern);
    if (optionMatch) {
      const yamlOption = line.substring.slice(optionMatch[0].length).trimEnd();
      yamlLines.push(yamlOption);
      endOfYamlOffset = line.range.end;
    } else {
      break;
    }
  }

  let yaml: Record<string, unknown> | undefined;
  if (yamlLines.length > 0) {
    const yamlText = yamlLines.join("\n");
    // Guard: skip YAML parsing for knitr-style option lines (e.g. `echo=TRUE, fig.width=5`).
    // Mirrors Q1 core/lib/partition-cell-options.ts — guessChunkOptionsFormat check.
    if (guessChunkOptionsFormat(yamlText) !== "knitr") {
      const parsed = yamlLoad(yamlText);
      if (parsed && typeof parsed === "object") {
        yaml = parsed as Record<string, unknown>;
      }
    }
  }

  return {
    yaml,
    source: mappedSubstringOf(outerSource, endOfYamlOffset),
    sourceStartLine: yamlLines.length,
  };
}

// ── lineOffsets (internal) ────────────────────────────────────────────────────

function* lineOffsets(text: string): Generator<number> {
  yield 0;
  const re = /\r\n?|\n/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    yield m.index + m[0].length;
  }
}

// ── breakQuartoMd ─────────────────────────────────────────────────────────────
//
// Ported from Q1 core/lib/break-quarto-md.ts:28.
// The async signature is vestigial (no I/O); kept for interface compatibility.

/**
 * Break a Quarto Markdown document into a list of cells: YAML front-matter,
 * markdown, code cells, raw blocks, and shortcode directives.
 *
 * Mirrors Q1 `breakQuartoMd` (core/lib/break-quarto-md.ts:28).
 *
 * @param src - The document source as a string or MappedString.
 * @param validate - Whether to validate cell YAML options (default false).
 * @param lenient - Whether to be lenient about shortcode parsing (default false).
 * @param startCodeCellRegex - Override the regex that detects code cell start.
 * @returns A QuartoMdChunks object with a `cells` array.
 */
export async function breakQuartoMd(
  src: EitherString,
  validate = false,
  lenient = false,
  startCodeCellRegex?: RegExp,
): Promise<QuartoMdChunks> {
  if (typeof src === "string") {
    src = fromString(src);
  }
  const mappedSrc = src as MappedString;
  const fileName = mappedSrc.fileName;

  const nb: QuartoMdChunks = { cells: [] };

  const yamlRegEx = /^---\s*$/;
  const startCodeCellRegEx =
    startCodeCellRegex ||
    new RegExp(
      "^\\s*(```+)\\s*\\{([=A-Za-z][=A-Za-z0-9._]*)( *[ ,].*)?\\}\\s*$",
    );
  const startCodeRegEx = /^```/;
  const endCodeRegEx = /^\s*(```+)\s*$/;

  let language = "";
  let directiveParams: Shortcode | undefined;
  let cellStartLine = 0;

  let codeStartRange: RangedSubstring | undefined;
  let codeEndRange: RangedSubstring | undefined;

  const lineBuffer: RangedSubstring[] = [];

  const flushLineBuffer = async (
    cell_type: "markdown" | "code" | "raw" | "directive",
    index: number,
  ) => {
    if (lineBuffer.length || cell_type === "code") {
      const mappedChunks: Range[] = [];
      for (const line of lineBuffer) {
        mappedChunks.push(line.range);
      }

      const source = mappedStringFromSource(mappedSrc, mappedChunks, fileName);

      const makeCellType = ():
        | CodeCellType
        | DirectiveCellType
        | "markdown"
        | "raw" => {
        if (cell_type === "code") {
          return { language };
        } else if (cell_type === "directive") {
          return {
            language: "_directive" as const,
            name: directiveParams!.name,
            shortcode: directiveParams!,
          };
        } else {
          return cell_type;
        }
      };

      const cell: QuartoMdCell = {
        cell_type: makeCellType(),
        source,
        sourceOffset: 0,
        sourceStartLine: 0,
        sourceVerbatim: source,
        cellStartLine,
      };

      cellStartLine = index + 1;

      if (cell_type === "code") {
        const { yaml, sourceStartLine } = await partitionCellOptionsMapped(
          language,
          cell.source,
          validate,
          "",
          lenient,
        );

        const breaks = Array.from(lineOffsets(cell.source.value));
        let strUpToLastBreak = "";
        if (sourceStartLine > 0) {
          cell.sourceWithYaml = cell.source;
          cell.source = mappedSubstringOf(cell.source, breaks[sourceStartLine]);

          if (breaks.length > 1) {
            const lastBreak =
              breaks[Math.min(sourceStartLine - 1, breaks.length - 1)];
            strUpToLastBreak = cell.source.value.substring(0, lastBreak);
          } else {
            strUpToLastBreak = cell.source.value;
          }
        } else {
          cell.sourceWithYaml = cell.source;
        }

        const prefix = "```{" + language + "}\n";
        cell.sourceOffset = strUpToLastBreak.length + prefix.length;

        cell.sourceVerbatim = mappedStringFromSource(
          mappedSrc,
          [
            codeStartRange!.range,
            ...mappedChunks,
            codeEndRange!.range,
          ],
          fileName,
        );
        cell.options = yaml;
        cell.sourceStartLine = sourceStartLine;
      } else if (cell_type === "directive") {
        cell.source = mappedStringFromSource(
          mappedSrc,
          mappedChunks.slice(1, -1),
          fileName,
        );
      }

      nb.cells.push(cell);
      lineBuffer.splice(0, lineBuffer.length);
    }
  };

  const tickCount = (s: string): number =>
    Array.from(s.split(" ")[0] || "").filter((c) => c === "`").length;

  let inYaml = false,
    inCodeCell = false,
    inCode = 0;

  const inPlainText = () => !inCodeCell && !inCode && !inYaml;

  const isYamlDelimiter = (
    line: string,
    index: number,
    skipHRs?: boolean,
  ): boolean => {
    if (!yamlRegEx.test(line)) return false;
    if (
      skipHRs &&
      index > 0 &&
      srcLines[index - 1].substring.trim() === "" &&
      index < srcLines.length - 1 &&
      srcLines[index + 1].substring.trim() === ""
    ) {
      return false;
    }
    return true;
  };

  const srcLines = rangedLines(mappedSrc.value, true);

  for (let i = 0; i < srcLines.length; ++i) {
    const line = srcLines[i];
    const directiveMatch = isBlockShortcode(line.substring, true);

    if (isYamlDelimiter(line.substring, i, !inYaml) && !inCodeCell && !inCode) {
      if (inYaml) {
        lineBuffer.push(line);
        await flushLineBuffer("raw", i);
        inYaml = false;
      } else {
        await flushLineBuffer("markdown", i);
        lineBuffer.push(line);
        inYaml = true;
      }
    } else if (inPlainText() && directiveMatch) {
      await flushLineBuffer("markdown", i);
      directiveParams = directiveMatch as Shortcode;
      lineBuffer.push(line);
      await flushLineBuffer("directive", i);
    } else if (startCodeCellRegEx.test(line.substring) && inPlainText()) {
      const m = line.substring.match(startCodeCellRegEx);
      language = (m as string[])[2];
      await flushLineBuffer("markdown", i);
      inCodeCell = true;
      inCode = (m as string[])[1].length;
      codeStartRange = line;
    } else if (
      endCodeRegEx.test(line.substring) &&
      inCode &&
      (line.substring.match(endCodeRegEx)!)[1].length === inCode
    ) {
      if (inCodeCell) {
        codeEndRange = line;
        inCodeCell = false;
        inCode = 0;
        await flushLineBuffer("code", i);
      } else {
        inCode = 0;
        lineBuffer.push(line);
      }
    } else if (startCodeRegEx.test(line.substring) && inCode === 0) {
      inCode = tickCount(line.substring);
      lineBuffer.push(line);
    } else {
      lineBuffer.push(line);
    }
  }

  await flushLineBuffer("markdown", srcLines.length);

  return nb;
}
