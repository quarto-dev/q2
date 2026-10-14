/*
 * julia-engine.ts
 *
 * Quarto engine extension for Julia
 */

// Standard library imports
import {
  existsSync,
  readFileSync,
  type RmOptions,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

// Type imports from Quarto via import map
import type {
  Command,
  DependenciesOptions,
  EngineProjectContext,
  ExecuteOptions,
  ExecuteResult,
  ExecutionEngineDiscovery,
  ExecutionEngineInstance,
  ExecutionTarget,
  JupyterNotebook,
  MappedString,
  PandocIncludes,
  PostProcessOptions,
  QuartoAPI,
} from "@quarto/types";

// oneShot worker-close orchestration (busy-recovery lives here so it is
// unit-testable independent of the socket)
import {
  type CloseCommandWriter,
  errorRunClose,
  postRunClose,
  preRunClose,
} from "./worker-close.ts";

// Constants for this engine
import {
  kExecuteDaemon,
  kExecuteDaemonRestart,
  kExecuteDebug,
  kFigDpi,
  kFigFormat,
  kFigPos,
  kIpynbProduceSourceNotebook,
  kJuliaEngine,
  kKeepHidden,
  kKeepIpynb,
} from "./constants.ts";
import {
  connectReady,
  LineConnection,
  type ProgressUpdate,
  sendCommand,
} from "./transport.ts";

// Platform detection
const isWindows = process.platform === "win32";

// Extension directory (where the bundled JS and resource files live)
const extensionDir = dirname(fileURLToPath(import.meta.url));

// Module-level quarto API reference
let quarto: QuartoAPI;

// Safe file removal helper
function safeRemoveSync(file: string, options: RmOptions = {}) {
  try {
    rmSync(file, options);
  } catch (e) {
    if (existsSync(file)) {
      throw e;
    }
  }
}

// Simple async delay helper
const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export interface SourceRange {
  lines: [number, number];
  file?: string;
  sourceLines?: [number, number];
}

export interface JuliaExecuteOptions extends ExecuteOptions {
  oneShot: boolean; // if true, the file's worker process is closed before and after running
}

export const juliaEngineDiscovery: ExecutionEngineDiscovery = {
  init: (quartoAPI: QuartoAPI) => {
    quarto = quartoAPI;
  },

  name: kJuliaEngine,

  defaultExt: ".qmd",

  defaultYaml: () => [],

  defaultContent: () => [
    "```{julia}",
    "1 + 1",
    "```",
  ],

  validExtensions: () => [],

  claimsFile: (file: string, _ext: string) => {
    return quarto.jupyter.isPercentScript(file, [".jl"]);
  },

  claimsLanguage: (language: string) => {
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
  populateCommand: (command) =>
    populateJuliaEngineCommand(command),

  /**
   * Check Julia installation
   */
  checkInstallation: async () => {
    await quarto.console.withSpinner(
      { message: "Checking Julia installation..." },
      async () => {
        // Simulate checking Julia installation by waiting 3 seconds
        await delay(3000);
      },
    );
  },

  /**
   * Launch a dynamic execution engine with project context
   */
  launch: (context: EngineProjectContext): ExecutionEngineInstance => {
    return {
      name: juliaEngineDiscovery.name,
      canFreeze: juliaEngineDiscovery.canFreeze,

      partitionedMarkdown: (file: string) => {
        return Promise.resolve(
          quarto.markdownRegex.partition(readFileSync(file, "utf8")),
        );
      },

      // TODO: ask dragonstyle what to do here
      executeTargetSkipped: () => false,

      // TODO: just return dependencies from execute and this can do nothing
      dependencies: (_options: DependenciesOptions) => {
        const includes: PandocIncludes = {};
        return Promise.resolve({
          includes,
        });
      },

      // TODO: this can also probably do nothing
      postprocess: (_options: PostProcessOptions) => {
        return Promise.resolve();
      },

      canKeepSource: (_target: ExecutionTarget) => {
        return true;
      },

      markdownForFile(file: string): Promise<MappedString> {
        if (quarto.jupyter.isPercentScript(file, [".jl"])) {
          return Promise.resolve(
            quarto.mappedString.fromString(
              quarto.jupyter.percentScriptToMarkdown(file),
            ),
          );
        } else {
          return Promise.resolve(quarto.mappedString.fromFile(file));
        }
      },

      execute: async (options: ExecuteOptions): Promise<ExecuteResult> => {
        options.target.source;

        // use daemon by default if we are in an interactive session (terminal
        // or rstudio) and not running in a CI system.
        let executeDaemon = options.format.execute[kExecuteDaemon];
        if (executeDaemon === null || executeDaemon === undefined) {
          executeDaemon = quarto.system.isInteractiveSession() &&
            !quarto.system.runningInCI();
        }

        const execOptions = {
          ...options,
          target: {
            ...options.target,
            input: quarto.path.absolute(options.target.input),
          },
        };

        const juliaExecOptions: JuliaExecuteOptions = {
          oneShot: !executeDaemon,
          ...execOptions,
        };

        // TODO: executeDaemon can take a number for timeout of kernels, but
        // QuartoNotebookRunner currently doesn't support that
        const nb = await executeJulia(juliaExecOptions);

        if (!nb) {
          quarto.console.error("Execution of notebook returned undefined");
          return Promise.reject();
        }

        // NOTE: the following is all mostly copied from the jupyter kernel file

        // there isn't really a "kernel" as we don't execute via Jupyter
        // but this seems to be needed later to assign the correct language markers to code cells etc.)
        nb.metadata.kernelspec = {
          display_name: "Julia",
          name: "julia",
          language: "julia",
        };

        const assets = quarto.jupyter.assets(
          options.target.input,
          options.format.pandoc.to,
        );

        // Write notebook to file if keep-ipynb is set (must happen before
        // toMarkdown which mutates nb in place)
        if (options.format.execute[kKeepIpynb]) {
          const stem = options.target.source.replace(/\.[^.]+$/, "");
          const ipynbPath = stem + ".ipynb";
          writeFileSync(ipynbPath, JSON.stringify(nb, null, 2));
        }

        // NOTE: for perforance reasons the 'nb' is mutated in place
        // by jupyterToMarkdown (we don't want to make a copy of a
        // potentially very large notebook) so should not be relied
        // on subseuqent to this call

        const result = await quarto.jupyter.toMarkdown(
          nb,
          {
            executeOptions: options,
            language: nb.metadata.kernelspec.language.toLowerCase(),
            assets,
            execute: options.format.execute,
            keepHidden: options.format.render[kKeepHidden] as boolean | undefined,
            toHtml: quarto.format.isHtmlCompatible(options.format),
            toLatex: quarto.format.isLatexOutput(options.format.pandoc),
            toMarkdown: quarto.format.isMarkdownOutput(options.format),
            toIpynb: quarto.format.isIpynbOutput(options.format.pandoc),
            toPresentation: quarto.format.isPresentationOutput(
              options.format.pandoc,
            ),
            figFormat: options.format.execute[kFigFormat] as string | undefined,
            figDpi: options.format.execute[kFigDpi] as number | undefined,
            figPos: options.format.render[kFigPos] as string | undefined,
            // preserveCellMetadata,
            preserveCodeCellYaml:
              options.format.render[kIpynbProduceSourceNotebook] === true,
          },
        );

        // return dependencies as either includes or raw dependencies
        let includes: PandocIncludes | undefined;
        let engineDependencies: Record<string, Array<unknown>> | undefined;
        if (options.dependencies) {
          includes = quarto.jupyter.resultIncludes(
            options.tempDir,
            result.dependencies,
          );
        } else {
          const dependencies = quarto.jupyter.resultEngineDependencies(
            result.dependencies,
          );
          if (dependencies) {
            engineDependencies = {
              [kJuliaEngine]: dependencies,
            };
          }
        }

        // Create markdown from the result
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

        // return results
        return {
          engine: kJuliaEngine,
          markdown: markdown,
          supporting: [join(assets.base_dir, assets.supporting_dir)],
          filters: [],
          pandoc: result.pandoc,
          includes,
          engineDependencies,
          preserve: result.htmlPreserve,
          postProcess: result.htmlPreserve &&
            (Object.keys(result.htmlPreserve).length > 0),
        };
      },

      target: (
        file: string,
        _quiet?: boolean,
        markdown?: MappedString,
      ): Promise<ExecutionTarget | undefined> => {
        if (markdown === undefined) {
          markdown = quarto.mappedString.fromFile(file);
        }
        const target: ExecutionTarget = {
          source: file,
          input: file,
          markdown,
          metadata: quarto.markdownRegex.extractYaml(markdown.value),
        };
        return Promise.resolve(target);
      },
    };
  },
};

export default juliaEngineDiscovery;

function juliaCmd() {
  return process.env.QUARTO_JULIA ?? "julia";
}

// The child gets exactly the environment it is given (unlike Deno, Node's
// `env` replaces rather than merges), so overrides are layered on process.env.
function withEnv(overrides: Record<string, string>) {
  return { ...process.env, ...overrides };
}

// Run a command with the terminal's stdout/stderr and wait for it. Resolves
// to whether it exited with status 0; rejects if it could not be started.
function runInherited(
  cmd: string,
  args: string[],
  env?: Record<string, string | undefined>,
): Promise<boolean> {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      env,
      stdio: ["ignore", "inherit", "inherit"],
    });
    child.once("error", (e: NodeJS.ErrnoException) =>
      reject(
        e.code === "ENOENT"
          ? new Error(`Could not run "${cmd}": command not found.`)
          : e,
      ));
    child.once("close", (code) => resolve(code === 0));
  });
}

// Run a command to completion capturing its output, for the launchers that
// start the detached server. This returns only once the launcher's stdout
// pipe is closed everywhere, so the detached server must not inherit it (the
// julia launcher script redirects the server's stdio for that reason).
function runCaptured(
  cmd: string,
  args: string[],
  env: Record<string, string | undefined>,
) {
  const result = spawnSync(cmd, args, { encoding: "utf8", env });
  if (result.error) {
    const code = (result.error as NodeJS.ErrnoException).code;
    throw code === "ENOENT"
      ? new Error(`Could not run "${cmd}": command not found.`)
      : result.error;
  }
  if (result.status !== 0) {
    throw new Error(result.stderr);
  }
}

function powershell_argument_list_to_string(...args: string[]): string {
  // formats as '"arg 1" "arg 2" "arg 3"'
  const inner = args.map((arg) => `"${arg}"`).join(" ");
  return `'${inner}'`;
}

async function startOrReuseJuliaServer(
  options: JuliaExecuteOptions,
): Promise<{ reused: boolean }> {
  const transportFile = juliaTransportFile();
  if (!existsSync(transportFile)) {
    trace(
      options,
      `Transport file ${transportFile} doesn't exist`,
    );
    quarto.console.info("Starting julia control server process. This might take a while...");

    let juliaProject = process.env.QUARTO_JULIA_PROJECT;

    if (juliaProject === undefined) {
      await ensureQuartoNotebookRunnerEnvironment(options);
      juliaProject = quarto.path.runtime("julia");
    } else {
      juliaProject = quarto.path.toForwardSlashes(juliaProject);
      trace(
        options,
        `Custom julia project set via QUARTO_JULIA_PROJECT="${juliaProject}". Checking if QuartoNotebookRunner can be loaded.`,
      );
      const qnrLoadable = await runInherited(
        juliaCmd(),
        [
          "--startup-file=no",
          `--project=${juliaProject}`,
          "-e",
          "using QuartoNotebookRunner",
        ],
        // ignore the main env
        withEnv({ JULIA_LOAD_PATH: isWindows ? "@;@stdlib" : "@:@stdlib" }),
      );
      if (!qnrLoadable) {
        throw Error(
          `Executing \`using QuartoNotebookRunner\` failed with QUARTO_JULIA_PROJECT="${juliaProject}". Ensure that this project exists, has QuartoNotebookRunner installed and is instantiated correctly.`,
        );
      }
      trace(
        options,
        `QuartoNotebookRunner could be loaded successfully.`,
      );
    }

    // We need to spawn the julia server in its own process that can outlive quarto.
    // Apparently, `run(detach(cmd))` in julia does not do that reliably on Windows,
    // at least deno never seems to recognize that the spawning julia process has finished,
    // presumably because it waits for the active child process to exit. Node's spawnSync
    // waits on the child's pipes the same way. This makes the
    // tests on windows hang forever if we use the same launching mechanism as for Unix systems.
    // So we utilize powershell instead which can start completely detached processes with
    // the Start-Process commandlet.
    if (isWindows) {
      trace(
        options,
        "Starting detached julia server through powershell, once transport file exists, server should be running.",
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
            juliaServerLogFile(),
          ),
          "-WindowStyle",
          "Hidden",
        ],
        withEnv({ JULIA_LOAD_PATH: "@;@stdlib" }), // ignore the main env
      );
    } else {
      trace(
        options,
        "Starting detached julia server through julia, once transport file exists, server should be running.",
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
          juliaServerLogFile(),
        ],
        withEnv({ JULIA_LOAD_PATH: "@:@stdlib" }), // ignore the main env
      );
    }
  } else {
    trace(
      options,
      `Transport file ${transportFile} exists, reusing server.`,
    );
    return { reused: true };
  }
  return { reused: false };
}

async function ensureQuartoNotebookRunnerEnvironment(
  options: JuliaExecuteOptions,
) {
  const runtimeDir = quarto.path.runtime("julia");
  const projectTomlTemplate = join(extensionDir, "Project.toml");
  const projectToml = join(runtimeDir, "Project.toml");
  writeFileSync(projectToml, readFileSync(projectTomlTemplate));
  const success = await runInherited(juliaCmd(), [
    "--startup-file=no",
    `--project=${runtimeDir}`,
    join(extensionDir, "ensure_environment.jl"),
  ]);
  if (!success) {
    throw (new Error("Ensuring an updated julia server environment failed"));
  }
  return Promise.resolve();
}

interface JuliaTransportFile {
  port: number;
  pid: number;
  key: string;
  juliaVersion: string;
  environment: string;
  runnerVersion: string;
}

async function pollTransportFile(
  options: JuliaExecuteOptions,
): Promise<JuliaTransportFile> {
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

async function readTransportFile(
  transportFile: string,
): Promise<JuliaTransportFile> {
  // As we know the json file ends with \n but we might accidentally read
  // it too quickly once it exists, for example when not the whole string
  // has been written to it, yet, we just repeat reading until the string
  // ends in a newline. The overhead doesn't matter as the file is so small.
  let content = readFileSync(transportFile, "utf8");
  let i = 0;
  while (i < 20 && !content.endsWith("\n")) {
    await delay(100);
    content = readFileSync(transportFile, "utf8");
    i += 1;
  }
  if (!content.endsWith("\n")) {
    throw ("Read invalid transport file that did not end with a newline");
  }
  return JSON.parse(content) as JuliaTransportFile;
}

async function getReadyServerConnection(
  transportOptions: JuliaTransportFile,
  executeOptions: JuliaExecuteOptions,
) {
  return await connectReady(
    transportOptions.port,
    transportOptions.key,
    10000,
    (msg) => trace(executeOptions, msg),
  );
}

async function getJuliaServerConnection(
  options: JuliaExecuteOptions,
): Promise<LineConnection> {
  const { reused } = await startOrReuseJuliaServer(options);

  let transportOptions: JuliaTransportFile;
  try {
    transportOptions = await pollTransportFile(options);
  } catch (err) {
    if (!reused) {
      quarto.console.info(
        "No transport file was found after the timeout. This is the log from the server process:",
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
    `Connecting to server at port ${transportOptions.port}, pid ${transportOptions.pid}`,
  );

  try {
    const conn = await getReadyServerConnection(transportOptions, options);
    if (typeof conn === "string") {
      // timed out or otherwise not ready
      throw new Error(conn);
    } else {
      return conn;
    }
  } catch (e) {
    if (reused) {
      trace(
        options,
        "Connecting to server failed, a transport file was reused so it might be stale. Delete transport file and retry.",
      );
      safeRemoveSync(juliaTransportFile());
      return await getJuliaServerConnection(options);
    } else {
      quarto.console.error(
        "Connecting to server failed. A transport file was successfully created by the server process, so something in the server process might be broken.",
      );
      throw e;
    }
  }
}

function firstSignificantLine(str: string, n: number): string {
  const lines = str.split("\n");

  for (const line of lines) {
    const trimmedLine = line.trim();
    // Check if the line is significant
    if (!trimmedLine.startsWith("#|") && trimmedLine !== "") {
      // Return line as is if its length is less than or equal to n
      if (trimmedLine.length <= n) {
        return trimmedLine;
      } else {
        // Return substring up to n characters with an ellipsis
        return trimmedLine.substring(0, n - 1) + "…";
      }
    }
  }

  // Return empty string if no significant line found
  return "";
}

// from cliffy, MIT licensed
function getConsoleColumns(): number | null {
  try {
    // columns is undefined when stdout is not a tty
    return process.stdout.columns ?? null;
  } catch (_error) {
    return null;
  }
}

function buildSourceRanges(
  markdown: MappedString,
): Array<SourceRange> {
  const lines = quarto.mappedString.splitLines(markdown);
  const sourceRanges: Array<SourceRange> = [];
  let currentRange: SourceRange | null = null;

  lines.forEach((line, index) => {
    // Get mapping info directly from the line's MappedString
    const mapResult = line.map(0, true);
    if (mapResult) {
      const { originalString } = mapResult;
      const lineCol = quarto.mappedString.indexToLineCol(
        originalString,
        mapResult.index,
      );
      const fileName = originalString.fileName
        ? resolve(originalString.fileName) // resolve to absolute path using cwd
        : undefined;
      const sourceLineNum = lineCol.line;

      // Check if this line continues the current range
      if (
        currentRange &&
        currentRange.file === fileName &&
        fileName !== undefined &&
        currentRange.sourceLines &&
        currentRange.sourceLines[1] === sourceLineNum
      ) {
        // Extend current range
        currentRange.lines[1] = index + 1; // +1 because lines are 1-indexed
        currentRange.sourceLines[1] = sourceLineNum + 1;
      } else {
        // Start new range
        if (currentRange) {
          sourceRanges.push(currentRange);
        }
        currentRange = {
          lines: [index + 1, index + 1], // +1 because lines are 1-indexed
        };
        if (fileName !== undefined) {
          currentRange.file = fileName;
          currentRange.sourceLines = [sourceLineNum + 1, sourceLineNum + 1];
        }
      }
    } else {
      // No mapping available - treat as separate range
      if (currentRange) {
        sourceRanges.push(currentRange);
        currentRange = null;
      }
    }
  });

  // Don't forget the last range
  if (currentRange) {
    sourceRanges.push(currentRange);
  }

  return sourceRanges;
}

async function executeJulia(
  options: JuliaExecuteOptions,
): Promise<JupyterNotebook> {
  const conn = await getJuliaServerConnection(options);
  const transportOptions = await pollTransportFile(options);
  const file = options.target.input;

  // Adapter: worker-close speaks a narrow close-command vocabulary; the generic
  // writeJuliaCommand covers the full ServerCommand union. Every command
  // worker-close emits (isopen/close/forceclose) is a valid ServerCommand.
  const writeCloseCommand: CloseCommandWriter = (command) =>
    // deno-lint-ignore no-explicit-any
    writeJuliaCommand(conn, command as any, transportOptions.key, options);

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
      (update: ProgressUpdate) => {
        const n = update.nChunks.toString();
        const i = update.chunkIndex.toString();
        const i_padded = `${" ".repeat(n.length - i.length)}${i}`;
        const ncols = getConsoleColumns() ?? 80;
        const firstPart = `Running [${i_padded}/${n}] at line ${update.line}:  `;
        const firstPartLength = firstPart.length;
        const sigLine = firstSignificantLine(
          update.source,
          Math.max(0, ncols - firstPartLength),
        );
        quarto.console.info(`${firstPart}${sigLine}`);
      },
    );
  } catch (e) {
    // A failed run must still close the oneShot worker, or it leaks on the
    // shared control server (and blocks the server's idle timeout). Best
    // effort — the run error rethrown below is the diagnostic that matters.
    if (options.oneShot) {
      await errorRunClose(
        writeCloseCommand,
        file,
        (message) => quarto.console.warning(message),
      );
    }
    throw e;
  }

  if (options.oneShot) {
    await postRunClose(
      writeCloseCommand,
      file,
      (message) => quarto.console.warning(message),
    );
  }

  return response.notebook;
}

type empty = Record<string | number | symbol, never>;

type ServerCommand =
  | {
    type: "run";
    content: {
      file: string;
      options: JuliaExecuteOptions;
      sourceRanges: Array<SourceRange>;
    };
  }
  | { type: "close"; content: { file: string } }
  | { type: "forceclose"; content: { file: string } }
  | { type: "isopen"; content: { file: string } }
  | { type: "stop"; content: empty }
  | { type: "isready"; content: empty }
  | { type: "status"; content: empty };

type ServerCommandResponseMap = {
  run: { notebook: JupyterNotebook };
  close: { status: true };
  forceclose: { status: true };
  stop: { message: "Server stopped." };
  isopen: boolean;
  isready: true;
  status: string;
};

type ServerCommandResponse<T extends ServerCommand["type"]> =
  ServerCommandResponseMap[T];

async function writeJuliaCommand<T extends ServerCommand["type"]>(
  conn: LineConnection,
  command: Extract<ServerCommand, { type: T }>,
  secret: string,
  options: JuliaExecuteOptions,
  onProgressUpdate?: (update: ProgressUpdate) => void,
): Promise<ServerCommandResponse<T>> {
  return await sendCommand(
    conn,
    command,
    secret,
    (msg) => trace(options, msg),
    onProgressUpdate,
  );
}

function juliaRuntimeDir(): string {
  try {
    return quarto.path.runtime("julia");
  } catch (e) {
    quarto.console.error("Could not create julia runtime directory.");
    quarto.console.error(
      "This is possibly a permission issue in the environment Quarto is running in.",
    );
    quarto.console.error(
      "Please consult the following documentation for more information:",
    );
    quarto.console.error(
      "https://github.com/quarto-dev/quarto-cli/issues/4594#issuecomment-1619177667",
    );
    throw e;
  }
}

export function juliaTransportFile() {
  return join(juliaRuntimeDir(), "julia_transport.txt");
}

export function juliaServerLogFile() {
  return join(juliaRuntimeDir(), "julia_server_log.txt");
}

function trace(options: ExecuteOptions, msg: string) {
  if (options.format?.execute[kExecuteDebug] === true) {
    quarto.console.info("- " + msg, { bold: true });
  }
}

function populateJuliaEngineCommand(command: Command) {
  command
    .command("status", "Status")
    .description(
      "Get status information on the currently running Julia server process.",
    ).action(logStatus)
    .command("kill", "Kill server")
    .description(
      "Kill the control server if it is currently running. This will also kill all notebook worker processes.",
    )
    .action(killJuliaServer)
    .command("log", "Print julia server log")
    .description(
      "Print the content of the julia server log file if it exists which can be used to diagnose problems.",
    )
    .action(printJuliaServerLog)
    .command(
      "close",
      "Close the worker for a given notebook. If it is currently running, it will not be interrupted.",
    )
    .arguments("<file:string>")
    .option(
      "-f, --force",
      "Force closing. This will terminate the worker if it is running.",
      { default: false },
    )
    .action(async (options: { force: boolean }, file: string) => {
      await closeWorker(file, options.force);
    })
    .command("stop", "Stop the server")
    .description(
      "Send a message to the server that it should close all notebooks and exit. This will fail if any notebooks are not idle.",
    )
    .action(stopServer);
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
    {} as JuliaExecuteOptions,
  );
  const successfullyConnected = typeof conn !== "string";

  if (successfullyConnected) {
    const status: string = await writeJuliaCommand(
      conn,
      { type: "status", content: {} },
      transportOptions.key,
      {} as JuliaExecuteOptions,
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

// todo: this could use a refactor with the other functions that make
// server connections or execute commands, this one is just supposed to
// simplify the pattern where a running server is expected (there will be an error if there is none)
// and we want to get the API response out quickly
async function connectAndWriteJuliaCommandToRunningServer<
  T extends ServerCommand["type"],
>(
  command: Extract<ServerCommand, { type: T }>,
): Promise<ServerCommandResponse<T>> {
  const transportFile = juliaTransportFile();
  if (!existsSync(transportFile)) {
    throw new Error("Julia control server is not running.");
  }
  const transportOptions = await readTransportFile(transportFile);

  const conn = await getReadyServerConnection(
    transportOptions,
    {} as JuliaExecuteOptions,
  );
  const successfullyConnected = typeof conn !== "string";

  if (successfullyConnected) {
    const result = await writeJuliaCommand(
      conn,
      command,
      transportOptions.key,
      {} as JuliaExecuteOptions,
    );
    conn.close();
    return result;
  } else {
    throw new Error(
      `Found transport file but can't connect to control server.`,
    );
  }
}

async function closeWorker(file: string, force: boolean) {
  const absfile = quarto.path.absolute(file);
  await connectAndWriteJuliaCommandToRunningServer({
    type: force ? "forceclose" : "close",
    content: { file: absfile },
  });
  quarto.console.info(`Worker ${force ? "force-" : ""}closed successfully.`);
}

async function stopServer() {
  const result = await connectAndWriteJuliaCommandToRunningServer({
    type: "stop",
    content: {},
  });
  quarto.console.info(result.message);
}
