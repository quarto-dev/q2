/**
 * echo-engine — static, fully-declared TS execution engine fixture.
 *
 * Claims the "echo" language and ".echo" file extension.
 * Used by Task 13 / Task 14 integration tests.
 */

import type {
  ExecutionEngineDiscovery,
  ExecutionEngineInstance,
  ExecutionTarget,
  ExecuteOptions,
  ExecuteResult,
  DependenciesOptions,
  DependenciesResult,
  PostProcessOptions,
  EngineProjectContext,
  PartitionedMarkdown,
  Format,
  QuartoAPI,
} from "@quarto/types";

// Stash the QuartoAPI reference set during init().
let _quarto: QuartoAPI | undefined;

const echoEngine: ExecutionEngineDiscovery = {
  name: "echo",
  defaultExt: ".echo",
  defaultYaml: (_kernel?: string) => [],
  defaultContent: (_kernel?: string) => [],
  validExtensions: () => [".echo"],
  canFreeze: false,
  generatesFigures: false,

  init(quarto: QuartoAPI): void {
    _quarto = quarto;
  },

  claimsLanguage(language: string, _firstClass?: string): boolean | number {
    return language === "echo";
  },

  claimsFile(_file: string, ext: string): boolean {
    return ext === ".echo";
  },

  launch(context: EngineProjectContext): ExecutionEngineInstance {
    // P1.1: capture the per-render project context so execute() can echo it
    // back. This binds the `LaunchEngine { project }` wiring: the value read
    // here is whatever `TsEngine::set_project` stored before the FIRST launch.
    const capturedContext = context;
    const contextMarker = (): string => {
      // JSON.stringify drops the function members (getOutputDirectory, …), so
      // echo an EXPLICIT object that CALLS getOutputDirectory() — this carries
      // the RESOLVED absolute output dir alongside the RAW relative
      // config.project.outputDir, letting the e2e assert raw-vs-resolved.
      const echoed = {
        dir: capturedContext.dir,
        isSingleFile: capturedContext.isSingleFile,
        config: capturedContext.config ?? null,
        outputDir: capturedContext.getOutputDirectory
          ? capturedContext.getOutputDirectory()
          : null,
      };
      // Emit inside a fenced code block so Pandoc renders it verbatim (no
      // smart-quoting / escaping of the JSON's double quotes).
      return (
        "\n\n```\nCONTEXT_JSON_START" +
        JSON.stringify(echoed) +
        "CONTEXT_JSON_END\n```\n"
      );
    };
    // P1.1b: echo the per-execute Format bin (post-`metadataAsFormat`
    // partition on the host side) so the e2e can assert that merged
    // document metadata reached the engine. Echoes the whole `execute`
    // bin (so a non-default binned value like `daemon: false` round-trips)
    // plus ONE asserted `format.metadata` key — NOT the whole format
    // object (the brief is explicit: don't dump the whole format).
    const formatMarker = (opts: ExecuteOptions): string => {
      const echoed = {
        execute: opts.format.execute,
        customKey: (opts.format.metadata as Record<string, unknown>)[
          "echo-custom-key"
        ],
      };
      return (
        "\n\n```\nFORMAT_JSON_START" +
        JSON.stringify(echoed) +
        "FORMAT_JSON_END\n```\n"
      );
    };
    return {
      name: "echo",
      canFreeze: false,

      async markdownForFile(file: string) {
        // Read the file content and wrap it as an {echo} fenced block,
        // plus a second {python} cell so the §8 single-engine pass-through
        // of a non-echo cell is exercised.
        const text = Deno.readTextFileSync(file);
        const wrapped =
          "```{echo}\n" +
          text +
          "\n```\n\n" +
          "```{python}\nprint('not run by echo')\n```\n";

        // MUST use quarto.mappedString.fromString — NOT a bare { value, fileName }
        // literal.  The harness serializes provenance via segments() and ignores a
        // bare sourceMap field, making the literal form a silent no-op bug.
        return _quarto!.mappedString.fromString(wrapped, file);
      },

      async target(
        file: string,
        _quiet?: boolean,
        markdown?,
      ): Promise<ExecutionTarget | undefined> {
        const ms =
          markdown ?? (await this.markdownForFile(file));
        return {
          source: file,
          input: file,
          markdown: ms,
          metadata: {},
          data: undefined,
        };
      },

      async partitionedMarkdown(
        file: string,
        _format?: Format,
      ): Promise<PartitionedMarkdown> {
        const ms = await this.markdownForFile(file);
        return {
          markdown: ms.value,
          yaml: undefined,
          headingText: undefined,
          headingAttr: undefined,
          containsRefs: false,
          srcMarkdownNoYaml: ms.value,
        };
      },

      async execute(opts: ExecuteOptions): Promise<ExecuteResult> {
        // Transform only {echo} fenced blocks → an executed-cell wrapper;
        // leave every other cell (e.g. {python}) untouched (pass-through).
        //
        // The output wraps `**ECHO_EXECUTED**` in a `::: {.cell}` Div carrying a
        // `.cell-output` child — the SAME shape real engines (jupyter/julia via
        // the engine-host's `mdFromCodeCell`) emit for an executed cell. This is
        // load-bearing for the q2-preview capture-splice path: the splice
        // (`derive_cell_outputs` / `is_cell_wrapper`) maps each engine cell to
        // the next `.cell` wrapper in the executed markdown. A bare paragraph
        // (the fixture's earlier shape) has no wrapper, so the splice can't
        // match it and the preview pane stays inert (bd-h4rhohhy / Bug B).
        const input = opts.target.markdown.value;
        const executed = input.replace(
          /```\{echo\}[\s\S]*?```/g,
          "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n" +
            "**ECHO_EXECUTED**\n:::\n:::",
        );
        return {
          markdown: executed + contextMarker() + formatMarker(opts),
          supporting: [],
          filters: [],
        };
      },

      async dependencies(
        _opts: DependenciesOptions,
      ): Promise<DependenciesResult> {
        return {
          includes: {},
        };
      },

      async postprocess(_opts: PostProcessOptions): Promise<void> {
        // nothing to do
      },
    };
  },
};

export default echoEngine;
