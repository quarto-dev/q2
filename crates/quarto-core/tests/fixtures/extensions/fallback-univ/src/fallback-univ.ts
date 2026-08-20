/**
 * fallback-univ — synthetic universal-Fallback engine fixture (Plan 4b Task A1).
 *
 * Declares a universal Fallback via the `_extension.yml` `claims.fallback` key
 * (`{ priority: 5 }`) — no per-language entries. `TsEngine::claims_language`
 * (`crates/quarto-core/src/engine/ts_engine.rs`) consults the `fallback` map
 * key whenever a probed language has no direct static entry, so this engine
 * catches ANY language no Primary claims (T2 explicit / T4 implicit Fallback).
 * Priority 5 is deliberately higher than the built-in `JupyterEngine`'s
 * universal `Fallback(0)` so this fixture wins deterministically regardless
 * of registration order or `jupyter`/`knitr` availability on the test
 * machine.
 *
 * Echo-shaped: no daemon, transforms ANY `{<lang>}` fenced block in-process
 * (generic, since a universal fallback cannot know its language ahead of
 * time), mirroring `../../echo-engine/src/echo-engine.ts`.
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
import { fallback } from "@quarto/api/claims";

// Stash the QuartoAPI reference set during init().
let _quarto: QuartoAPI | undefined;

const fallbackUnivEngine: ExecutionEngineDiscovery = {
  name: "fallback-univ",
  defaultExt: ".fallbackuniv",
  defaultYaml: (_kernel?: string) => [],
  defaultContent: (_kernel?: string) => [],
  validExtensions: () => [],
  canFreeze: false,
  generatesFigures: false,

  init(quarto: QuartoAPI): void {
    _quarto = quarto;
  },

  claimsLanguage(
    _language: string,
    _firstClass?: string,
  ): boolean | number | LanguageClaim | null {
    // Universal: claim ANY language as Fallback(5), matching the static
    // `claims.fallback: { priority: 5 }` declaration exactly.
    return fallback(5);
  },

  claimsFile(_file: string, _ext: string): boolean {
    return false;
  },

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
    return {
      name: "fallback-univ",
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
        // Generic: transform ANY `{<lang>}` fenced block, since a universal
        // fallback is asked to handle whatever language resolution routes to
        // it (a concrete language name is chosen by the caller/test, not
        // known ahead of time by this engine).
        // Marker avoids markdown-special characters (e.g. `[...]`, which
        // Pandoc's bracketed-span extension turns into a `<span>`) so the
        // rendered HTML contains the marker as literal text.
        const input = opts.target.markdown.value;
        const executed = input.replace(
          /```\{([a-zA-Z0-9_.]+)\}[\s\S]*?```/g,
          (_match, lang: string) =>
            "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n" +
            `**FALLBACK_UNIV_EXECUTED_${lang}**\n:::\n:::`,
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

export default fallbackUnivEngine;
