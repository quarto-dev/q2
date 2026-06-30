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

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
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
        // Transform only {echo} fenced blocks → **ECHO_EXECUTED**;
        // leave every other cell (e.g. {python}) untouched (pass-through).
        const input = opts.target.markdown.value;
        const executed = input.replace(
          /```\{echo\}[\s\S]*?```/g,
          "**ECHO_EXECUTED**",
        );
        return {
          markdown: executed,
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
