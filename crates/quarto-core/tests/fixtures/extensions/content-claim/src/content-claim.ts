/**
 * content-claim — synthetic content-inspecting dynamic `claims_file` fixture
 * (Plan 4b Task A1).
 *
 * `_extension.yml` deliberately OMITS `claims-files:` (no static,
 * unconditional file claim for `.syn` — `claims_files: None` on the Rust
 * side, `crates/quarto-core/src/extension/types.rs`), so file ownership for
 * `.syn` falls back to this module's DYNAMIC `claimsFile`, which inspects the
 * file's content: it claims a `.syn` file when its first line is exactly
 * `# synth-claim`, and declines otherwise. This exercises the
 * dynamic-fallback `claims_file` wire path generically (does NOT use
 * `content_pattern` — that field does not exist yet; Plan 7a future).
 *
 * This fixture is NOT smoke-tested by
 * `crates/quarto-core/tests/integration/synth_engines_e2e.rs` — bound in
 * Phase 4b-B (B11).
 *
 * Echo-shaped otherwise: no daemon, converts a claimed `.syn` file into a
 * `{content-claim}` fenced block and transforms it in-process, mirroring the
 * whole-file-claim half of `../../echo-engine/src/echo-engine.ts`.
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

// Stash the QuartoAPI reference set during init().
let _quarto: QuartoAPI | undefined;

const contentClaimEngine: ExecutionEngineDiscovery = {
  name: "content-claim",
  defaultExt: ".syn",
  defaultYaml: (_kernel?: string) => [],
  defaultContent: (_kernel?: string) => [],
  validExtensions: () => [".syn"],
  canFreeze: false,
  generatesFigures: false,

  init(quarto: QuartoAPI): void {
    _quarto = quarto;
  },

  claimsLanguage(
    _language: string,
    _firstClass?: string,
  ): boolean | number | LanguageClaim | null {
    // This fixture claims files by CONTENT, not by language cell.
    return false;
  },

  claimsFile(file: string, ext: string): boolean {
    if (ext !== ".syn") return false;
    try {
      const text = Deno.readTextFileSync(file);
      const firstLine = text.split(/\r?\n/, 1)[0];
      return firstLine === "# synth-claim";
    } catch {
      return false;
    }
  },

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
    return {
      name: "content-claim",
      canFreeze: false,

      async markdownForFile(file: string) {
        const text = Deno.readTextFileSync(file);
        const basename = file.split(/[\\/]/).pop() ?? file;
        const wrapped =
          "# Content-claimed: " +
          basename +
          "\n\n```{content-claim}\n" +
          text +
          "\n```\n";
        return _quarto!.mappedString.fromString(wrapped, file);
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
          /```\{content-claim\}[\s\S]*?```/g,
          "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n" +
            "**CONTENT_CLAIM_EXECUTED**\n:::\n:::",
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

export default contentClaimEngine;
