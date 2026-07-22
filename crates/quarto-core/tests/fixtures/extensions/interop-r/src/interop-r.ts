/**
 * interop-r — synthetic Primary+Interop engine fixture (Plan 4b Task A1).
 *
 * Primary-claims "rsynth" (priority 1) and Interop-claims "pysynth" (default
 * priority 0). Interop is presence-gated (`resolution.rs` T3): ownership only
 * extends to "pysynth" when interop-r is already `present` via its "rsynth"
 * Primary claim elsewhere in the same document. This fixture's smoke test
 * (`crates/quarto-core/tests/integration/synth_engines_e2e.rs`) exercises the
 * "rsynth" Primary claim only; the presence-gated multi-cell scenario is
 * Phase 4b-B's T3 tier-resolution assertion.
 *
 * Echo-shaped: no daemon, transforms cells in-process, mirroring
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
import { primary, interop } from "@quarto/api/claims";

// Stash the QuartoAPI reference set during init().
let _quarto: QuartoAPI | undefined;

const interopREngine: ExecutionEngineDiscovery = {
  name: "interop-r",
  defaultExt: ".interopr",
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
    if (language === "rsynth") return primary(1);
    if (language === "pysynth") return interop();
    return false;
  },

  claimsFile(_file: string, _ext: string): boolean {
    return false;
  },

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
    return {
      name: "interop-r",
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
        // Transform {rsynth} and {pysynth} fenced blocks independently; leave
        // everything else untouched.
        let executed = opts.target.markdown.value;
        executed = executed.replace(
          /```\{rsynth\}[\s\S]*?```/g,
          "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n" +
            "**INTEROP_R_RSYNTH_EXECUTED**\n:::\n:::",
        );
        executed = executed.replace(
          /```\{pysynth\}[\s\S]*?```/g,
          "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n" +
            "**INTEROP_R_PYSYNTH_EXECUTED**\n:::\n:::",
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

export default interopREngine;
