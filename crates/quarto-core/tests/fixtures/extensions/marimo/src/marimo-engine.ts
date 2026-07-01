/*
 * marimo-engine.ts
 *
 * Quarto external engine for marimo
 */

import { fromFileUrl, join, dirname } from "path";
import { delay } from "https://deno.land/std@0.224.0/async/delay.ts";

import type {
  DependenciesOptions,
  DependenciesResult,
  ExecuteOptions,
  ExecuteResult,
  ExecutionEngineDiscovery,
  ExecutionTarget,
  ExecutionEngineInstance,
  LanguageClaim,
  MappedString,
  PandocIncludes,
  PostProcessOptions,
  EngineProjectContext,
  QuartoAPI,
} from "@quarto/types";

import {
  MARIMO_CELL_REGEX,
  MARIMO_CELL_REGEX_WITH_BARE_SQL,
} from "../lib/cell-execution-regex.ts";
import { cellOwnedByMarimo } from "../lib/is-marimo-cell.ts";
import { renderOutput } from "../lib/render-output.ts";
import type { MarimoOutput } from "../lib/render-output.ts";

let quarto: QuartoAPI;


// Type for marimo execution result from extract.py
interface MarimoExecutionResult {
  header: string;
  outputs: MarimoOutput[];
  count: number;
}

export function containsMarimoFence(markdown: string): boolean {
  return markdown
    .split(/\r?\n/)
    .some((line) => MARIMO_CELL_REGEX.test(line));
}

// Helper function to execute external processes
async function executePython(
  command: string,
  args: string[] = [],
  stdin: string = ""
): Promise<string> {
  const cmd = new Deno.Command(command, {
    args: args,
    stdin: "piped",
    stdout: "piped",
    stderr: "piped",
  });

  const process = cmd.spawn();

  // Input handling
  if (stdin) {
    const writer = process.stdin.getWriter();
    const encoder = new TextEncoder();
    await writer.write(encoder.encode(stdin));
    await writer.close();
  } else {
    process.stdin.close();
  }

  // Wait for process to complete
  const output = await process.output();

  const decoder = new TextDecoder();

  // Log stderr if present
  const stderr = decoder.decode(output.stderr);
  if (stderr) {
    quarto.console.info(`Subprocess stderr: ${stderr}`);
  }

  // Handle errors
  if (!output.success) {
    throw new Error(`Process execution failed: ${stderr}`);
  }

  // Return output
  return decoder.decode(output.stdout);
}

// Construct UV command for dependencies
async function constructUvCommand(header: string): Promise<string[]> {
  // Get the directory of the current module
  const currentDir = dirname(fromFileUrl(import.meta.url));

  // Create a platform-appropriate path to the script
  const scriptPath = join(currentDir, "command.py");

  // Run uv with correct arguments (matching the Lua implementation)
  const result = await executePython(
    "uv",
    ["run", "--with", "marimo", scriptPath],
    header
  );

  return JSON.parse(result);
}

// Build the subprocess command for extract.py, based on environment mode
// (SC19, q2 plan4c 4cE): `external-env` metadata selects a bare `python`
// invocation; otherwise the uv-managed environment is used, with dependency
// flags resolved via `getUvFlags` (defaults to the real `constructUvCommand`,
// which shells out to `uv`; tests inject a stub here).
export async function buildCommand(
  metadata: Record<string, unknown>,
  extractPath: string,
  getUvFlags: (header: string) => Promise<string[]> = constructUvCommand,
): Promise<string[]> {
  const useExternalEnv = metadata["external-env"] === true;

  if (useExternalEnv) {
    return ["python", extractPath];
  }

  const pyprojectConfig = metadata["pyproject"];
  const header = pyprojectConfig ? String(pyprojectConfig) : "";
  const uvFlags = await getUvFlags(header);
  return ["uv", ...uvFlags, extractPath];
}

// Convert HTML to markdown using pandoc (for PDF output)
async function htmlToMarkdown(html: string): Promise<string> {
  const result = await quarto.system.pandoc(
    ["-f", "html", "-t", "markdown"],
    html
  );
  if (!result.success) {
    quarto.console.warning(`Pandoc conversion failed: ${result.stderr}`);
    return html; // Fall back to original HTML
  }
  return result.stdout || "";
}

const marimoEngineDiscovery: ExecutionEngineDiscovery = {
  init: (quartoAPI: QuartoAPI) => {
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
    "```",
  ],

  validExtensions: () => [".qmd", ".md"],

  // When Quarto's language discovery sees `python {...}`
  // then it will route to Python engine without scanning block contents.
  // We want `python {.marimo}` blocks route to our engine, so we override the default claim logic.
  claimsFile: (file: string, ext: string) => {
    if (![".qmd", ".md"].includes(ext.toLowerCase())) {
      return false;
    }

    try {
      return containsMarimoFence(Deno.readTextFileSync(file));
    } catch {
      return false;
    }
  },

  claimsLanguage: (
    language: string,
    firstClass?: string,
  ): boolean | number | LanguageClaim => {
    // Claim {python .marimo} / {sql .marimo} with priority 2 (overrides Jupyter/sql fallback)
    if ((language === "python" || language === "sql") && firstClass === "marimo") {
      return 2;
    }
    // Claim {python.marimo} / {sql.marimo} with priority 1 (no competition for this language)
    if (language === "python.marimo" || language === "sql.marimo") {
      return 1;
    }
    // Bare `{sql}` rides along as Interop when marimo is already present via
    // a primary python/sql claim elsewhere in the doc (Option B; q2 plan4c
    // corrections 3/9/12). Only sql is Interop — bare `{python}` stays
    // unclaimed (falls through to `false` below).
    if (language === "sql") {
      return { kind: "interop" };
    }
    return false; // Don't claim other python/sql blocks
  },

  canFreeze: false,

  generatesFigures: true,

  checkInstallation: async () => {
    await quarto.console.withSpinner(
      { message: "Checking Marimo installation..." },
      async () => {
        await delay(2000);
      },
    );
  },

  launch: (context: EngineProjectContext): ExecutionEngineInstance => {
    return {
      name: marimoEngineDiscovery.name,
      canFreeze: marimoEngineDiscovery.canFreeze,

      markdownForFile(file: string): Promise<MappedString> {
        return Promise.resolve(quarto.mappedString.fromFile(file));
      },

      target: (
        file: string,
        _quiet?: boolean,
        markdown?: MappedString
      ): Promise<ExecutionTarget | undefined> => {
        const md = markdown ?? quarto.mappedString.fromFile(file);
        const metadata = quarto.markdownRegex.extractYaml(md.value);
        return Promise.resolve({
          source: file,
          input: file,
          markdown: md,
          metadata,
        });
      },

      partitionedMarkdown: (file: string) => {
        return Promise.resolve(
          quarto.markdownRegex.partition(Deno.readTextFileSync(file))
        );
      },

      execute: async (options: ExecuteOptions): Promise<ExecuteResult> => {
        const { target, format } = options;
        const markdown = target.markdown.value;

        // Determine MIME sensitivity
        const outputFormat = format.pandoc.to || "html";
        const mimeSensitive = outputFormat === "pdf" || outputFormat === "latex" || outputFormat === "typst";

        // Bare `{sql}` is an Interop language (Option B, q2 plan4c corrections
        // 3/9/12/B1-B2): marimo owns it only when q2's resolver has assigned
        // marimo ownership of "sql" for this render. `handledLanguages` is
        // q2's LEAVE-ALONE set (q2's `EngineResolution::handled_languages_for`,
        // `resolution.rs:292`: q2's built-in HANDLED_LANGUAGES ∪ languages
        // owned by OTHER engines), not a positive "assigned to me" set — so
        // marimo owns "sql" here exactly when "sql" is ABSENT from
        // `handledLanguages` (q2 did not leave it alone for someone else).
        // This is sound because q2's resolver assigns every language present
        // in the document an owner, or hard-fails, before execute() ever
        // runs. Drives both which cells get spliced with execution output
        // below AND the `bare_sql` flag threaded to extract.py (so its
        // rewrite/execution of bare sql cells stays gated the same way, not
        // just the TS-side splice). Fixed 2026-07-02 (q2 plan4c FINDING #4):
        // previously read `handledLanguages.includes("sql")` — backwards,
        // since that is `true` exactly when q2 left sql alone for some OTHER
        // engine, i.e. exactly when marimo does NOT own it.
        const bareSqlOwned = !(options.handledLanguages ?? []).includes(
          "sql",
        );

        try {
          // Get platform-appropriate paths
          const currentDir = dirname(fromFileUrl(import.meta.url));
          const extractPath = join(currentDir, "extract.py");

          // Build command based on environment mode (SC19: factored into
          // buildCommand — see marimo-engine.ts's buildCommand doc comment).
          const [command, ...args] = await buildCommand(
            target.metadata,
            extractPath,
          );

          // Add file, MIME sensitivity, global eval, and bare-sql-ownership
          // arguments. `bare_sql` (argv[4]) is a NEW 4th positional — see B2:
          // extract.py has no other way to learn whether q2 assigned it
          // ownership of bare sql on this render.
          const globalEval = target.metadata["eval"] !== false;
          args.push(
            target.input,
            mimeSensitive ? "yes" : "no",
            globalEval ? "yes" : "no",
            bareSqlOwned ? "yes" : "no",
          );

          // Execute Python script to get cell outputs
          const result = await quarto.console.withSpinner(
            { message: "Executing marimo cells..." },
            async () => {
              return await executePython(command, args, markdown);
            },
          );
          const marimoExecution: MarimoExecutionResult = JSON.parse(result);

          // Break markdown into cells using custom regex for marimo syntax.
          // When marimo owns bare sql on this render (bareSqlOwned), use the
          // wider split matcher so bare `{sql}` cells come back as their own
          // chunk instead of being folded into surrounding markdown text
          // (B1: this widened matcher is NOT the shared MARIMO_CELL_REGEX
          // that containsMarimoFence/claimsFile use).
          const chunks = await quarto.markdownRegex.breakQuartoMd(
            target.markdown,
            false, // validate
            false, // lenient
            bareSqlOwned ? MARIMO_CELL_REGEX_WITH_BARE_SQL : MARIMO_CELL_REGEX,
          );

          // Process each cell, replacing marimo-owned cells with outputs
          const processedCells: string[] = [];
          let marimoIndex = 0;
          const handledLanguages = options.handledLanguages ?? [];

          for (const cell of chunks.cells) {
            if (cellOwnedByMarimo(cell, handledLanguages)) {
              // Replace marimo cell with rendered output
              if (marimoIndex < marimoExecution.outputs.length) {
                const output = marimoExecution.outputs[marimoIndex];
                const rendered = await renderOutput(output, mimeSensitive, htmlToMarkdown);
                processedCells.push(rendered);
              } else {
                quarto.console.warning(
                  `Marimo cell ${marimoIndex} has no corresponding output`
                );
                processedCells.push(cell.sourceVerbatim.value);
              }
              marimoIndex++;
            } else {
              // Pass non-marimo cells through unchanged
              processedCells.push(cell.sourceVerbatim.value);
            }
          }

          // Verify we consumed all outputs
          if (marimoIndex !== marimoExecution.count) {
            quarto.console.warning(
              `Expected ${marimoExecution.count} marimo cells, found ${marimoIndex}`
            );
          }

          const processedMarkdown = processedCells.join("");

          // Setup includes for HTML formats
          const includes: PandocIncludes = {};
          if (outputFormat === "html" && marimoExecution.header) {
            // Write header content to a temp file (like Jupyter does)
            const tempFile = Deno.makeTempFileSync({
              dir: options.tempDir,
              prefix: "marimo-header-",
              suffix: ".html",
            });
            Deno.writeTextFileSync(tempFile, marimoExecution.header);
            includes["include-in-header"] = [tempFile];
          }

          return {
            engine: "marimo",
            markdown: processedMarkdown,
            supporting: [],
            filters: [],
            includes: Object.keys(includes).length > 0 ? includes : undefined,
          };
        } catch (error) {
          quarto.console.error(`Error executing marimo: ${error}`);
          return {
            engine: "marimo",
            markdown: `\`\`\`\nError executing marimo: ${
              (error as Error).message
            }\n\`\`\`\n\n${markdown}`,
            supporting: [],
            filters: [],
          };
        }
      },

      dependencies: (_options: DependenciesOptions): Promise<DependenciesResult> => {
        return Promise.resolve({
          includes: {},
        });
      },

      postprocess: (_options: PostProcessOptions): Promise<void> =>
        Promise.resolve(),
    };
  },
};

export default marimoEngineDiscovery;
