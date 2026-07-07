/**
 * whenclass-marimo — synthetic whenClass-conditioned Primary engine fixture
 * (Plan 4b Task A1).
 *
 * Claims "pysynth" as Primary(priority 1) ONLY when the cell's first class is
 * ".marimo" (`_extension.yml`'s `claims.pysynth.whenClass: marimo`). A bare
 * `{pysynth}` cell (no `.marimo` first class) does NOT get claimed — mirrors
 * the real `marimo` fixture's `{python .marimo}` conditioning
 * (`../../marimo/src/marimo-engine.ts`,
 * `../../marimo/_extensions/marimo/_extension.yml`).
 *
 * Echo-shaped: no daemon, transforms `{pysynth .marimo}` cells in-process,
 * mirroring `../../echo-engine/src/echo-engine.ts`.
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
  LanguageClaim,
} from "@quarto/types";
import { primary } from "@quarto/api";

// Stash the QuartoAPI reference set during init().
let _quarto: QuartoAPI | undefined;

const whenclassMarimoEngine: ExecutionEngineDiscovery = {
  name: "whenclass-marimo",
  defaultExt: ".whenclassmarimo",
  defaultYaml: (_kernel?: string) => [],
  defaultContent: (_kernel?: string) => [],
  validExtensions: () => [],
  canFreeze: false,
  generatesFigures: false,

  init(quarto: QuartoAPI): void {
    _quarto = quarto;
  },

  claimsLanguage(
    language: string,
    firstClass?: string,
  ): boolean | number | LanguageClaim | null {
    if (language === "pysynth" && firstClass === "marimo") {
      return primary(1);
    }
    // Bare {pysynth} (no .marimo first class) is NOT claimed.
    return false;
  },

  claimsFile(_file: string, _ext: string): boolean {
    return false;
  },

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
    return {
      name: "whenclass-marimo",
      canFreeze: false,

      async markdownForFile(file: string) {
        const text = Deno.readTextFileSync(file);
        return _quarto!.mappedString.fromString(text, file);
      },

      async target(
        file: string,
        _quiet?: boolean,
        markdown?,
      ): Promise<ExecutionTarget | undefined> {
        const ms = markdown ?? (await this.markdownForFile(file));
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
        // Transform only `{pysynth .marimo}` fenced blocks; a bare
        // `{pysynth}` block is never routed here (whenClass gate denies the
        // claim), so it is never matched.
        const input = opts.target.markdown.value;
        const executed = input.replace(
          /```\{pysynth\s+\.marimo\}[\s\S]*?```/g,
          "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n" +
            "**WHENCLASS_MARIMO_EXECUTED**\n:::\n:::",
        );
        return { markdown: executed, supporting: [], filters: [] };
      },

      async dependencies(
        _opts: DependenciesOptions,
      ): Promise<DependenciesResult> {
        return { includes: {} };
      },

      async postprocess(_opts: PostProcessOptions): Promise<void> {
        // nothing to do
      },
    };
  },
};

export default whenclassMarimoEngine;
