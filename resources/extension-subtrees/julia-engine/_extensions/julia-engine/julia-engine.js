// src/julia-engine.ts
import {
  existsSync,
  readFileSync,
  rmSync,
  writeFileSync
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

// src/worker-close.ts
function isWorkerBusyError(e) {
  return e instanceof Error && /worker is busy/i.test(e.message);
}
async function preRunClose(writeCommand, file) {
  const isopen = await writeCommand({ type: "isopen", content: { file } });
  if (isopen) {
    try {
      await writeCommand({ type: "close", content: { file } });
    } catch (e) {
      if (isWorkerBusyError(e)) {
        await writeCommand({ type: "forceclose", content: { file } });
      } else {
        throw e;
      }
    }
  }
}
async function postRunClose(writeCommand, file, warn) {
  try {
    await writeCommand({ type: "close", content: { file } });
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    warn(
      `Julia worker close after a successful run failed; returning results anyway.
${message}`
    );
  }
}
async function errorRunClose(writeCommand, file, warn) {
  try {
    try {
      await writeCommand({ type: "close", content: { file } });
    } catch (e) {
      if (isWorkerBusyError(e)) {
        await writeCommand({ type: "forceclose", content: { file } });
      } else {
        throw e;
      }
    }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    warn(
      `Julia worker close after a failed run also failed; the worker may be leaked.
${message}`
    );
  }
}

// src/constants.ts
var kJuliaEngine = "julia";
var kExecuteDaemon = "daemon";
var kExecuteDaemonRestart = "daemon-restart";
var kExecuteDebug = "debug";
var kFigDpi = "fig-dpi";
var kFigFormat = "fig-format";
var kFigPos = "fig-pos";
var kIpynbProduceSourceNotebook = "produce-source-notebook";
var kKeepHidden = "keep-hidden";
var kKeepIpynb = "keep-ipynb";

// src/transport.ts
import { createHmac } from "node:crypto";
import { connect } from "node:net";
function signPayload(payload, secret) {
  return createHmac("sha256", secret).update(payload).digest("base64");
}
function buildRequestLine(payload, secret) {
  return JSON.stringify({ hmac: signPayload(payload, secret), payload }) + "\n";
}
var NEWLINE = 10;
var LineConnection = class _LineConnection {
  parts = [];
  lines = [];
  waiter = null;
  failure = null;
  socket;
  constructor(socket) {
    this.socket = socket;
    socket.on("data", (chunk) => this.onData(chunk));
    socket.on("error", (e) => this.fail(e));
    socket.on("close", () => {
      this.fail(
        new Error(
          this.parts.length > 0 ? "Connection to the julia server closed in the middle of a message." : "Connection to the julia server closed before a reply was received."
        )
      );
    });
  }
  static connect(port, host = "127.0.0.1") {
    return new Promise((resolve2, reject) => {
      const socket = connect({ host, port });
      const onError = (e) => reject(e);
      socket.once("error", onError);
      socket.once("connect", () => {
        socket.off("error", onError);
        resolve2(new _LineConnection(socket));
      });
    });
  }
  onData(chunk) {
    let start = 0;
    while (true) {
      const nl = chunk.indexOf(NEWLINE, start);
      if (nl === -1) {
        if (start < chunk.length) this.parts.push(chunk.subarray(start));
        return;
      }
      this.parts.push(chunk.subarray(start, nl));
      this.lines.push(Buffer.concat(this.parts).toString("utf8"));
      this.parts = [];
      start = nl + 1;
      this.flush();
    }
  }
  flush() {
    if (this.waiter && this.lines.length > 0) {
      const { resolve: resolve2 } = this.waiter;
      this.waiter = null;
      resolve2(this.lines.shift());
    } else if (this.waiter && this.failure && this.lines.length === 0) {
      const { reject } = this.waiter;
      this.waiter = null;
      reject(this.failure);
    }
  }
  fail(e) {
    this.failure ??= e;
    this.flush();
  }
  /** Write one message (the caller supplies the trailing newline). */
  writeLine(line) {
    return new Promise((resolve2, reject) => {
      if (this.failure) return reject(this.failure);
      this.socket.write(line, "utf8", (e) => e ? reject(e) : resolve2());
    });
  }
  /** The next complete line, without its newline. */
  readLine() {
    if (this.lines.length > 0) return Promise.resolve(this.lines.shift());
    if (this.failure) return Promise.reject(this.failure);
    return new Promise((resolve2, reject) => {
      this.waiter = { resolve: resolve2, reject };
    });
  }
  close() {
    this.socket.destroy();
  }
};
function isProgressUpdate(data) {
  return data && data.type === "progress_update";
}
function isServerCommandError(data) {
  return data && typeof data.error === "string";
}
async function sendCommand(conn, command, secret, trace2 = () => {
}, onProgressUpdate) {
  trace2(`write command "${command.type}" to socket server`);
  await conn.writeLine(buildRequestLine(JSON.stringify(command), secret));
  while (true) {
    const line = await conn.readLine();
    trace2("received server response");
    let responseData;
    try {
      responseData = JSON.parse(line);
    } catch (e) {
      throw new Error(
        `Julia server sent an unparseable response to the "${command.type}" command: ${e}`
      );
    }
    if (isServerCommandError(responseData)) {
      let errorMessage = `Julia server returned error after receiving "${command.type}" command:

${responseData.error}`;
      if (responseData.juliaError) {
        errorMessage += `

The underlying Julia error was:

${responseData.juliaError}`;
      }
      throw new Error(errorMessage);
    }
    if (command.type === "run" && isProgressUpdate(responseData)) {
      trace2("received progress update response, listening for further responses");
      onProgressUpdate?.(responseData);
      continue;
    }
    return responseData;
  }
}
async function connectReady(port, secret, timeoutMs = 1e4, trace2 = () => {
}) {
  const conn = await LineConnection.connect(port);
  const isready = sendCommand(
    conn,
    { type: "isready", content: {} },
    secret,
    trace2
  );
  let timer;
  const timeout = new Promise((accept) => {
    timer = setTimeout(() => {
      accept(
        `Timed out after getting no response for ${timeoutMs} milliseconds.`
      );
    }, timeoutMs);
  });
  let result;
  try {
    result = await Promise.race([isready, timeout]);
  } catch (e) {
    conn.close();
    throw e;
  } finally {
    clearTimeout(timer);
  }
  if (typeof result === "string") {
    conn.close();
    isready.catch(() => {
    });
    return result;
  } else if (result !== true) {
    conn.close();
    return `Expected isready command to return true, returned ${result} instead. Closing connection.`;
  } else {
    return conn;
  }
}

// src/julia-engine.ts
var isWindows = process.platform === "win32";
var extensionDir = dirname(fileURLToPath(import.meta.url));
var quarto;
function safeRemoveSync(file, options = {}) {
  try {
    rmSync(file, options);
  } catch (e) {
    if (existsSync(file)) {
      throw e;
    }
  }
}
var delay = (ms) => new Promise((resolve2) => setTimeout(resolve2, ms));
var juliaEngineDiscovery = {
  init: (quartoAPI) => {
    quarto = quartoAPI;
  },
  name: kJuliaEngine,
  defaultExt: ".qmd",
  defaultYaml: () => [],
  defaultContent: () => [
    "```{julia}",
    "1 + 1",
    "```"
  ],
  validExtensions: () => [],
  claimsFile: (file, _ext) => {
    return quarto.jupyter.isPercentScript(file, [".jl"]);
  },
  claimsLanguage: (language) => {
    return language.toLowerCase() === "julia";
  },
  canFreeze: true,
  generatesFigures: true,
  ignoreDirs: () => {
    return [];
  },
  /**
   * Populate engine-specific CLI commands
   */
  populateCommand: (command) => populateJuliaEngineCommand(command),
  /**
   * Check Julia installation
   */
  checkInstallation: async () => {
    await quarto.console.withSpinner(
      { message: "Checking Julia installation..." },
      async () => {
        await delay(3e3);
      }
    );
  },
  /**
   * Launch a dynamic execution engine with project context
   */
  launch: (context) => {
    return {
      name: juliaEngineDiscovery.name,
      canFreeze: juliaEngineDiscovery.canFreeze,
      partitionedMarkdown: (file) => {
        return Promise.resolve(
          quarto.markdownRegex.partition(readFileSync(file, "utf8"))
        );
      },
      // TODO: ask dragonstyle what to do here
      executeTargetSkipped: () => false,
      // TODO: just return dependencies from execute and this can do nothing
      dependencies: (_options) => {
        const includes = {};
        return Promise.resolve({
          includes
        });
      },
      // TODO: this can also probably do nothing
      postprocess: (_options) => {
        return Promise.resolve();
      },
      canKeepSource: (_target) => {
        return true;
      },
      markdownForFile(file) {
        if (quarto.jupyter.isPercentScript(file, [".jl"])) {
          return Promise.resolve(
            quarto.mappedString.fromString(
              quarto.jupyter.percentScriptToMarkdown(file)
            )
          );
        } else {
          return Promise.resolve(quarto.mappedString.fromFile(file));
        }
      },
      execute: async (options) => {
        options.target.source;
        let executeDaemon = options.format.execute[kExecuteDaemon];
        if (executeDaemon === null || executeDaemon === void 0) {
          executeDaemon = quarto.system.isInteractiveSession() && !quarto.system.runningInCI();
        }
        const execOptions = {
          ...options,
          target: {
            ...options.target,
            input: quarto.path.absolute(options.target.input)
          }
        };
        const juliaExecOptions = {
          oneShot: !executeDaemon,
          ...execOptions
        };
        const nb = await executeJulia(juliaExecOptions);
        if (!nb) {
          quarto.console.error("Execution of notebook returned undefined");
          return Promise.reject();
        }
        nb.metadata.kernelspec = {
          display_name: "Julia",
          name: "julia",
          language: "julia"
        };
        const assets = quarto.jupyter.assets(
          options.target.input,
          options.format.pandoc.to
        );
        if (options.format.execute[kKeepIpynb]) {
          const stem = options.target.source.replace(/\.[^.]+$/, "");
          const ipynbPath = stem + ".ipynb";
          writeFileSync(ipynbPath, JSON.stringify(nb, null, 2));
        }
        const result = await quarto.jupyter.toMarkdown(
          nb,
          {
            executeOptions: options,
            language: nb.metadata.kernelspec.language.toLowerCase(),
            assets,
            execute: options.format.execute,
            keepHidden: options.format.render[kKeepHidden],
            toHtml: quarto.format.isHtmlCompatible(options.format),
            toLatex: quarto.format.isLatexOutput(options.format.pandoc),
            toMarkdown: quarto.format.isMarkdownOutput(options.format),
            toIpynb: quarto.format.isIpynbOutput(options.format.pandoc),
            toPresentation: quarto.format.isPresentationOutput(
              options.format.pandoc
            ),
            figFormat: options.format.execute[kFigFormat],
            figDpi: options.format.execute[kFigDpi],
            figPos: options.format.render[kFigPos],
            // preserveCellMetadata,
            preserveCodeCellYaml: options.format.render[kIpynbProduceSourceNotebook] === true
          }
        );
        let includes;
        let engineDependencies;
        if (options.dependencies) {
          includes = quarto.jupyter.resultIncludes(
            options.tempDir,
            result.dependencies
          );
        } else {
          const dependencies = quarto.jupyter.resultEngineDependencies(
            result.dependencies
          );
          if (dependencies) {
            engineDependencies = {
              [kJuliaEngine]: dependencies
            };
          }
        }
        const outputs = result.cellOutputs.map((output) => output.markdown);
        if (result.notebookOutputs) {
          if (result.notebookOutputs.prefix) {
            outputs.unshift(result.notebookOutputs.prefix);
          }
          if (result.notebookOutputs.suffix) {
            outputs.push(result.notebookOutputs.suffix);
          }
        }
        const markdown = outputs.join("");
        return {
          engine: kJuliaEngine,
          markdown,
          supporting: [join(assets.base_dir, assets.supporting_dir)],
          filters: [],
          pandoc: result.pandoc,
          includes,
          engineDependencies,
          preserve: result.htmlPreserve,
          postProcess: result.htmlPreserve && Object.keys(result.htmlPreserve).length > 0
        };
      },
      target: (file, _quiet, markdown) => {
        if (markdown === void 0) {
          markdown = quarto.mappedString.fromFile(file);
        }
        const target = {
          source: file,
          input: file,
          markdown,
          metadata: quarto.markdownRegex.extractYaml(markdown.value)
        };
        return Promise.resolve(target);
      }
    };
  }
};
var julia_engine_default = juliaEngineDiscovery;
function juliaCmd() {
  return process.env.QUARTO_JULIA ?? "julia";
}
function withEnv(overrides) {
  return { ...process.env, ...overrides };
}
function runInherited(cmd, args, env) {
  return new Promise((resolve2, reject) => {
    const child = spawn(cmd, args, {
      env,
      stdio: ["ignore", "inherit", "inherit"]
    });
    child.once("error", (e) => reject(
      e.code === "ENOENT" ? new Error(`Could not run "${cmd}": command not found.`) : e
    ));
    child.once("close", (code) => resolve2(code === 0));
  });
}
function runCaptured(cmd, args, env) {
  const result = spawnSync(cmd, args, { encoding: "utf8", env });
  if (result.error) {
    const code = result.error.code;
    throw code === "ENOENT" ? new Error(`Could not run "${cmd}": command not found.`) : result.error;
  }
  if (result.status !== 0) {
    throw new Error(result.stderr);
  }
}
function powershell_argument_list_to_string(...args) {
  const inner = args.map((arg) => `"${arg}"`).join(" ");
  return `'${inner}'`;
}
async function startOrReuseJuliaServer(options) {
  const transportFile = juliaTransportFile();
  if (!existsSync(transportFile)) {
    trace(
      options,
      `Transport file ${transportFile} doesn't exist`
    );
    quarto.console.info("Starting julia control server process. This might take a while...");
    let juliaProject = process.env.QUARTO_JULIA_PROJECT;
    if (juliaProject === void 0) {
      await ensureQuartoNotebookRunnerEnvironment(options);
      juliaProject = quarto.path.runtime("julia");
    } else {
      juliaProject = quarto.path.toForwardSlashes(juliaProject);
      trace(
        options,
        `Custom julia project set via QUARTO_JULIA_PROJECT="${juliaProject}". Checking if QuartoNotebookRunner can be loaded.`
      );
      const qnrLoadable = await runInherited(
        juliaCmd(),
        [
          "--startup-file=no",
          `--project=${juliaProject}`,
          "-e",
          "using QuartoNotebookRunner"
        ],
        // ignore the main env
        withEnv({ JULIA_LOAD_PATH: isWindows ? "@;@stdlib" : "@:@stdlib" })
      );
      if (!qnrLoadable) {
        throw Error(
          `Executing \`using QuartoNotebookRunner\` failed with QUARTO_JULIA_PROJECT="${juliaProject}". Ensure that this project exists, has QuartoNotebookRunner installed and is instantiated correctly.`
        );
      }
      trace(
        options,
        `QuartoNotebookRunner could be loaded successfully.`
      );
    }
    if (isWindows) {
      trace(
        options,
        "Starting detached julia server through powershell, once transport file exists, server should be running."
      );
      runCaptured(
        "PowerShell",
        [
          "-Command",
          "Start-Process",
          juliaCmd(),
          "-ArgumentList",
          powershell_argument_list_to_string(
            "--startup-file=no",
            `--project=${juliaProject}`,
            join(extensionDir, "quartonotebookrunner.jl"),
            transportFile,
            juliaServerLogFile()
          ),
          "-WindowStyle",
          "Hidden"
        ],
        withEnv({ JULIA_LOAD_PATH: "@;@stdlib" })
        // ignore the main env
      );
    } else {
      trace(
        options,
        "Starting detached julia server through julia, once transport file exists, server should be running."
      );
      runCaptured(
        juliaCmd(),
        [
          "--startup-file=no",
          join(extensionDir, "start_quartonotebookrunner_detached.jl"),
          juliaCmd(),
          juliaProject,
          join(extensionDir, "quartonotebookrunner.jl"),
          transportFile,
          juliaServerLogFile()
        ],
        withEnv({ JULIA_LOAD_PATH: "@:@stdlib" })
        // ignore the main env
      );
    }
  } else {
    trace(
      options,
      `Transport file ${transportFile} exists, reusing server.`
    );
    return { reused: true };
  }
  return { reused: false };
}
async function ensureQuartoNotebookRunnerEnvironment(options) {
  const runtimeDir = quarto.path.runtime("julia");
  const projectTomlTemplate = join(extensionDir, "Project.toml");
  const projectToml = join(runtimeDir, "Project.toml");
  writeFileSync(projectToml, readFileSync(projectTomlTemplate));
  const success = await runInherited(juliaCmd(), [
    "--startup-file=no",
    `--project=${runtimeDir}`,
    join(extensionDir, "ensure_environment.jl")
  ]);
  if (!success) {
    throw new Error("Ensuring an updated julia server environment failed");
  }
  return Promise.resolve();
}
async function pollTransportFile(options) {
  const transportFile = juliaTransportFile();
  for (let i = 0; i < 15; i++) {
    if (existsSync(transportFile)) {
      const transportOptions = await readTransportFile(transportFile);
      trace(options, "Transport file read successfully.");
      return transportOptions;
    }
    trace(options, "Transport file did not exist, yet.");
    await delay(i * 100);
  }
  return Promise.reject();
}
async function readTransportFile(transportFile) {
  let content = readFileSync(transportFile, "utf8");
  let i = 0;
  while (i < 20 && !content.endsWith("\n")) {
    await delay(100);
    content = readFileSync(transportFile, "utf8");
    i += 1;
  }
  if (!content.endsWith("\n")) {
    throw "Read invalid transport file that did not end with a newline";
  }
  return JSON.parse(content);
}
async function getReadyServerConnection(transportOptions, executeOptions) {
  return await connectReady(
    transportOptions.port,
    transportOptions.key,
    1e4,
    (msg) => trace(executeOptions, msg)
  );
}
async function getJuliaServerConnection(options) {
  const { reused } = await startOrReuseJuliaServer(options);
  let transportOptions;
  try {
    transportOptions = await pollTransportFile(options);
  } catch (err) {
    if (!reused) {
      quarto.console.info(
        "No transport file was found after the timeout. This is the log from the server process:"
      );
      quarto.console.info("#### BEGIN LOG ####");
      printJuliaServerLog();
      quarto.console.info("#### END LOG ####");
    }
    throw err;
  }
  if (!reused) {
    quarto.console.info("Julia server process started.");
  }
  trace(
    options,
    `Connecting to server at port ${transportOptions.port}, pid ${transportOptions.pid}`
  );
  try {
    const conn = await getReadyServerConnection(transportOptions, options);
    if (typeof conn === "string") {
      throw new Error(conn);
    } else {
      return conn;
    }
  } catch (e) {
    if (reused) {
      trace(
        options,
        "Connecting to server failed, a transport file was reused so it might be stale. Delete transport file and retry."
      );
      safeRemoveSync(juliaTransportFile());
      return await getJuliaServerConnection(options);
    } else {
      quarto.console.error(
        "Connecting to server failed. A transport file was successfully created by the server process, so something in the server process might be broken."
      );
      throw e;
    }
  }
}
function firstSignificantLine(str, n) {
  const lines = str.split("\n");
  for (const line of lines) {
    const trimmedLine = line.trim();
    if (!trimmedLine.startsWith("#|") && trimmedLine !== "") {
      if (trimmedLine.length <= n) {
        return trimmedLine;
      } else {
        return trimmedLine.substring(0, n - 1) + "\u2026";
      }
    }
  }
  return "";
}
function getConsoleColumns() {
  try {
    return process.stdout.columns ?? null;
  } catch (_error) {
    return null;
  }
}
function buildSourceRanges(markdown) {
  const lines = quarto.mappedString.splitLines(markdown);
  const sourceRanges = [];
  let currentRange = null;
  lines.forEach((line, index) => {
    const mapResult = line.map(0, true);
    if (mapResult) {
      const { originalString } = mapResult;
      const lineCol = quarto.mappedString.indexToLineCol(
        originalString,
        mapResult.index
      );
      const fileName = originalString.fileName ? resolve(originalString.fileName) : void 0;
      const sourceLineNum = lineCol.line;
      if (currentRange && currentRange.file === fileName && fileName !== void 0 && currentRange.sourceLines && currentRange.sourceLines[1] === sourceLineNum) {
        currentRange.lines[1] = index + 1;
        currentRange.sourceLines[1] = sourceLineNum + 1;
      } else {
        if (currentRange) {
          sourceRanges.push(currentRange);
        }
        currentRange = {
          lines: [index + 1, index + 1]
          // +1 because lines are 1-indexed
        };
        if (fileName !== void 0) {
          currentRange.file = fileName;
          currentRange.sourceLines = [sourceLineNum + 1, sourceLineNum + 1];
        }
      }
    } else {
      if (currentRange) {
        sourceRanges.push(currentRange);
        currentRange = null;
      }
    }
  });
  if (currentRange) {
    sourceRanges.push(currentRange);
  }
  return sourceRanges;
}
async function executeJulia(options) {
  const conn = await getJuliaServerConnection(options);
  const transportOptions = await pollTransportFile(options);
  const file = options.target.input;
  const writeCloseCommand = (command) => (
    // deno-lint-ignore no-explicit-any
    writeJuliaCommand(conn, command, transportOptions.key, options)
  );
  if (options.oneShot || options.format.execute[kExecuteDaemonRestart]) {
    await preRunClose(writeCloseCommand, file);
  }
  const sourceRanges = buildSourceRanges(options.target.markdown);
  let response;
  try {
    response = await writeJuliaCommand(
      conn,
      { type: "run", content: { file, options, sourceRanges } },
      transportOptions.key,
      options,
      (update) => {
        const n = update.nChunks.toString();
        const i = update.chunkIndex.toString();
        const i_padded = `${" ".repeat(n.length - i.length)}${i}`;
        const ncols = getConsoleColumns() ?? 80;
        const firstPart = `Running [${i_padded}/${n}] at line ${update.line}:  `;
        const firstPartLength = firstPart.length;
        const sigLine = firstSignificantLine(
          update.source,
          Math.max(0, ncols - firstPartLength)
        );
        quarto.console.info(`${firstPart}${sigLine}`);
      }
    );
  } catch (e) {
    if (options.oneShot) {
      await errorRunClose(
        writeCloseCommand,
        file,
        (message) => quarto.console.warning(message)
      );
    }
    throw e;
  }
  if (options.oneShot) {
    await postRunClose(
      writeCloseCommand,
      file,
      (message) => quarto.console.warning(message)
    );
  }
  return response.notebook;
}
async function writeJuliaCommand(conn, command, secret, options, onProgressUpdate) {
  return await sendCommand(
    conn,
    command,
    secret,
    (msg) => trace(options, msg),
    onProgressUpdate
  );
}
function juliaRuntimeDir() {
  try {
    return quarto.path.runtime("julia");
  } catch (e) {
    quarto.console.error("Could not create julia runtime directory.");
    quarto.console.error(
      "This is possibly a permission issue in the environment Quarto is running in."
    );
    quarto.console.error(
      "Please consult the following documentation for more information:"
    );
    quarto.console.error(
      "https://github.com/quarto-dev/quarto-cli/issues/4594#issuecomment-1619177667"
    );
    throw e;
  }
}
function juliaTransportFile() {
  return join(juliaRuntimeDir(), "julia_transport.txt");
}
function juliaServerLogFile() {
  return join(juliaRuntimeDir(), "julia_server_log.txt");
}
function trace(options, msg) {
  if (options.format?.execute[kExecuteDebug] === true) {
    quarto.console.info("- " + msg, { bold: true });
  }
}
function populateJuliaEngineCommand(command) {
  command.command("status", "Status").description(
    "Get status information on the currently running Julia server process."
  ).action(logStatus).command("kill", "Kill server").description(
    "Kill the control server if it is currently running. This will also kill all notebook worker processes."
  ).action(killJuliaServer).command("log", "Print julia server log").description(
    "Print the content of the julia server log file if it exists which can be used to diagnose problems."
  ).action(printJuliaServerLog).command(
    "close",
    "Close the worker for a given notebook. If it is currently running, it will not be interrupted."
  ).arguments("<file:string>").option(
    "-f, --force",
    "Force closing. This will terminate the worker if it is running.",
    { default: false }
  ).action(async (options, file) => {
    await closeWorker(file, options.force);
  }).command("stop", "Stop the server").description(
    "Send a message to the server that it should close all notebooks and exit. This will fail if any notebooks are not idle."
  ).action(stopServer);
  return;
}
async function logStatus() {
  const transportFile = juliaTransportFile();
  if (!existsSync(transportFile)) {
    quarto.console.info("Julia control server is not running.");
    return;
  }
  const transportOptions = await readTransportFile(transportFile);
  const conn = await getReadyServerConnection(
    transportOptions,
    {}
  );
  const successfullyConnected = typeof conn !== "string";
  if (successfullyConnected) {
    const status = await writeJuliaCommand(
      conn,
      { type: "status", content: {} },
      transportOptions.key,
      {}
    );
    process.stdout.write(status);
    conn.close();
  } else {
    quarto.console.info(`Found transport file but can't connect to control server.`);
  }
}
async function killJuliaServer() {
  const transportFile = juliaTransportFile();
  if (!existsSync(transportFile)) {
    quarto.console.info("Julia control server is not running.");
    return;
  }
  const transportOptions = await readTransportFile(transportFile);
  process.kill(transportOptions.pid, "SIGTERM");
  quarto.console.info("Sent SIGTERM to server process");
}
function printJuliaServerLog() {
  if (existsSync(juliaServerLogFile())) {
    process.stdout.write(readFileSync(juliaServerLogFile()));
  } else {
    quarto.console.info("Server log file doesn't exist");
  }
  return;
}
async function connectAndWriteJuliaCommandToRunningServer(command) {
  const transportFile = juliaTransportFile();
  if (!existsSync(transportFile)) {
    throw new Error("Julia control server is not running.");
  }
  const transportOptions = await readTransportFile(transportFile);
  const conn = await getReadyServerConnection(
    transportOptions,
    {}
  );
  const successfullyConnected = typeof conn !== "string";
  if (successfullyConnected) {
    const result = await writeJuliaCommand(
      conn,
      command,
      transportOptions.key,
      {}
    );
    conn.close();
    return result;
  } else {
    throw new Error(
      `Found transport file but can't connect to control server.`
    );
  }
}
async function closeWorker(file, force) {
  const absfile = quarto.path.absolute(file);
  await connectAndWriteJuliaCommandToRunningServer({
    type: force ? "forceclose" : "close",
    content: { file: absfile }
  });
  quarto.console.info(`Worker ${force ? "force-" : ""}closed successfully.`);
}
async function stopServer() {
  const result = await connectAndWriteJuliaCommandToRunningServer({
    type: "stop",
    content: {}
  });
  quarto.console.info(result.message);
}
export {
  julia_engine_default as default,
  juliaEngineDiscovery,
  juliaServerLogFile,
  juliaTransportFile
};
