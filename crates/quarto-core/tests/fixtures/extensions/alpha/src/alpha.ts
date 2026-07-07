/**
 * alpha — synthetic Primary-claims engine fixture (Plan 4b Task A1).
 *
 * Claims the "synth" language as Primary(priority 1), identically to `beta`
 * (see `../beta/src/beta.ts`). The (alpha, beta) pair is the
 * equal-kind-equal-priority contention pair Phase 4b-B uses for
 * `contribution_order` / `engines:` tiebreak assertions — THIS fixture is
 * only smoke-tested here in isolation (see
 * `crates/quarto-core/tests/integration/synth_engines_e2e.rs`).
 *
 * Echo-shaped: no daemon, transforms `{synth}` cells in-process, mirroring
 * `../../echo-engine/src/echo-engine.ts`.
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

const alphaEngine: ExecutionEngineDiscovery = {
  name: "alpha",
  defaultExt: ".alpha",
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
    _firstClass?: string,
  ): boolean | number | LanguageClaim | null {
    return language === "synth" ? primary(1) : false;
  },

  claimsFile(_file: string, _ext: string): boolean {
    return false;
  },

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
    return {
      name: "alpha",
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
        // Transform only {synth} fenced blocks; leave everything else untouched
        // (matches the echo-engine single-engine pass-through pattern).
        const input = opts.target.markdown.value;
        const executed = input.replace(
          /```\{synth\}[\s\S]*?```/g,
          "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n" +
            "**ALPHA_EXECUTED**\n:::\n:::",
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

export default alphaEngine;
