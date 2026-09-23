/**
 * synth-echo — extension-subtree infrastructure fixture (plan
 * 2026-09-23-extension-subtree-infrastructure.md, Phase 3).
 *
 * Shaped like a whole vendored repo checked out under
 * `resources/extension-subtrees/<name>/` (this fixture lives at
 * `tests/fixtures/extension-subtrees/synth-echo/`, mirroring that layout),
 * with the actual extension nested at `_extensions/synth-echo/` inside it —
 * exactly how a real Quarto extension repo (e.g. the eventual julia-engine
 * vendor) is structured.
 *
 * Claims the "synthsub" language as Primary(priority 1) and echoes the
 * fenced block's content back inside an executed-cell wrapper, so an e2e
 * render can assert the real Deno engine host actually ran it. Structurally
 * mirrors `../../../extensions/alpha/src/alpha.ts` (the simplest existing
 * synthetic-engine fixture); the echo-back behavior mirrors
 * `../../../extensions/echo-engine/src/echo-engine.ts`.
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
import { primary } from "@quarto/api/claims";

// Stash the QuartoAPI reference set during init().
let _quarto: QuartoAPI | undefined;

const synthEchoEngine: ExecutionEngineDiscovery = {
  name: "synth-echo",
  defaultExt: ".synthsub",
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
    return language === "synthsub" ? primary(1) : false;
  },

  claimsFile(_file: string, _ext: string): boolean {
    return false;
  },

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
    return {
      name: "synth-echo",
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
        // Transform only {synthsub} fenced blocks, echoing the cell's own
        // source back inside the executed-cell wrapper so the e2e test can
        // assert the real engine host actually ran this code.
        const input = opts.target.markdown.value;
        const executed = input.replace(
          /```\{synthsub\}\n([\s\S]*?)```/g,
          (_match, body: string) =>
            "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n" +
            "SYNTHSUB_EXECUTED:" +
            body.trim() +
            "\n:::\n:::",
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

export default synthEchoEngine;
