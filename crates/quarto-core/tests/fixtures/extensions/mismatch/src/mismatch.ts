/**
 * mismatch — synthetic static-vs-dynamic INCONSISTENT claim fixture
 * (Plan 4b Task A1).
 *
 * `_extension.yml` statically declares `claims.synth: { kind: primary }`
 * (Primary(1)), but this module's dynamic `claimsLanguage("synth")` returns
 * `interop()` (Interop(0)) — a deliberate mismatch.
 *
 * This is intentional and must NOT be "fixed": it exists so Phase 4b-B can
 * assert the static-vs-dynamic hard-error guard in
 * `TsEngine::ensure_loaded` (`crates/quarto-core/src/engine/ts_engine.rs`,
 * the "Static-vs-dynamic validation" block) fires at first execute-time load.
 * The bundle BUILDS fine (valid TypeScript/bundle) — the mismatch is only
 * detected at load, when the harness sends a `ClaimsLanguage` wire probe to
 * the now-loaded module and compares it against the recorded static answer.
 *
 * This fixture is NOT smoke-tested by
 * `crates/quarto-core/tests/integration/synth_engines_e2e.rs` (it is
 * designed to hard-error at load) — bound in Phase 4b-B (B10).
 *
 * Echo-shaped otherwise: no daemon, transforms cells in-process, mirroring
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
import { interop } from "@quarto/api/claims";

// Stash the QuartoAPI reference set during init().
let _quarto: QuartoAPI | undefined;

const mismatchEngine: ExecutionEngineDiscovery = {
  name: "mismatch",
  defaultExt: ".mismatch",
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
    // DELIBERATE mismatch: _extension.yml declares `synth` as `primary`, but
    // this returns `interop()`. Do NOT "fix" this — see file header.
    return language === "synth" ? interop() : false;
  },

  claimsFile(_file: string, _ext: string): boolean {
    return false;
  },

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
    return {
      name: "mismatch",
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
        const input = opts.target.markdown.value;
        const executed = input.replace(
          /```\{synth\}[\s\S]*?```/g,
          "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n" +
            "**MISMATCH_EXECUTED**\n:::\n:::",
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

export default mismatchEngine;
