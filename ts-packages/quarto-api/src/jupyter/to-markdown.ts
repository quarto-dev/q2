/**
 * @quarto/api/jupyter — to-markdown (the core `jupyterToMarkdown`)
 *
 * Convert a `JupyterNotebook` into a `JupyterToMarkdownResult` (a
 * `cellOutputs` array plus `dependencies` / `htmlPreserve` / `notebookOutputs`
 * / `pandoc`). This is the payload the whole `@quarto/api/jupyter` namespace
 * exists to produce — the Julia engine consumes
 * `result.cellOutputs.map((o) => o.markdown)`.
 *
 * Ported (REWRITE, not extract) from:
 *   external-sources/quarto-cli/src/core/jupyter/jupyter.ts
 *   (`jupyterToMarkdown` ~713-874, `mdFromCodeCell`/`mdFromContentCell`
 *    ~1004-2090, `mdRawOutput`/`mdFormatOutput`/`mdImageOutput` and the
 *    output-formatting helper family). Host I/O routes through the injected
 *    `Pick<PlatformHost,"fs">` — no `Deno.*` / `node:*`.
 *
 * DOCUMENTED SIMPLIFICATIONS vs. Q1 (see the Task 8 report for rationale):
 *   - No notebook fixups / project-type branch (`options.executeOptions` is
 *     not consulted). Fixups are a separate concern (Plan-3 module set).
 *   - No slide/presentation delimiters and no front-matter hold-out.
 *   - ANSI is *stripped only* (P3-16): the `options.toHtml` ansi_up →
 *     `<span>` colorization branch of Q1 is intentionally not ported (a
 *     documented HTML-color gap). Stripping matches Q1 for
 *     latex/markdown/ipynb.
 *   - text/latex dispatch checks `displayDataLatexIsMath` (P3-10): math latex
 *     renders as math, non-math latex becomes a `{=tex}` raw block — Q1's
 *     latex branch was unconditional `{=tex}`.
 *   - No retina PngImage width/height derivation (image bytes are written
 *     as-is).
 *   - The cell-container `#id` echo fires for html/ipynb targets
 *     (`options.toHtml || options.toIpynb`) rather than routing through Q1's
 *     `isHtmlOutput`/`isJatsOutput`/`isIpynbOutput` format helpers.
 */

import { stringify } from "yaml";

import type {
  JupyterCell,
  JupyterCellOptions,
  JupyterCellOutput,
  JupyterCellWithOptions,
  JupyterNotebook,
  JupyterOutput,
  JupyterOutputDisplayData,
  JupyterOutputFigureOptions,
  JupyterOutputStream,
  JupyterToMarkdownOptions,
  JupyterToMarkdownResult,
} from "@quarto/types";
import type { PlatformHost } from "../platform/index.js";

import {
  kApplicationJavascript,
  kApplicationPdf,
  kImageJpeg,
  kImagePng,
  kImageSvg,
  kQuartoMimeType,
  kTextHtml,
  kTextLatex,
} from "./constants.js";
import {
  displayDataIsHtml,
  displayDataIsImage,
  displayDataIsJavascript,
  displayDataIsJson,
  displayDataIsLatex,
  displayDataIsMarkdown,
  displayDataLatexIsMath,
  displayDataMimeType,
  displayDataWithMarkdownMath,
  isCaptionableData,
  isDisplayData,
} from "./display-data.js";
import {
  asHtmlId,
  cellLabel,
  cellLabelValidator,
  resolveCaptions,
  shouldLabelCellContainer,
  shouldLabelOutputContainer,
} from "./labels.js";
import {
  echoFenced,
  hideCell,
  hideCode,
  hideOutput,
  hideWarnings,
  includeCell,
  includeCode,
  includeOutput,
  includeWarnings,
} from "./tags.js";
import { parseCellOptions } from "./cell-options.js";
import { removeAndPreserveHtml } from "./preserve.js";
import { widgetDependencies } from "./widgets.js";
import { pandocAutoIdentifier } from "./pandoc-id.js";
import { trimEmptyLines } from "../text/index.js";

// ─── local mime constants (raw-cell dispatch) ──────────────────────────────
const kRestructuredText = "text/restructuredtext";
const kApplicationRtf = "application/rtf";
const kTextPlain = "text/plain";

// ─── cell-option / metadata key constants (ported from Q1 config/constants) ──
const kEcho = "echo";
const kOutput = "output";
const kCellLabel = "label";
const kCellClasses = "classes";
const kCellPanel = "panel";
const kCellColumn = "column";
const kCellFigColumn = "fig-column";
const kCellTblColumn = "tbl-column";
const kCellFigCap = "fig-cap";
const kCapLoc = "cap-location";
const kFigCapLoc = "fig-cap-location";
const kTblCapLoc = "tbl-cap-location";
const kCellFigAlign = "fig-align";
const kCellFigScap = "fig-scap";
const kCellFigLink = "fig-link";
const kCellFigEnv = "fig-env";
const kCellFigPos = "fig-pos";
const kCellFigAlt = "fig-alt";
const kCellLstLabel = "lst-label";
const kCellLstCap = "lst-cap";
const kCodeFold = "code-fold";
const kCodeLineNumbers = "code-line-numbers";
const kCodeSummary = "code-summary";
const kCodeOverflow = "code-overflow";
const kHtmlTableProcessing = "html-table-processing";
const kCellOutWidth = "out-width";
const kCellOutHeight = "out-height";
const kCellWidth = "width";
const kCellHeight = "height";
const kCellMdIndent = "md-indent";
const kCellRawMimeType = "raw_mimetype";
const kQuartoOutputOrder = "quarto_order";
const kQuartoOutputDisplay = "quarto_display";
const kCellLinesToNext = "lines_to_next_cell";

// keys that must NOT be forwarded onto the cell div as attributes
const kJupyterCellInternalOptionKeys = [
  "eval",
  kEcho,
  "warning",
  "error",
  kOutput,
  "include",
  kCellLabel,
  kCellClasses,
  kCellPanel,
  kCellColumn,
  kCellFigCap,
  "fig-subcap",
  kCellFigScap,
  kFigCapLoc,
  kTblCapLoc,
  kCapLoc,
  kCellFigColumn,
  kCellTblColumn,
  kCellFigLink,
  kCellFigAlign,
  kCellFigAlt,
  kCellFigEnv,
  kCellFigPos,
  kCellLstLabel,
  kCellLstCap,
  kCellOutWidth,
  kCellOutHeight,
  kCellMdIndent,
  kCodeFold,
  kCodeLineNumbers,
  kCodeSummary,
  kCodeOverflow,
  kHtmlTableProcessing,
];
const kJupyterCellStandardMetadataKeys = [
  "collapsed",
  "autoscroll",
  "deletable",
  "format",
  "name",
];
const kJupyterCellThirdPartyMetadataKeys = [
  "id",
  "colab",
  "colab_type",
  "outputId",
  kCellLinesToNext,
  "language",
];

// ─── local error-output shape (Q1 defines this in jupyter.ts, not @quarto/types) ─
interface JupyterOutputError extends JupyterOutput {
  ename: string;
  evalue: string;
  traceback: string[];
}

function isDisplayDataType(
  output: JupyterOutput,
  options: JupyterToMarkdownOptions,
  checkFn: (mimeType: string) => boolean,
): boolean {
  if (isDisplayData(output)) {
    const mimeType = displayDataMimeType(output, options);
    if (mimeType && checkFn(mimeType)) {
      return true;
    }
  }
  return false;
}
function isImage(output: JupyterOutput, options: JupyterToMarkdownOptions): boolean {
  return isDisplayDataType(output, options, displayDataIsImage);
}
function isMarkdown(output: JupyterOutput, options: JupyterToMarkdownOptions): boolean {
  return isDisplayDataType(output, options, displayDataIsMarkdown);
}
function isWarningOutput(output: JupyterOutput): boolean {
  return (
    output.output_type === "stream" &&
    (output as JupyterOutputStream).name === "stderr"
  );
}

// ─── portable helpers (no Deno.* / node:*) ──────────────────────────────────

/** Strip ANSI escape / control sequences. Portable replacement for Deno's
 * `colors.stripAnsiCode` (the canonical `ansi-regex` pattern). */
// eslint-disable-next-line no-control-regex
const ANSI_PATTERN =
  // eslint-disable-next-line no-control-regex
  /[\u001B\u009B][[\]()#;?]*(?:(?:(?:[a-zA-Z\d]*(?:;[a-zA-Z\d]*)*)?\u0007)|(?:(?:\d{1,4}(?:;\d{0,4})*)?[\dA-PR-TZcf-ntqry=><~]))/g;
function stripAnsiCode(text: string): string {
  return text.replace(ANSI_PATTERN, "");
}

/** Portable base64 → bytes (Q1 removes embedded newlines before decoding). */
function base64ToBytes(b64: string): Uint8Array {
  const clean = b64.replace(/\n/g, "");
  const binary = atob(clean);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/** UTF-8-safe base64 encode (for notebookOutputs widget metadata). */
function encodeBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (const b of bytes) {
    binary += String.fromCharCode(b);
  }
  return btoa(binary);
}

/** Join a base dir and a (possibly figures-relative) path without node:path. */
function joinPath(base: string, rel: string): string {
  if (!base) {
    return rel;
  }
  return base.endsWith("/") ? base + rel : base + "/" + rel;
}

function extensionForMimeImageType(mimeType: string): string {
  switch (mimeType) {
    case kImagePng:
      return "png";
    case kImageJpeg:
      return "jpeg";
    case kImageSvg:
      return "svg";
    case kApplicationPdf:
      return "pdf";
    default:
      return "bin";
  }
}

function srcAsLines(src: string | string[]): string[] {
  return typeof src === "string" ? [src] : [...src];
}

const countTicks = (code: string[]): number => {
  const countLeadingTicks = (s: string): number => {
    const m = s.match(/^\s*`+/);
    return m ? m[0].length : 0;
  };
  return Math.max(0, ...code.map((s) => countLeadingTicks(s)));
};
const ticksForCode = (code: string[]): string => {
  return "`".repeat(Math.max(3, countTicks(code) + 1));
};

function outputTypeCssClass(outputType: string): string {
  if (["display_data", "execute_result"].includes(outputType)) {
    outputType = "display";
  }
  return `cell-output-${outputType}`;
}

// ─── generic output-formatting helpers (Task 9 imports these) ───────────────

/** Emit `source` as a fenced raw block for the given pandoc `format`
 * (`` ```{=<format>} ``). Ported from Q1 `mdFormatOutput`. */
export function mdFormatOutput(format: string, source: string[]): string {
  const ticks = ticksForCode(source);
  return mdEnclosedOutput(ticks + "{=" + format + "}", source, ticks);
}

/** Dispatch a raw output (from a raw cell / percent raw block) by MIME type.
 * Ported from Q1 `mdRawOutput` — returns `undefined` for an unknown type. */
export function mdRawOutput(mimeType: string, source: string[]): string | undefined {
  switch (mimeType) {
    case kTextHtml:
      return mdHtmlOutput(source);
    case kTextLatex:
      return mdLatexOutput(source);
    case kRestructuredText:
      return mdFormatOutput("rst", source);
    case kApplicationRtf:
      return mdFormatOutput("rtf", source);
    case kApplicationJavascript:
      return mdScriptOutput(mimeType, source);
    default:
      return undefined;
  }
}

function mdMarkdownOutput(md: string[]): string {
  return md.join("") + "\n";
}
function mdLatexOutput(latex: string[]): string {
  return mdFormatOutput("tex", latex);
}
function mdHtmlOutput(html: string[]): string {
  return mdFormatOutput("html", html);
}
function mdScriptOutput(mimeType: string, script: string[]): string {
  const scriptTag = [`<script type="${mimeType}">\n`, ...script, "\n</script>"];
  return mdHtmlOutput(scriptTag);
}
function mdJsonOutput(
  mimeType: string,
  json: Record<string, unknown>,
  options: JupyterToMarkdownOptions,
): string {
  if (options.toIpynb) {
    return mdCodeOutput([JSON.stringify(json)], "json");
  } else {
    return mdScriptOutput(mimeType, [JSON.stringify(json)]);
  }
}
function mdCodeOutput(code: string[], clz?: string): string {
  const ticks = ticksForCode(code);
  const open = ticks + (clz ? `{.${clz}}` : "");
  return mdEnclosedOutput(open, code, ticks);
}
function mdEnclosedOutput(begin: string, text: string[], end: string): string {
  const output = text.join("");
  return [begin + "\n", output + (output.endsWith("\n") ? "" : "\n"), end + "\n"].join("");
}

function mdEnsureTrailingNewline(source: string[]): string[] {
  if (source.length > 0 && !source[source.length - 1].endsWith("\n")) {
    return source
      .slice(0, source.length - 1)
      .concat([source[source.length - 1] + "\n"]);
  }
  return source;
}

// ─── output producers ───────────────────────────────────────────────────────

function mdWarningOutput(msg: string): string {
  return mdOutputStream({
    output_type: "stream",
    name: "stderr",
    text: [msg],
  } as JupyterOutputStream);
}

/** stream output (stdout/stderr) — ANSI stripped (P3-16). */
function mdOutputStream(output: JupyterOutputStream): string {
  let text: string[] = typeof output.text === "string" ? [output.text] : output.text;

  // trim off the "<ipython-input...>" source-line prefix on warnings
  if (output.name === "stderr" && text[0]) {
    const firstLine = text[0].replace(/<ipython-input.*?>:\d+:\s+/, "");
    text = [firstLine, ...text.slice(1)];
  }

  return mdCodeOutput(text.map(stripAnsiCode));
}

/** error output — traceback ANSI stripped (P3-16). */
function mdOutputError(output: JupyterOutputError): string {
  const traceback = (output.traceback || []).join("\n");
  const header = output.ename + ": " + output.evalue;
  const body = output.traceback && output.traceback.length > 0
    ? header + "\n" + traceback
    : header;
  return mdCodeOutput([stripAnsiCode(body)]);
}

function cleanJupyterOutputDisplayData(output: JupyterOutput): JupyterOutputDisplayData {
  const raw = output as unknown as Record<string, unknown>;
  const outputData: { [mimeType: string]: unknown } = {};
  for (const [key, value] of Object.entries(raw.data as { [mimeType: string]: unknown })) {
    const strValue = typeof value === "string"
      ? [value]
      : Array.isArray(value) && value.every((x) => typeof x === "string")
        ? (value as string[])
        : undefined;
    outputData[key] = strValue === undefined ? value : strValue;
  }
  return {
    ...(output as JupyterOutputDisplayData),
    data: outputData,
    metadata: raw.metadata as { [mimeType: string]: Record<string, unknown> },
    noCaption: raw.noCaption as boolean | undefined,
  };
}

function mdImageOutput(
  host: Pick<PlatformHost, "fs">,
  label: string | null,
  caption: string | null,
  filename: string,
  mimeType: string,
  output: JupyterOutputDisplayData,
  options: JupyterToMarkdownOptions,
  figureOptions: JupyterOutputFigureOptions,
): string {
  const data = output.data[mimeType] as string[] | string;
  const metadata = output.metadata?.[mimeType];

  function metadataValue<T>(key: string, defaultValue?: T): T | undefined {
    if (metadata) {
      return (metadata[key] as T) ? (metadata[key] as T) : defaultValue;
    }
    return defaultValue;
  }
  const width = metadataValue<number>(kCellOutWidth) ?? metadataValue<number>(kCellWidth, 0);
  const height = metadataValue<number>(kCellOutHeight) ?? metadataValue<number>(kCellHeight, 0);
  const alt = caption || "";

  const ext = extensionForMimeImageType(mimeType);
  const imageFile = options.assets.figures_dir + "/" + filename + "." + ext;

  const imageText = Array.isArray(data) ? data.join("") : (data as string).trim();
  const outputFile = joinPath(options.assets.base_dir, imageFile);

  // ensure the target directory exists (no-op in mock hosts)
  const lastSlash = outputFile.lastIndexOf("/");
  if (lastSlash > 0) {
    host.fs.ensureDir(outputFile.slice(0, lastSlash));
  }

  if (mimeType !== kImageSvg || !/<svg/.test(imageText)) {
    // base64-decode → bytes for png/jpeg/pdf (and base64-encoded svg)
    host.fs.writeFileSync(outputFile, base64ToBytes(imageText));
  } else {
    host.fs.writeFileSync(outputFile, imageText);
  }

  const kFigOptions = [kCellFigAlign, kCellFigEnv, kCellFigAlt, kCellFigPos, kCellFigScap];

  let image = `![${alt}](${imageFile})`;
  if (
    label ||
    width ||
    height ||
    Object.keys(figureOptions).some((option) => kFigOptions.includes(option))
  ) {
    image += "{";
    if (label) {
      image += `${label} `;
    }
    if (width) {
      image += `width=${width} `;
    }
    if (height) {
      image += `height=${height} `;
    }
    for (const attrib of kFigOptions) {
      const value = (figureOptions as Record<string, unknown>)[attrib];
      if (value) {
        image += `${attrib}='${String(value).replaceAll("'", "\\'")}' `;
      }
    }
    image = image.trimEnd() + "}";
  }

  if (figureOptions[kCellFigLink]) {
    image = `[${image}](${figureOptions[kCellFigLink]})`;
  }

  return mdMarkdownOutput([image]);
}

function mdOutputDisplayData(
  host: Pick<PlatformHost, "fs">,
  label: string | null,
  caption: string | null,
  filename: string,
  output: JupyterOutputDisplayData,
  options: JupyterToMarkdownOptions,
  figureOptions: JupyterOutputFigureOptions,
): string {
  const mimeType = displayDataMimeType(output, options);
  if (mimeType) {
    if (displayDataIsImage(mimeType)) {
      return mdImageOutput(host, label, caption, filename, mimeType, output, options, figureOptions);
    } else if (displayDataIsMarkdown(mimeType)) {
      return mdMarkdownOutput(output.data[mimeType] as string[]);
    } else if (displayDataIsLatex(mimeType)) {
      // P3-10: math latex renders as math; non-math latex → {=tex} raw block.
      // (Under toLatex the upstream displayDataWithMarkdownMath hoist is
      // skipped, so math latex still arrives here — render it as math rather
      // than emitting an unconditional {=tex}.)
      const latex = output.data[mimeType] as string[];
      if (displayDataLatexIsMath(latex)) {
        return mdMarkdownOutput(latex);
      }
      return mdLatexOutput(latex);
    } else if (displayDataIsHtml(mimeType)) {
      return mdHtmlOutput(output.data[mimeType] as string[]);
    } else if (displayDataIsJson(mimeType)) {
      const json = output.data[mimeType] as Record<string, unknown>;
      json[kQuartoMimeType] = mimeType;
      return mdJsonOutput(mimeType, json, options);
    } else if (displayDataIsJavascript(mimeType)) {
      return mdScriptOutput(mimeType, output.data[mimeType] as string[]);
    } else if (mimeType === kTextPlain) {
      const dataVal = output.data[mimeType] as unknown;
      if (!Array.isArray(dataVal) || dataVal.some((s) => typeof s !== "string")) {
        return mdWarningOutput(
          `Unable to process text plain output data which does not appear to be plain text: ${JSON.stringify(dataVal)}`,
        );
      }
      const plainLines = dataVal as string[];
      // pandas emits html tables as text/plain wrapped in single quotes
      if (
        plainLines.length === 1 &&
        plainLines[0].startsWith("'<table") &&
        plainLines[0].endsWith("</table>'")
      ) {
        plainLines[0] = plainLines[0].slice(1, -1);
        return mdMarkdownOutput(plainLines);
      }
      return mdCodeOutput(plainLines.map(stripAnsiCode));
    }
  }

  return mdWarningOutput(
    "Unable to display output for mime type(s): " + Object.keys(output.data).join(", "),
  );
}

function isDiscardableTextExecuteResult(output: JupyterOutput, haveImage: boolean): boolean {
  if (output.output_type === "execute_result") {
    const data = (output as JupyterOutputDisplayData).data;
    if (Object.keys(data).length === 1) {
      const textPlain = data[kTextPlain] as string[] | undefined;
      if (textPlain && textPlain.length) {
        if (haveImage && textPlain.length === 1) {
          return /^([<([]).*?([>)\]])$/.test(textPlain[0].trim());
        }
        return ["[<matplotlib", "<matplotlib", "<seaborn.", "<ggplot:"].some((s) =>
          textPlain[0].startsWith(s),
        );
      }
    }
  }
  return false;
}

function hasLayoutOptions(cell: JupyterCellWithOptions): boolean {
  return Object.keys(cell.options).some((key) => key.startsWith("layout"));
}

// ─── cell → markdown ────────────────────────────────────────────────────────

/** Upgrade a raw `JupyterCell` into a `JupyterCellWithOptions`: parse the
 * leading `#| ...` option block, strip it off `source`, derive a stable
 * non-empty `id`. */
function toCellWithOptions(
  cell: JupyterCell,
  cellIndex: number,
  language: string,
): JupyterCellWithOptions {
  const fullSource = srcAsLines(cell.source);
  const { options, optionsSource } = parseCellOptions(fullSource, language);
  const codeSource = fullSource.slice(optionsSource.length);

  const withOptions: JupyterCellWithOptions = {
    ...cell,
    source: codeSource,
    options: options as JupyterCellOptions,
    optionsSource,
    id: "",
  };

  const rawLabel = cellLabel(withOptions);
  withOptions.id = rawLabel ? asHtmlId(rawLabel) : `cell-${cellIndex}`;
  return withOptions;
}

function mdFromRawCell(cell: JupyterCellWithOptions): string[] {
  const mimeType = cell.metadata?.[kCellRawMimeType] as string | undefined;
  if (mimeType) {
    const rawOutput = mdRawOutput(mimeType, srcAsLines(cell.source));
    if (rawOutput) {
      return [rawOutput];
    }
  }
  // otherwise pass the raw source through as-is
  return mdEnsureTrailingNewline(srcAsLines(cell.source));
}

function mdFromCodeCell(
  host: Pick<PlatformHost, "fs">,
  cell: JupyterCellWithOptions,
  cellIndex: number,
  options: JupyterToMarkdownOptions,
): string[] {
  // bail if we aren't including this cell
  if (!includeCell(cell, options)) {
    return [];
  }

  const haveImage = !!cell.outputs?.some((output) => isImage(output, options));

  // filter + transform outputs
  const outputs = (cell.outputs || [])
    .filter((output) => {
      if (
        output.output_type === "stream" &&
        (output as JupyterOutputStream).name === "stderr" &&
        !includeWarnings(cell, options)
      ) {
        return false;
      }
      if (isDiscardableTextExecuteResult(output, haveImage)) {
        return false;
      }
      return true;
    })
    .map((output) => {
      // convert text/latex math to markdown when not targeting latex
      if (!options.toLatex && isDisplayData(output) && output.data[kTextLatex]) {
        return displayDataWithMarkdownMath(output as JupyterOutputDisplayData);
      }
      return output;
    });

  // redact if no source and no output
  if (!cell.source.length && !outputs.length) {
    return [];
  }

  // output: asis => raw markup with no enclosures
  const asis =
    cell.options[kOutput] === "asis" ||
    (options.execute[kOutput] === "asis" && cell.options[kOutput] === undefined);

  const md: string[] = [];
  const divMd: string[] = [`::: {`];

  const cellOptionsFilter = kJupyterCellInternalOptionKeys.concat(
    kJupyterCellStandardMetadataKeys,
    kJupyterCellThirdPartyMetadataKeys,
  );

  const label = cellLabel(cell);
  const labelCellContainer = shouldLabelCellContainer(cell, outputs, options);
  if (label && labelCellContainer) {
    divMd.push(`${label} `);
  } else if ((options.toHtml || options.toIpynb) && cell.id) {
    divMd.push(`#${cell.id} `);
  }

  let { cellCaption, outputCaptions } = resolveCaptions(cell);
  outputCaptions = outputCaptions.map((caption) => caption.trim().replaceAll("\n", " "));

  divMd.push(`.cell `);
  if (hideCell(cell, options)) {
    divMd.push(`.hidden `);
  }

  // css classes
  const cellClasses = cell.options[kCellClasses] || new Array<string>();
  const classes = Array.isArray(cellClasses) ? cellClasses : [cellClasses];
  if (typeof cell.options[kCellPanel] === "string") {
    classes.push(`panel-${cell.options[kCellPanel]}`);
  }
  if (typeof cell.options[kCellColumn] === "string") {
    classes.push(`column-${cell.options[kCellColumn]}`);
  }
  if (typeof cell.options[kCellFigColumn] === "string") {
    classes.push(`fig-column-${cell.options[kCellFigColumn]}`);
  }
  if (typeof cell.options[kCellTblColumn] === "string") {
    classes.push(`tbl-column-${cell.options[kCellTblColumn]}`);
  }
  if (typeof cell.options[kCapLoc] === "string") {
    classes.push(`caption-${cell.options[kFigCapLoc]}`);
  }
  if (typeof cell.options[kFigCapLoc] === "string") {
    classes.push(`fig-cap-location-${cell.options[kFigCapLoc]}`);
  }
  if (typeof cell.options[kTblCapLoc] === "string") {
    classes.push(`tbl-cap-location-${cell.options[kTblCapLoc]}`);
  }
  if (classes.length > 0) {
    const classText = classes
      .map((clz: unknown) => {
        const s = String(clz);
        return s.startsWith(".") ? s : "." + s;
      })
      .join(" ");
    divMd.push(classText + " ");
  }

  // forward other attributes (from options yaml + cell metadata)
  const cellOptions: Record<string, unknown> = { ...cell.metadata, ...cell.options };
  let forwardedAttrs = false;
  for (const key of Object.keys(cellOptions)) {
    if (!cellOptionsFilter.includes(key.toLowerCase())) {
      let value = cellOptions[key];
      if (value !== undefined) {
        if (typeof value !== "string") {
          value = JSON.stringify(value);
        }
        value = (value as string).replaceAll("'", `\\'`);
        divMd.push(`${key}='${value}' `);
        forwardedAttrs = true;
      }
    }
  }

  const needCell =
    (label && labelCellContainer) ||
    classes.length > 0 ||
    forwardedAttrs ||
    cellCaption !== undefined ||
    outputCaptions.length > 0;

  if (typeof cell.execution_count === "number") {
    divMd.push(`execution_count=${cell.execution_count} `);
  }

  const divBeginMd = divMd.join("").replace(/ $/, "").concat("}\n");

  // write code if appropriate
  if (includeCode(cell, options) || options.preserveCodeCellYaml) {
    const fenced = echoFenced(cell, options);
    const ticks = "`".repeat(Math.max(countTicks(srcAsLines(cell.source)) + 1, fenced ? 4 : 3));

    md.push(ticks + " {");
    if (!options.preserveCodeCellYaml) {
      if (typeof cell.options[kCellLstLabel] === "string") {
        let lst = cell.options[kCellLstLabel] as string;
        if (!lst.startsWith("#")) {
          lst = "#" + lst;
        }
        md.push(lst + " ");
      }
      if (!fenced) {
        md.push("." + (cellOptions.language || options.language));
      }
      md.push(" .cell-code");
      if (hideCode(cell, options)) {
        md.push(" .hidden");
      }
      if (cell.options[kCodeOverflow] === "wrap") {
        md.push(" .code-overflow-wrap");
      } else if (cell.options[kCodeOverflow] === "scroll") {
        md.push(" .code-overflow-scroll");
      }
      if (typeof cell.options[kCellLstCap] === "string") {
        md.push(` lst-cap="${cell.options[kCellLstCap]}"`);
      }
      if (typeof cell.options[kCodeFold] !== "undefined") {
        md.push(` code-fold="${cell.options[kCodeFold]}"`);
      }
      if (typeof cell.options[kCodeSummary] !== "undefined") {
        md.push(` code-summary="${cell.options[kCodeSummary]}"`);
      }
      if (typeof cell.options[kCodeLineNumbers] !== "undefined") {
        md.push(` code-line-numbers="${cell.options[kCodeLineNumbers]}"`);
      }
    }
    md.push("}\n");

    let source = srcAsLines(cell.source);
    if (fenced) {
      const optionsSource = cell.optionsSource.filter(
        (line) => line.search(/\|\s+echo:\s+fenced\s*$/) === -1,
      );
      source = optionsSource.length > 0
        ? trimEmptyLines(source, "trailing")
        : trimEmptyLines(source, "all");
      source.unshift(...optionsSource);
      source.unshift("```{{" + options.language + "}}\n");
      source.push("\n```\n");
    } else if (cell.optionsSource.length > 0) {
      source = trimEmptyLines(source, "leading");
    }
    if (options.preserveCodeCellYaml) {
      md.push(...cell.optionsSource);
    }
    md.push(...source, "\n");
    md.push(ticks + "\n");
  }

  // write output if appropriate
  if (includeOutput(cell, options)) {
    const labelName = label
      ? label.replace(/^#/, "").replaceAll(":", "-")
      : "cell-" + (cellIndex + 1);
    const outputName = `${options.outputPrefix ? options.outputPrefix + "-" : ""}${pandocAutoIdentifier(
      labelName,
      true,
    )}-output`;
    let nextOutputSuffix = 1;

    const sortedOutputs = outputs
      .map((value, index) => ({ index, output: value }))
      .sort((a, b) => {
        const aIdx =
          a.output.metadata?.[kQuartoOutputOrder] !== undefined
            ? (a.output.metadata?.[kQuartoOutputOrder] as unknown as number)
            : Number.MAX_SAFE_INTEGER;
        const bIdx =
          b.output.metadata?.[kQuartoOutputOrder] !== undefined
            ? (b.output.metadata?.[kQuartoOutputOrder] as unknown as number)
            : Number.MAX_SAFE_INTEGER;
        return aIdx - bIdx;
      });

    for (const { index, output } of sortedOutputs) {
      const outputLabel =
        label && labelCellContainer && isDisplayData(output)
          ? label + "-" + nextOutputSuffix++
          : label;

      if ((output.metadata?.[kQuartoOutputDisplay] as unknown) === false) {
        continue;
      }

      if (!asis) {
        md.push("\n::: {");
        if (outputLabel && shouldLabelOutputContainer(output, cell.options, options)) {
          md.push(outputLabel + " ");
        }
        md.push(".cell-output ");
        if (output.output_type === "stream") {
          md.push(`.cell-output-${(output as JupyterOutputStream).name}`);
        } else {
          md.push(`.${outputTypeCssClass(output.output_type)}`);
        }
        if (isMarkdown(output, options)) {
          md.push(` .${outputTypeCssClass("markdown")}`);
        }
        if (hideOutput(cell, options) || (isWarningOutput(output) && hideWarnings(cell, options))) {
          md.push(` .hidden`);
        }
        if (typeof output.execution_count === "number") {
          md.push(` execution_count=${output.execution_count}`);
        }
        if (cell.options[kHtmlTableProcessing] === "none") {
          md.push(" html-table-processing=none");
        }
        md.push("}\n");
      }

      // for latex, default fig-pos='H' when code is included w/ the figure
      if (
        options.toLatex &&
        !options.figPos &&
        !cell.options[kCellFigPos] &&
        !hasLayoutOptions(cell) &&
        includeCode(cell, options)
      ) {
        cell.options[kCellFigPos] = "H";
      }

      // broadcast figure options
      const figureOptions: JupyterOutputFigureOptions = {};
      const broadcastFigureOption = (name: string): unknown => {
        const value = cell.options[name];
        if (value) {
          return Array.isArray(value) ? value[index] : value;
        }
        return null;
      };
      figureOptions[kCellFigAlign] = broadcastFigureOption(kCellFigAlign);
      figureOptions[kCellFigScap] = broadcastFigureOption(kCellFigScap);
      figureOptions[kCellFigLink] = broadcastFigureOption(kCellFigLink);
      figureOptions[kCellFigEnv] = broadcastFigureOption(kCellFigEnv);
      figureOptions[kCellFigPos] = broadcastFigureOption(kCellFigPos);
      figureOptions[kCellFigAlt] = broadcastFigureOption(kCellFigAlt);

      if (output.output_type === "stream") {
        const stream = output as JupyterOutputStream;
        if (asis && stream.name === "stdout") {
          const text = typeof stream.text === "string" ? [stream.text] : stream.text;
          md.push(text.join(""));
        } else {
          md.push(mdOutputStream(stream));
        }
      } else if (output.output_type === "error") {
        md.push(mdOutputError(output as JupyterOutputError));
      } else if (isDisplayData(output)) {
        const fixedOutput = cleanJupyterOutputDisplayData(output);
        if (Object.keys(fixedOutput.data).length > 0) {
          const caption = isCaptionableData(output) ? outputCaptions.shift() || null : null;
          md.push(
            mdOutputDisplayData(
              host,
              outputLabel,
              caption,
              outputName + "-" + (index + 1),
              fixedOutput,
              options,
              figureOptions,
            ),
          );
          if (caption && !isImage(output, options)) {
            md.push(`\n${caption}\n`);
          }
        }
      } else {
        throw new Error("Unexpected output type " + output.output_type);
      }

      if (!asis) {
        md.push(`:::\n`);
      }
    }
  }

  // write md w/ div enclosure (only if there is content)
  if (md.length > 0 && (needCell || !asis)) {
    md.unshift(divBeginMd);
    if (cellCaption) {
      md.push("\n" + cellCaption + "\n");
    }
    md.push(":::\n");
  }

  // lines to next cell
  const linesToNext = (cell.metadata[kCellLinesToNext] as number) || 1;
  md.push("\n".repeat(linesToNext));

  // md-indent
  if (cell.options[kCellMdIndent]) {
    const indent = String(cell.options[kCellMdIndent]);
    const mdWithIndent = md
      .join("")
      .split("\n")
      .map((line) => indent + line)
      .join("\n");
    md.splice(0, md.length);
    md.push(mdWithIndent);
  }

  return md;
}

// ─── public entry point ─────────────────────────────────────────────────────

/**
 * Convert a Jupyter notebook to markdown cell-outputs. Async to match the
 * `@quarto/api/jupyter` namespace `toMarkdown` contract (the body itself does
 * no awaiting — ANSI is stripped synchronously).
 */
export async function jupyterToMarkdown(
  host: Pick<PlatformHost, "fs">,
  nb: JupyterNotebook,
  options: JupyterToMarkdownOptions,
): Promise<JupyterToMarkdownResult> {
  // Pre-walk pass (ORDER IS LOAD-BEARING): both widgetDependencies and
  // removeAndPreserveHtml mutate `nb` in place, and the cell walk depends on
  // those mutations having already happened.
  const isHtml = options.toHtml && !options.toIpynb;
  const dependencies = isHtml ? widgetDependencies(nb) : undefined;
  const htmlPreserve = isHtml ? removeAndPreserveHtml(nb) : undefined;

  const cellOutputs: JupyterCellOutput[] = [];
  const validateCellLabel = cellLabelValidator();
  let codeCellIndex = 0;

  const language = (nb.metadata.kernelspec?.language || options.language || "python").toLowerCase();

  for (let i = 0; i < nb.cells.length; i++) {
    const cell = toCellWithOptions(nb.cells[i], i, language);

    // duplicate-label guard
    validateCellLabel(cell);

    const md: string[] = [];
    switch (cell.cell_type) {
      case "markdown":
        md.push(srcAsLines(cell.source).join(""));
        break;
      case "raw":
        md.push(...mdFromRawCell(cell));
        break;
      case "code":
        md.push(...mdFromCodeCell(host, cell, ++codeCellIndex, options));
        break;
      default:
        throw new Error("Unexpected cell type " + (cell as JupyterCell).cell_type);
    }

    md.push("\n");

    cellOutputs.push({
      id: cell.id,
      markdown: md.join(""),
      metadata: cell.metadata,
      options: cell.options,
    });
  }

  // notebook-level YAML suffix, only when targeting ipynb
  let notebookOutputs: { prefix?: string; suffix?: string } | undefined = undefined;
  if (options.toIpynb) {
    const widgets = nb.metadata.widgets
      ? encodeBase64(JSON.stringify(nb.metadata.widgets))
      : undefined;
    const jupyterMetadata = { jupyter: { ...nb.metadata, widgets } };
    const yamlText = stringify(jupyterMetadata, { lineWidth: 0 });
    notebookOutputs = { suffix: "---\n" + yamlText + "---\n" };
  }

  return {
    cellOutputs,
    notebookOutputs,
    dependencies,
    htmlPreserve,
    // Exact Q1 parity: jupyterToMarkdown never populates `pandoc`.
    pandoc: undefined,
  };
}
