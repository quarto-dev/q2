/**
 * echo-legacy — dynamic / legacy TS execution engine fixture.
 *
 * Declares ONLY a path in _extension.yml (no name, no claims, no file-extensions).
 * This triggers the missing-static-fields warning + dynamic load path (P3-2).
 *
 * Runtime name: "echolegacy".  Claims the "echolegacy" language.
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

let _quarto: QuartoAPI | undefined;

const echoLegacyEngine: ExecutionEngineDiscovery = {
  name: "echolegacy",
  defaultExt: ".qmd",
  defaultYaml: (_kernel?: string) => [],
  defaultContent: (_kernel?: string) => [],
  validExtensions: () => [],
  canFreeze: false,
  generatesFigures: false,

  init(quarto: QuartoAPI): void {
    _quarto = quarto;
  },

  claimsLanguage(language: string, _firstClass?: string): boolean | number {
    return language === "echolegacy";
  },

  claimsFile(_file: string, _ext: string): boolean {
    // Legacy engines don't claim files statically.
    return false;
  },

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
    return {
      name: "echolegacy",
      canFreeze: false,

      async markdownForFile(file: string) {
        const text = Deno.readTextFileSync(file);
        const wrapped = "```{echolegacy}\n" + text + "\n```\n";
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
        // Transform only {echolegacy} fenced blocks; leave others untouched.
        const input = opts.target.markdown.value;
        const executed = input.replace(
          /```\{echolegacy\}[\s\S]*?```/g,
          "**ECHOLEGACY_EXECUTED**",
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

export default echoLegacyEngine;
