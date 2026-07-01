// deno:https://jsr.io/@std/path/1.0.8/_os.ts
var isWindows = globalThis.Deno?.build.os === "windows" || globalThis.navigator?.platform?.startsWith("Win") || globalThis.process?.platform?.startsWith("win") || false;

// deno:https://jsr.io/@std/path/1.0.8/_common/assert_path.ts
function assertPath(path) {
  if (typeof path !== "string") {
    throw new TypeError(`Path must be a string, received "${JSON.stringify(path)}"`);
  }
}

// deno:https://jsr.io/@std/path/1.0.8/_common/strip_trailing_separators.ts
function stripTrailingSeparators(segment, isSep) {
  if (segment.length <= 1) {
    return segment;
  }
  let end = segment.length;
  for (let i = segment.length - 1; i > 0; i--) {
    if (isSep(segment.charCodeAt(i))) {
      end = i;
    } else {
      break;
    }
  }
  return segment.slice(0, end);
}

// deno:https://jsr.io/@std/path/1.0.8/_common/constants.ts
var CHAR_UPPERCASE_A = 65;
var CHAR_LOWERCASE_A = 97;
var CHAR_UPPERCASE_Z = 90;
var CHAR_LOWERCASE_Z = 122;
var CHAR_DOT = 46;
var CHAR_FORWARD_SLASH = 47;
var CHAR_BACKWARD_SLASH = 92;
var CHAR_COLON = 58;

// deno:https://jsr.io/@std/path/1.0.8/posix/_util.ts
function isPosixPathSeparator(code) {
  return code === CHAR_FORWARD_SLASH;
}

// deno:https://jsr.io/@std/path/1.0.8/windows/_util.ts
function isPosixPathSeparator2(code) {
  return code === CHAR_FORWARD_SLASH;
}
function isPathSeparator(code) {
  return code === CHAR_FORWARD_SLASH || code === CHAR_BACKWARD_SLASH;
}
function isWindowsDeviceRoot(code) {
  return code >= CHAR_LOWERCASE_A && code <= CHAR_LOWERCASE_Z || code >= CHAR_UPPERCASE_A && code <= CHAR_UPPERCASE_Z;
}

// deno:https://jsr.io/@std/path/1.0.8/_common/dirname.ts
function assertArg(path) {
  assertPath(path);
  if (path.length === 0) return ".";
}

// deno:https://jsr.io/@std/path/1.0.8/posix/dirname.ts
function dirname(path) {
  assertArg(path);
  let end = -1;
  let matchedNonSeparator = false;
  for (let i = path.length - 1; i >= 1; --i) {
    if (isPosixPathSeparator(path.charCodeAt(i))) {
      if (matchedNonSeparator) {
        end = i;
        break;
      }
    } else {
      matchedNonSeparator = true;
    }
  }
  if (end === -1) {
    return isPosixPathSeparator(path.charCodeAt(0)) ? "/" : ".";
  }
  return stripTrailingSeparators(path.slice(0, end), isPosixPathSeparator);
}

// deno:https://jsr.io/@std/path/1.0.8/windows/dirname.ts
function dirname2(path) {
  assertArg(path);
  const len = path.length;
  let rootEnd = -1;
  let end = -1;
  let matchedSlash = true;
  let offset = 0;
  const code = path.charCodeAt(0);
  if (len > 1) {
    if (isPathSeparator(code)) {
      rootEnd = offset = 1;
      if (isPathSeparator(path.charCodeAt(1))) {
        let j = 2;
        let last = j;
        for (; j < len; ++j) {
          if (isPathSeparator(path.charCodeAt(j))) break;
        }
        if (j < len && j !== last) {
          last = j;
          for (; j < len; ++j) {
            if (!isPathSeparator(path.charCodeAt(j))) break;
          }
          if (j < len && j !== last) {
            last = j;
            for (; j < len; ++j) {
              if (isPathSeparator(path.charCodeAt(j))) break;
            }
            if (j === len) {
              return path;
            }
            if (j !== last) {
              rootEnd = offset = j + 1;
            }
          }
        }
      }
    } else if (isWindowsDeviceRoot(code)) {
      if (path.charCodeAt(1) === CHAR_COLON) {
        rootEnd = offset = 2;
        if (len > 2) {
          if (isPathSeparator(path.charCodeAt(2))) rootEnd = offset = 3;
        }
      }
    }
  } else if (isPathSeparator(code)) {
    return path;
  }
  for (let i = len - 1; i >= offset; --i) {
    if (isPathSeparator(path.charCodeAt(i))) {
      if (!matchedSlash) {
        end = i;
        break;
      }
    } else {
      matchedSlash = false;
    }
  }
  if (end === -1) {
    if (rootEnd === -1) return ".";
    else end = rootEnd;
  }
  return stripTrailingSeparators(path.slice(0, end), isPosixPathSeparator2);
}

// deno:https://jsr.io/@std/path/1.0.8/dirname.ts
function dirname3(path) {
  return isWindows ? dirname2(path) : dirname(path);
}

// deno:https://jsr.io/@std/path/1.0.8/_common/from_file_url.ts
function assertArg3(url) {
  url = url instanceof URL ? url : new URL(url);
  if (url.protocol !== "file:") {
    throw new TypeError(`URL must be a file URL: received "${url.protocol}"`);
  }
  return url;
}

// deno:https://jsr.io/@std/path/1.0.8/posix/from_file_url.ts
function fromFileUrl(url) {
  url = assertArg3(url);
  return decodeURIComponent(url.pathname.replace(/%(?![0-9A-Fa-f]{2})/g, "%25"));
}

// deno:https://jsr.io/@std/path/1.0.8/windows/from_file_url.ts
function fromFileUrl2(url) {
  url = assertArg3(url);
  let path = decodeURIComponent(url.pathname.replace(/\//g, "\\").replace(/%(?![0-9A-Fa-f]{2})/g, "%25")).replace(/^\\*([A-Za-z]:)(\\|$)/, "$1\\");
  if (url.hostname !== "") {
    path = `\\\\${url.hostname}${path}`;
  }
  return path;
}

// deno:https://jsr.io/@std/path/1.0.8/from_file_url.ts
function fromFileUrl3(url) {
  return isWindows ? fromFileUrl2(url) : fromFileUrl(url);
}

// deno:https://jsr.io/@std/path/1.0.8/_common/normalize.ts
function assertArg4(path) {
  assertPath(path);
  if (path.length === 0) return ".";
}

// deno:https://jsr.io/@std/path/1.0.8/_common/normalize_string.ts
function normalizeString(path, allowAboveRoot, separator, isPathSeparator2) {
  let res = "";
  let lastSegmentLength = 0;
  let lastSlash = -1;
  let dots = 0;
  let code;
  for (let i = 0; i <= path.length; ++i) {
    if (i < path.length) code = path.charCodeAt(i);
    else if (isPathSeparator2(code)) break;
    else code = CHAR_FORWARD_SLASH;
    if (isPathSeparator2(code)) {
      if (lastSlash === i - 1 || dots === 1) {
      } else if (lastSlash !== i - 1 && dots === 2) {
        if (res.length < 2 || lastSegmentLength !== 2 || res.charCodeAt(res.length - 1) !== CHAR_DOT || res.charCodeAt(res.length - 2) !== CHAR_DOT) {
          if (res.length > 2) {
            const lastSlashIndex = res.lastIndexOf(separator);
            if (lastSlashIndex === -1) {
              res = "";
              lastSegmentLength = 0;
            } else {
              res = res.slice(0, lastSlashIndex);
              lastSegmentLength = res.length - 1 - res.lastIndexOf(separator);
            }
            lastSlash = i;
            dots = 0;
            continue;
          } else if (res.length === 2 || res.length === 1) {
            res = "";
            lastSegmentLength = 0;
            lastSlash = i;
            dots = 0;
            continue;
          }
        }
        if (allowAboveRoot) {
          if (res.length > 0) res += `${separator}..`;
          else res = "..";
          lastSegmentLength = 2;
        }
      } else {
        if (res.length > 0) res += separator + path.slice(lastSlash + 1, i);
        else res = path.slice(lastSlash + 1, i);
        lastSegmentLength = i - lastSlash - 1;
      }
      lastSlash = i;
      dots = 0;
    } else if (code === CHAR_DOT && dots !== -1) {
      ++dots;
    } else {
      dots = -1;
    }
  }
  return res;
}

// deno:https://jsr.io/@std/path/1.0.8/posix/normalize.ts
function normalize(path) {
  assertArg4(path);
  const isAbsolute3 = isPosixPathSeparator(path.charCodeAt(0));
  const trailingSeparator = isPosixPathSeparator(path.charCodeAt(path.length - 1));
  path = normalizeString(path, !isAbsolute3, "/", isPosixPathSeparator);
  if (path.length === 0 && !isAbsolute3) path = ".";
  if (path.length > 0 && trailingSeparator) path += "/";
  if (isAbsolute3) return `/${path}`;
  return path;
}

// deno:https://jsr.io/@std/path/1.0.8/posix/join.ts
function join(...paths) {
  if (paths.length === 0) return ".";
  paths.forEach((path) => assertPath(path));
  const joined = paths.filter((path) => path.length > 0).join("/");
  return joined === "" ? "." : normalize(joined);
}

// deno:https://jsr.io/@std/path/1.0.8/windows/normalize.ts
function normalize2(path) {
  assertArg4(path);
  const len = path.length;
  let rootEnd = 0;
  let device;
  let isAbsolute3 = false;
  const code = path.charCodeAt(0);
  if (len > 1) {
    if (isPathSeparator(code)) {
      isAbsolute3 = true;
      if (isPathSeparator(path.charCodeAt(1))) {
        let j = 2;
        let last = j;
        for (; j < len; ++j) {
          if (isPathSeparator(path.charCodeAt(j))) break;
        }
        if (j < len && j !== last) {
          const firstPart = path.slice(last, j);
          last = j;
          for (; j < len; ++j) {
            if (!isPathSeparator(path.charCodeAt(j))) break;
          }
          if (j < len && j !== last) {
            last = j;
            for (; j < len; ++j) {
              if (isPathSeparator(path.charCodeAt(j))) break;
            }
            if (j === len) {
              return `\\\\${firstPart}\\${path.slice(last)}\\`;
            } else if (j !== last) {
              device = `\\\\${firstPart}\\${path.slice(last, j)}`;
              rootEnd = j;
            }
          }
        }
      } else {
        rootEnd = 1;
      }
    } else if (isWindowsDeviceRoot(code)) {
      if (path.charCodeAt(1) === CHAR_COLON) {
        device = path.slice(0, 2);
        rootEnd = 2;
        if (len > 2) {
          if (isPathSeparator(path.charCodeAt(2))) {
            isAbsolute3 = true;
            rootEnd = 3;
          }
        }
      }
    }
  } else if (isPathSeparator(code)) {
    return "\\";
  }
  let tail;
  if (rootEnd < len) {
    tail = normalizeString(path.slice(rootEnd), !isAbsolute3, "\\", isPathSeparator);
  } else {
    tail = "";
  }
  if (tail.length === 0 && !isAbsolute3) tail = ".";
  if (tail.length > 0 && isPathSeparator(path.charCodeAt(len - 1))) {
    tail += "\\";
  }
  if (device === void 0) {
    if (isAbsolute3) {
      if (tail.length > 0) return `\\${tail}`;
      else return "\\";
    }
    return tail;
  } else if (isAbsolute3) {
    if (tail.length > 0) return `${device}\\${tail}`;
    else return `${device}\\`;
  }
  return device + tail;
}

// deno:https://jsr.io/@std/path/1.0.8/windows/join.ts
function join2(...paths) {
  paths.forEach((path) => assertPath(path));
  paths = paths.filter((path) => path.length > 0);
  if (paths.length === 0) return ".";
  let needsReplace = true;
  let slashCount = 0;
  const firstPart = paths[0];
  if (isPathSeparator(firstPart.charCodeAt(0))) {
    ++slashCount;
    const firstLen = firstPart.length;
    if (firstLen > 1) {
      if (isPathSeparator(firstPart.charCodeAt(1))) {
        ++slashCount;
        if (firstLen > 2) {
          if (isPathSeparator(firstPart.charCodeAt(2))) ++slashCount;
          else {
            needsReplace = false;
          }
        }
      }
    }
  }
  let joined = paths.join("\\");
  if (needsReplace) {
    for (; slashCount < joined.length; ++slashCount) {
      if (!isPathSeparator(joined.charCodeAt(slashCount))) break;
    }
    if (slashCount >= 2) joined = `\\${joined.slice(slashCount)}`;
  }
  return normalize2(joined);
}

// deno:https://jsr.io/@std/path/1.0.8/join.ts
function join3(...paths) {
  return isWindows ? join2(...paths) : join(...paths);
}

// deno:https://deno.land/std@0.224.0/async/delay.ts
function delay(ms, options = {}) {
  const { signal, persistent = true } = options;
  if (signal?.aborted) return Promise.reject(signal.reason);
  return new Promise((resolve3, reject) => {
    const abort = () => {
      clearTimeout(i);
      reject(signal?.reason);
    };
    const done = () => {
      signal?.removeEventListener("abort", abort);
      resolve3();
    };
    const i = setTimeout(done, ms);
    signal?.addEventListener("abort", abort, {
      once: true
    });
    if (persistent === false) {
      try {
        Deno.unrefTimer(i);
      } catch (error) {
        if (!(error instanceof ReferenceError)) {
          throw error;
        }
        console.error("`persistent` option is only available in Deno");
      }
    }
  });
}

// crates/quarto-core/tests/fixtures/extensions/marimo/lib/cell-execution-regex.ts
var MARIMO_CELL_REGEX = /^\s*(```+)\s*(?=.*\.marimo)\{?((?:python|sql)(?:\.marimo)?)[^}]*\}\s*$/;
var MARIMO_CELL_REGEX_WITH_BARE_SQL = /^\s*(```+)\s*\{?((?:python|sql)(?:\.marimo)?)[^}]*\}\s*$/;

// crates/quarto-core/tests/fixtures/extensions/marimo/lib/is-marimo-cell.ts
function isMarimoCell(cell) {
  if (typeof cell.cell_type !== "object" || !("language" in cell.cell_type)) {
    return false;
  }
  const lang = cell.cell_type.language;
  if (lang === "python.marimo" || lang === "sql.marimo") {
    return true;
  }
  if (lang === "python" || lang === "sql") {
    const firstLine = cell.sourceVerbatim.value.split("\n")[0] || "";
    return /\.marimo/.test(firstLine);
  }
  return false;
}
function cellOwnedByMarimo(cell, handledLanguages) {
  if (isMarimoCell(cell)) {
    return true;
  }
  if (typeof cell.cell_type !== "object" || !("language" in cell.cell_type)) {
    return false;
  }
  return cell.cell_type.language === "sql" && !handledLanguages.includes("sql");
}

// crates/quarto-core/tests/fixtures/extensions/marimo/lib/render-output.ts
async function renderOutput(output, mimeSensitive, htmlToMarkdown2 = async (h) => h) {
  let result = "";
  if (output.display_code && output.code) {
    result += "```python\n" + output.code + "\n```\n\n";
  }
  if (!mimeSensitive) {
    if (output.value) {
      result += "```{=html}\n" + output.value + "\n```\n\n";
    }
  } else {
    switch (output.type) {
      case "figure":
        if (output.value) {
          result += `![Generated Figure](${output.value})

`;
        }
        break;
      case "para":
        if (output.value) {
          result += output.value + "\n\n";
        }
        break;
      case "blockquote":
        if (output.value) {
          result += "> " + output.value + "\n\n";
        }
        break;
      case "html":
      default:
        if (output.value) {
          if (/<table[\s>]/i.test(output.value)) {
            result += "```{=html}\n" + output.value + "\n```\n\n";
          } else {
            const markdown = await htmlToMarkdown2(output.value);
            result += markdown + "\n\n";
          }
        }
        break;
    }
  }
  return result;
}

// crates/quarto-core/tests/fixtures/extensions/marimo/src/marimo-engine.ts
var quarto;
function containsMarimoFence(markdown) {
  return markdown.split(/\r?\n/).some((line) => MARIMO_CELL_REGEX.test(line));
}
async function executePython(command, args = [], stdin = "") {
  const cmd = new Deno.Command(command, {
    args,
    stdin: "piped",
    stdout: "piped",
    stderr: "piped"
  });
  const process = cmd.spawn();
  if (stdin) {
    const writer = process.stdin.getWriter();
    const encoder = new TextEncoder();
    await writer.write(encoder.encode(stdin));
    await writer.close();
  } else {
    process.stdin.close();
  }
  const output = await process.output();
  const decoder = new TextDecoder();
  const stderr = decoder.decode(output.stderr);
  if (stderr) {
    quarto.console.info(`Subprocess stderr: ${stderr}`);
  }
  if (!output.success) {
    throw new Error(`Process execution failed: ${stderr}`);
  }
  return decoder.decode(output.stdout);
}
async function constructUvCommand(header) {
  const currentDir = dirname3(fromFileUrl3(import.meta.url));
  const scriptPath = join3(currentDir, "command.py");
  const result = await executePython("uv", [
    "run",
    "--with",
    "marimo",
    scriptPath
  ], header);
  return JSON.parse(result);
}
async function buildCommand(metadata, extractPath, getUvFlags = constructUvCommand) {
  const useExternalEnv = metadata["external-env"] === true;
  if (useExternalEnv) {
    return [
      "python",
      extractPath
    ];
  }
  const pyprojectConfig = metadata["pyproject"];
  const header = pyprojectConfig ? String(pyprojectConfig) : "";
  const uvFlags = await getUvFlags(header);
  return [
    "uv",
    ...uvFlags,
    extractPath
  ];
}
async function htmlToMarkdown(html) {
  const result = await quarto.system.pandoc([
    "-f",
    "html",
    "-t",
    "markdown"
  ], html);
  if (!result.success) {
    quarto.console.warning(`Pandoc conversion failed: ${result.stderr}`);
    return html;
  }
  return result.stdout || "";
}
var marimoEngineDiscovery = {
  init: (quartoAPI) => {
    quarto = quartoAPI;
  },
  name: "marimo",
  defaultExt: ".qmd",
  defaultYaml: () => [
    "format: html",
    "engine: marimo"
  ],
  defaultContent: () => [
    "```{python .marimo}",
    "import marimo as mo",
    "slider = mo.ui.slider(1, 10, 1)",
    "slider",
    "```"
  ],
  validExtensions: () => [
    ".qmd",
    ".md"
  ],
  // When Quarto's language discovery sees `python {...}`
  // then it will route to Python engine without scanning block contents.
  // We want `python {.marimo}` blocks route to our engine, so we override the default claim logic.
  claimsFile: (file, ext) => {
    if (![
      ".qmd",
      ".md"
    ].includes(ext.toLowerCase())) {
      return false;
    }
    try {
      return containsMarimoFence(Deno.readTextFileSync(file));
    } catch {
      return false;
    }
  },
  claimsLanguage: (language, firstClass) => {
    if ((language === "python" || language === "sql") && firstClass === "marimo") {
      return 2;
    }
    if (language === "python.marimo" || language === "sql.marimo") {
      return 1;
    }
    if (language === "sql") {
      return {
        kind: "interop"
      };
    }
    return false;
  },
  canFreeze: false,
  generatesFigures: true,
  checkInstallation: async () => {
    await quarto.console.withSpinner({
      message: "Checking Marimo installation..."
    }, async () => {
      await delay(2e3);
    });
  },
  launch: (context) => {
    return {
      name: marimoEngineDiscovery.name,
      canFreeze: marimoEngineDiscovery.canFreeze,
      markdownForFile(file) {
        return Promise.resolve(quarto.mappedString.fromFile(file));
      },
      target: (file, _quiet, markdown) => {
        const md = markdown ?? quarto.mappedString.fromFile(file);
        const metadata = quarto.markdownRegex.extractYaml(md.value);
        return Promise.resolve({
          source: file,
          input: file,
          markdown: md,
          metadata
        });
      },
      partitionedMarkdown: (file) => {
        return Promise.resolve(quarto.markdownRegex.partition(Deno.readTextFileSync(file)));
      },
      execute: async (options) => {
        const { target, format: format3 } = options;
        const markdown = target.markdown.value;
        const outputFormat = format3.pandoc.to || "html";
        const mimeSensitive = outputFormat === "pdf" || outputFormat === "latex" || outputFormat === "typst";
        const bareSqlOwned = !(options.handledLanguages ?? []).includes("sql");
        try {
          const currentDir = dirname3(fromFileUrl3(import.meta.url));
          const extractPath = join3(currentDir, "extract.py");
          const [command, ...args] = await buildCommand(target.metadata, extractPath);
          const globalEval = target.metadata["eval"] !== false;
          args.push(target.input, mimeSensitive ? "yes" : "no", globalEval ? "yes" : "no", bareSqlOwned ? "yes" : "no");
          const result = await quarto.console.withSpinner({
            message: "Executing marimo cells..."
          }, async () => {
            return await executePython(command, args, markdown);
          });
          const marimoExecution = JSON.parse(result);
          const chunks = await quarto.markdownRegex.breakQuartoMd(target.markdown, false, false, bareSqlOwned ? MARIMO_CELL_REGEX_WITH_BARE_SQL : MARIMO_CELL_REGEX);
          const processedCells = [];
          let marimoIndex = 0;
          const handledLanguages = options.handledLanguages ?? [];
          for (const cell of chunks.cells) {
            if (cellOwnedByMarimo(cell, handledLanguages)) {
              if (marimoIndex < marimoExecution.outputs.length) {
                const output = marimoExecution.outputs[marimoIndex];
                const rendered = await renderOutput(output, mimeSensitive, htmlToMarkdown);
                processedCells.push(rendered);
              } else {
                quarto.console.warning(`Marimo cell ${marimoIndex} has no corresponding output`);
                processedCells.push(cell.sourceVerbatim.value);
              }
              marimoIndex++;
            } else {
              processedCells.push(cell.sourceVerbatim.value);
            }
          }
          if (marimoIndex !== marimoExecution.count) {
            quarto.console.warning(`Expected ${marimoExecution.count} marimo cells, found ${marimoIndex}`);
          }
          const processedMarkdown = processedCells.join("");
          const includes = {};
          if (outputFormat === "html" && marimoExecution.header) {
            const tempFile = Deno.makeTempFileSync({
              dir: options.tempDir,
              prefix: "marimo-header-",
              suffix: ".html"
            });
            Deno.writeTextFileSync(tempFile, marimoExecution.header);
            includes["include-in-header"] = [
              tempFile
            ];
          }
          return {
            engine: "marimo",
            markdown: processedMarkdown,
            supporting: [],
            filters: [],
            includes: Object.keys(includes).length > 0 ? includes : void 0
          };
        } catch (error) {
          quarto.console.error(`Error executing marimo: ${error}`);
          return {
            engine: "marimo",
            markdown: `\`\`\`
Error executing marimo: ${error.message}
\`\`\`

${markdown}`,
            supporting: [],
            filters: []
          };
        }
      },
      dependencies: (_options) => {
        return Promise.resolve({
          includes: {}
        });
      },
      postprocess: (_options) => Promise.resolve()
    };
  }
};
var marimo_engine_default = marimoEngineDiscovery;
export {
  buildCommand,
  containsMarimoFence,
  marimo_engine_default as default
};
