/**
 * owns-cant-run — synthetic engine fixture for Plan 4d-D Step 2b (JOL-4).
 *
 * Statically `Primary`-claims the language `synth` (so q2 assigns it
 * ownership) but handles NO computational language: its capability set is
 * empty. `execute()` self-enforces the engine contract by reading the positive
 * set `options.ownedLanguages` and throwing `owns "<L>" but cannot run it` for
 * any owned language outside that set. The error is driven by
 * `ownedLanguages` alone (never by scanning cells), so the test reddens if the
 * field is not carried through the wire and the host.
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

/** The languages this engine can actually run. Deliberately empty. */
const CAPABILITIES: readonly string[] = [];

const ownsCantRun: ExecutionEngineDiscovery = {
  name: "owns-cant-run",
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
    return language === "synth";
  },

  claimsFile(_file: string, _ext: string): boolean {
    return false;
  },

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
    return {
      name: "owns-cant-run",
      canFreeze: false,

      async markdownForFile(file: string) {
        return _quarto!.mappedString.fromString("", file);
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
        for (const lang of opts.ownedLanguages) {
          if (!CAPABILITIES.includes(lang)) {
            throw new Error(`owns "${lang}" but cannot run it`);
          }
        }
        return { markdown: opts.target.markdown.value, supporting: [], filters: [] };
      },

      async dependencies(_opts: DependenciesOptions): Promise<DependenciesResult> {
        return { includes: {} };
      },

      async postprocess(_opts: PostProcessOptions): Promise<void> {},
    };
  },
};

export default ownsCantRun;
