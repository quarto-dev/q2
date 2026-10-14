import{createRequire as __cr}from"node:module";const require=__cr(import.meta.url);

// src/engine/index.ts
import { readFileSync as readFileSync2 } from "node:fs";
import { dirname as dirname2 } from "node:path";
import { fileURLToPath as fileURLToPath2 } from "node:url";

// src/engine/authoring.ts
var MARIMO_CELL_REGEX = /^\s{0,3}(`{3,})\s*(?=(?:\{)?\.?((?:python|sql|markdown)(?:\.marimo)?|marimo)(?=[\s}]))(?:(?:python|sql|markdown)\s+\{(?=(?:\.marimo(?=[\s}])|[^}]*\s\.marimo(?=[\s}])))[^}]*\}|\{(?:(?:python|sql|markdown)\.marimo(?=[\s}])|(?:python|sql|markdown)(?=\s)(?=[^}]*\s\.marimo(?=[\s}]))|\.?marimo(?=[\s}]))[^}]*\})\s*$/;
var MARKDOWN_FENCE_REGEX = /^\s{0,3}(`{3,}|~{3,})/;
var MARKDOWN_FENCE_CLOSE_REGEX = /^\s{0,3}(`{3,}|~{3,})\s*$/;
function containsMarimoFence(markdown) {
  let enclosingFence;
  for (const line of markdown.split(/\r?\n/)) {
    if (enclosingFence) {
      const closingFence = line.match(MARKDOWN_FENCE_CLOSE_REGEX)?.[1];
      if (closingFence?.[0] === enclosingFence[0] && closingFence.length >= enclosingFence.length) {
        enclosingFence = void 0;
      }
      continue;
    }
    if (MARIMO_CELL_REGEX.test(line)) return true;
    enclosingFence = line.match(MARKDOWN_FENCE_REGEX)?.[1];
  }
  return false;
}
function isMarimoCell(cell) {
  if (typeof cell.cell_type !== "object" || !("language" in cell.cell_type)) {
    return false;
  }
  const language = cell.cell_type.language;
  if (language !== "python" && language !== "sql" && language !== "markdown" && language !== "marimo" && language !== "python.marimo" && language !== "sql.marimo" && language !== "markdown.marimo") {
    return false;
  }
  const firstLine = cell.sourceVerbatim.value.split("\n", 1)[0] ?? "";
  return MARIMO_CELL_REGEX.test(firstLine);
}
function cellOwnedByMarimo(cell, handledLanguages) {
  if (isMarimoCell(cell)) return true;
  if (handledLanguages === void 0) return false;
  if (typeof cell.cell_type !== "object" || !("language" in cell.cell_type)) {
    return false;
  }
  return cell.cell_type.language === "sql" && !handledLanguages.includes("sql");
}
var MARIMO_CELL_REGEX_WITH_BARE_SQL = withBareSqlAlternative(
  MARIMO_CELL_REGEX
);
function withBareSqlAlternative(regex) {
  const tail = ")\\s*$";
  if (!regex.source.endsWith(tail)) {
    throw new Error("unexpected MARIMO_CELL_REGEX shape");
  }
  const head = regex.source.slice(0, -tail.length);
  return new RegExp(`${head}|\\{sql(?=[\\s}])[^}]*\\}${tail}`);
}

// src/engine/browser-assets.ts
import {
  mkdtempSync,
  readFileSync,
  writeFileSync
} from "node:fs";
import { join } from "node:path";
var EXTENSION_CONFIG = "_extension.yml";
function writeBrowserHeader(extensionDir, tempDir) {
  const version = extensionVersion(extensionDir);
  const assetsDir = join(extensionDir, "assets");
  const browser = readFileSync(join(assetsDir, `browser-v${version}.js`), "utf-8").replace(/<\/script/gi, "<\\/script");
  const styles = readFileSync(join(assetsDir, `islands-bridge-v${version}.css`), "utf-8").replace(/<\/style/gi, "<\\/style");
  const header = [
    `<style data-marimo-islands>${styles}</style>`,
    `<script type="module" data-marimo-islands>${browser}</script>`
  ].join("\n");
  const directory = mkdtempSync(join(tempDir, "marimo-header-"));
  const path = join(directory, "header.html");
  writeFileSync(path, header);
  return path;
}
function extensionVersion(extensionDir) {
  const config = readFileSync(join(extensionDir, EXTENSION_CONFIG), "utf-8");
  const match = config.match(/^version:\s*["']?([^"'\s#]+)["']?\s*$/m);
  if (!match) throw new Error("marimo extension version is missing");
  return match[1];
}

// node_modules/@marimo-team/mdx-marimo/dist/bridge/protocol/index.js
function pageCellPayload(page, cell) {
  return {
    protocolVersion: page.protocolVersion,
    app: page.app,
    cell: pageCell(cell)
  };
}
function pageCellReferencePayload(page, cell) {
  if (!page.app) return pageCellPayload(page, cell);
  return {
    protocolVersion: page.protocolVersion,
    appId: page.app.id,
    cell: pageCell(cell)
  };
}
function projectPageCellPayloads(page) {
  const indices = page.cells.map((cell) => cell.index);
  if (indices.some((index, position) => index !== position)) {
    const expected = page.cells.map((_, index) => index);
    throw new Error(`marimo compiler returned cell indices [${indices.join(", ")}]; expected [${expected.join(", ")}]`);
  }
  const carrierIndex = page.app ? page.cells.find((cell) => cell.options.render.include)?.index : void 0;
  return page.cells.map((cell) => {
    if (!cell.options.render.include) return null;
    return cell.index === carrierIndex ? pageCellPayload(page, cell) : pageCellReferencePayload(page, cell);
  });
}
function encodePageCellPayload(payload) {
  const bytes = new TextEncoder().encode(JSON.stringify(payload));
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}
function isCompiledMarimoPage(value) {
  return isRecord(value) && value.protocolVersion === 2 && (value.app === null || isPageRuntime(value.app)) && isArrayOf(value.cells, isCompiledCell) && isArrayOf(value.diagnostics, isDiagnostic);
}
function isCompiledCell(value) {
  return isPageCell(value) && "output" in value && isCompiledOutput(value.output);
}
function isPageCell(value) {
  return isRecord(value) && isFiniteNumber(value.index) && typeof value.html === "string" && isCellOptions(value.options) && (value.diagnostics === void 0 || isArrayOf(value.diagnostics, isDiagnostic));
}
function isCompiledOutput(value) {
  return value === null || isRecord(value) && typeof value.mimetype === "string" && isJsonValue(value.data) && typeof value.html === "string";
}
function pageCell(cell) {
  const projected = {
    index: cell.index,
    html: cell.html,
    options: cell.options
  };
  if (cell.diagnostics !== void 0) projected.diagnostics = cell.diagnostics;
  return projected;
}
function isPageRuntime(value) {
  return isRecord(value) && typeof value.id === "string" && value.id.length > 0 && isFiniteNumber(value.runtimeCellCount) && isRuntimeAssets(value.assets) && (value.notebookCode === void 0 || typeof value.notebookCode === "string");
}
function isRuntimeAssets(value) {
  return isRecord(value) && isArrayOf(value.moduleScripts, isString) && isArrayOf(value.links, isStringRecord) && (value.headTags === void 0 || isArrayOf(value.headTags, isHeadTag)) && (value.version === void 0 || typeof value.version === "string");
}
function isHeadTag(value) {
  return isRecord(value) && typeof value.tag === "string" && isStringRecord(value.attrs) && (value.text === void 0 || typeof value.text === "string");
}
function isCellOptions(value) {
  return isRecord(value) && (value.language === "python" || value.language === "sql" || value.language === "markdown") && isRenderOptions(value.render) && isRecord(value.execution) && typeof value.execution.enabled === "boolean" && isRecord(value.marimo) && typeof value.marimo.disabled === "boolean" && typeof value.marimo.unparsable === "boolean" && (value.sql === void 0 || isRecord(value.sql) && (value.sql.outputName === void 0 || typeof value.sql.outputName === "string") && (value.sql.engine === void 0 || typeof value.sql.engine === "string")) && (value.name === void 0 || typeof value.name === "string") && (value.column === void 0 || isFiniteNumber(value.column));
}
function isRenderOptions(value) {
  return isRecord(value) && typeof value.source === "boolean" && typeof value.output === "boolean" && typeof value.include === "boolean" && typeof value.editor === "boolean" && typeof value.error === "boolean" && typeof value.serverOutput === "boolean";
}
function isDiagnostic(value) {
  return isRecord(value) && (value.severity === "warning" || value.severity === "error") && typeof value.message === "string" && (value.cellIndex === void 0 || isFiniteNumber(value.cellIndex)) && (value.line === void 0 || isFiniteNumber(value.line));
}
function isJsonValue(value) {
  if (value === null || typeof value === "string" || typeof value === "boolean") return true;
  if (isFiniteNumber(value)) return true;
  if (Array.isArray(value)) return Array.from(value).every(isJsonValue);
  return isRecord(value) && Object.values(value).every(isJsonValue);
}
function isStringRecord(value) {
  return isRecord(value) && Object.values(value).every(isString);
}
function isArrayOf(value, guard) {
  return Array.isArray(value) && Array.from(value).every(guard);
}
function isString(value) {
  return typeof value === "string";
}
function isFiniteNumber(value) {
  return typeof value === "number" && Number.isFinite(value);
}
function isRecord(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

// src/island-element.ts
var MARIMO_ELEMENT_NAME = "marimo-quarto-island";

// src/engine/projection.ts
function projectInteractivePage(page) {
  return projectPageCellPayloads(page).map(
    (payload) => payload ? rawHtml(renderIsland(payload)) : ""
  );
}
async function projectStaticPage(outputs, htmlToMarkdown2) {
  return await Promise.all(
    outputs.map((output) => renderStaticOutput(output, htmlToMarkdown2))
  );
}
function validateProjectionCount(projected, actualCellCount) {
  if (projected.length !== actualCellCount) {
    throw new Error(
      `marimo compiler returned ${projected.length} cells for ${actualCellCount} source blocks`
    );
  }
}
function renderIsland(payload) {
  return [
    `<${MARIMO_ELEMENT_NAME}`,
    ` data-marimo-payload="${encodePageCellPayload(payload)}"`,
    ' data-marimo-payload-encoding="base64url"',
    ' data-marimo-theme-mode="auto"',
    `></${MARIMO_ELEMENT_NAME}>`
  ].join("");
}
async function renderStaticOutput(output, htmlToMarkdown2) {
  let result = "";
  if (output.displayCode && output.code) {
    result += fencedCode(output.code, output.language);
  }
  if (!output.value) return result;
  switch (output.type) {
    case "figure":
      return `${result}![Generated Figure](<${output.value}>)

`;
    case "para":
      return `${result}${output.value}

`;
    case "plain":
      return `${result}${fencedCode(output.value)}`;
    case "blockquote":
      return `${result}> ${output.value.replace(/\r?\n/g, "\n> ")}

`;
    case "html":
      if (/<table[\s>]/i.test(output.value)) {
        return `${result}${rawHtml(output.value)}`;
      }
      return `${result}${await htmlToMarkdown2(output.value)}

`;
  }
}
function fencedCode(value, language = "") {
  const longestRun = Math.max(
    0,
    ...Array.from(value.matchAll(/`+/g), (match) => match[0].length)
  );
  const fence = "`".repeat(Math.max(3, longestRun + 1));
  return `${fence}${language}
${value}
${fence}

`;
}
function rawHtml(value) {
  return fencedCode(value, "{=html}");
}

// src/engine/process.ts
import { spawn } from "node:child_process";
import { mkdtempSync as mkdtempSync2, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join as join2 } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
var defaultCompilerTimeoutMs = 3e5;
var pythonUtf8Environment = {
  PYTHONUTF8: "1",
  PYTHONIOENCODING: "utf-8"
};
async function runMarimoCompiler(quarto2, options) {
  const extensionDir = dirname(fileURLToPath(options.moduleUrl));
  const extractPath = join2(extensionDir, "python", "extract.py");
  let command;
  let args;
  let temporaryDirectory;
  if (options.externalEnv) {
    command = process.env.QUARTO_PYTHON || "python";
    args = [extractPath];
  } else {
    command = "uv";
    const uvCommand = await constructUvCommand(
      quarto2,
      extensionDir,
      options.pyproject
    );
    temporaryDirectory = uvCommand.temporaryDirectory;
    args = [...uvCommand.args, extractPath];
  }
  args.push(
    options.input,
    options.interactive ? "html" : "static",
    options.globalEval ? "yes" : "no"
  );
  if (options.bareSql) args.push("yes");
  let output;
  try {
    output = await executeProcess(quarto2, command, args, options.source);
  } finally {
    if (temporaryDirectory) {
      removeTemporaryDirectory(temporaryDirectory);
    }
  }
  const value = JSON.parse(output);
  if (isPageExecution(value) || isStaticExecution(value)) return value;
  throw new TypeError("marimo compiler returned an invalid execution payload");
}
async function constructUvCommand(quarto2, extensionDir, pyproject) {
  const commandPath = join2(extensionDir, "python", "command.py");
  const temporaryDirectory = mkdtempSync2(join2(tmpdir(), "quarto-marimo-"));
  try {
    const output = await executeProcess(
      quarto2,
      "uv",
      ["run", "--with", "marimo", commandPath],
      pyproject,
      compilerTimeoutMs(),
      {
        TEMP: temporaryDirectory,
        TMP: temporaryDirectory,
        TMPDIR: temporaryDirectory
      }
    );
    const value = JSON.parse(output);
    if (!Array.isArray(value) || !value.every((entry) => typeof entry === "string")) {
      throw new TypeError(
        "marimo dependency resolver returned invalid uv arguments"
      );
    }
    return { args: value, temporaryDirectory };
  } catch (error) {
    removeTemporaryDirectory(temporaryDirectory);
    throw error;
  }
}
function executeProcess(quarto2, command, args, stdin, timeoutMs = compilerTimeoutMs(), env) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      env: { ...process.env, ...pythonUtf8Environment, ...env },
      stdio: ["pipe", "pipe", "pipe"]
    });
    const stdout = [];
    const stderr = [];
    let timedOut = false;
    let settled = false;
    const timeout = setTimeout(() => {
      timedOut = true;
      child.kill("SIGTERM");
    }, timeoutMs);
    const settle = (action) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      action();
    };
    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.stdin.on("error", () => {
    });
    child.on("error", (error) => settle(() => reject(error)));
    child.on("close", (code) => {
      settle(() => {
        if (timedOut) {
          reject(timeoutError(command, args, timeoutMs));
          return;
        }
        const errorText = Buffer.concat(stderr).toString("utf-8");
        if (errorText) quarto2.console.info(errorText.trim());
        if (code !== 0) {
          reject(
            new Error(
              errorText.trim() || `${command} exited with code ${code}`
            )
          );
          return;
        }
        resolve(Buffer.concat(stdout).toString("utf-8"));
      });
    });
    child.stdin.end(stdin, "utf-8");
  });
}
function compilerTimeoutMs() {
  const seconds = Number(process.env.QUARTO_MARIMO_TIMEOUT_SECONDS ?? "");
  return Number.isFinite(seconds) && seconds > 0 ? seconds * 1e3 : defaultCompilerTimeoutMs;
}
function timeoutError(command, args, timeoutMs) {
  return new Error(
    `marimo compilation timed out after ${timeoutMs / 1e3}s while running ${[command, ...args].join(" ")}`
  );
}
function removeTemporaryDirectory(path) {
  rmSync(path, { recursive: true, force: true });
}
function isPageExecution(value) {
  return isRecord2(value) && value.kind === "page" && isCompiledMarimoPage(value.page);
}
function isStaticExecution(value) {
  return isRecord2(value) && value.kind === "static" && Array.isArray(value.outputs) && value.outputs.every(isStaticOutput);
}
function isStaticOutput(value) {
  return isRecord2(value) && (value.type === "html" || value.type === "figure" || value.type === "para" || value.type === "plain" || value.type === "blockquote") && typeof value.value === "string" && typeof value.displayCode === "boolean" && typeof value.code === "string" && typeof value.language === "string";
}
function isRecord2(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

// src/engine/index.ts
var quarto;
var marimoEngineDiscovery = {
  init: (quartoAPI) => {
    quarto = quartoAPI;
  },
  name: "marimo",
  defaultExt: ".qmd",
  defaultYaml: () => ["format: html", "engine: marimo"],
  defaultContent: () => [
    "```{python .marimo}",
    "import marimo as mo",
    "slider = mo.ui.slider(1, 10, 1)",
    "slider",
    "```"
  ],
  validExtensions: () => [".qmd", ".md"],
  // Quarto 1 only: Quarto 2 reads the static `claims-files: []` from
  // `_extension.yml` and never calls this, because it selects marimo by
  // language claim. Quarto 1 needs the sniff to detect documents that use the
  // legacy `python {.marimo}` fence, which its language detection misses.
  claimsFile: (file, extension) => {
    if (![".qmd", ".md"].includes(extension.toLowerCase())) return false;
    try {
      return containsMarimoFence(readFileSync2(file, "utf-8"));
    } catch {
      return false;
    }
  },
  claimsLanguage: (language, firstClass) => {
    if ((language === "python" || language === "sql" || language === "markdown") && firstClass === "marimo") {
      return 2;
    }
    if (language === "python.marimo" || language === "sql.marimo" || language === "markdown.marimo" || language === "marimo") {
      return 1;
    }
    if (language === "sql") return { kind: "interop" };
    return false;
  },
  canFreeze: false,
  generatesFigures: true,
  checkInstallation: async (configuration) => {
    const report = {};
    if (configuration.jsonResult) {
      const render = configuration.jsonResult.render ??= {};
      render.marimo = report;
    }
    const checkRender = async () => {
      const result = await quarto.system.checkRender({
        content: "```{python .marimo}\n1 + 1\n```\n",
        language: "python",
        services: configuration.services
      });
      if (result.error) {
        if (configuration.jsonResult) {
          report.error = result.error.message;
          return;
        }
        throw result.error;
      }
      report.ok = true;
    };
    if (configuration.jsonResult) {
      await checkRender();
    } else {
      const message = "Checking marimo engine render...";
      await quarto.console.withSpinner(
        { message, doneMessage: `${message}OK
` },
        checkRender
      );
    }
  },
  launch: (_context) => ({
    name: marimoEngineDiscovery.name,
    canFreeze: marimoEngineDiscovery.canFreeze,
    markdownForFile(file) {
      return Promise.resolve(quarto.mappedString.fromFile(file));
    },
    target: (file, _quiet, markdown) => {
      const source = markdown ?? quarto.mappedString.fromFile(file);
      return Promise.resolve({
        source: file,
        input: file,
        markdown: source,
        metadata: quarto.markdownRegex.extractYaml(source.value)
      });
    },
    partitionedMarkdown: (file) => Promise.resolve(
      quarto.markdownRegex.partition(readFileSync2(file, "utf-8"))
    ),
    execute: async (options) => {
      const interactive = quarto.format.isHtmlCompatible(options.format);
      const bareSql = options.handledLanguages !== void 0 && !options.handledLanguages.includes("sql");
      const execution = await quarto.console.withSpinner(
        { message: "Executing marimo cells..." },
        async () => await runMarimoCompiler(quarto, {
          moduleUrl: import.meta.url,
          source: options.target.markdown.value,
          input: options.target.input,
          interactive,
          globalEval: options.target.metadata.eval !== false,
          externalEnv: options.target.metadata["external-env"] === true,
          pyproject: String(options.target.metadata.pyproject ?? ""),
          bareSql
        })
      );
      const chunks = await quarto.markdownRegex.breakQuartoMd(
        options.target.markdown,
        false,
        false,
        bareSql ? MARIMO_CELL_REGEX_WITH_BARE_SQL : MARIMO_CELL_REGEX
      );
      const marimoCells = chunks.cells.filter(
        (cell) => cellOwnedByMarimo(cell, options.handledLanguages)
      );
      const projected = execution.kind === "page" ? projectInteractivePage(execution.page) : await projectStaticPage(execution.outputs, htmlToMarkdown);
      validateProjectionCount(projected, marimoCells.length);
      let index = 0;
      const markdown = chunks.cells.map(
        (cell) => cellOwnedByMarimo(cell, options.handledLanguages) ? projected[index++] ?? "" : cell.sourceVerbatim.value
      ).join("");
      const includes = {};
      if (execution.kind === "page") {
        const extensionDir = dirname2(fileURLToPath2(import.meta.url));
        includes["include-in-header"] = [
          writeBrowserHeader(extensionDir, options.tempDir)
        ];
      }
      return {
        engine: "marimo",
        markdown,
        supporting: [],
        filters: [],
        includes
      };
    },
    dependencies: (_options) => Promise.resolve({ includes: {} }),
    postprocess: (_options) => Promise.resolve()
  })
};
async function htmlToMarkdown(html) {
  const result = await quarto.system.pandoc(
    ["-f", "html", "-t", "markdown"],
    html
  );
  if (!result.success) {
    throw new Error(result.stderr || "Pandoc could not convert marimo HTML");
  }
  return result.stdout || "";
}
var index_default = marimoEngineDiscovery;
export {
  index_default as default
};
