/**
 * Author-style fixture for Plan 2 Phase B gate (T-Gate-tsc).
 *
 * Simulates a realistic engine-extension author's `ExecutionEngineDiscovery`
 * implementation — the kind a third-party engine author would write — compiled
 * against the frozen `@quarto/types` contract via this package's `tsc --noEmit`.
 *
 * What this fixture proves:
 *   - An author can implement `ExecutionEngineDiscovery` using only the public
 *     `@quarto/types` types and `@quarto/api` constructors.
 *   - `claimsLanguage`'s widened return (which now includes `LanguageClaim`)
 *     is usable naturally with `primary()`, `interop()`, `fallback()`.
 *   - `init(quarto)` can call namespace methods (`text`, `path`, `format`,
 *     `system`) without any casts.
 *   - `launch()` returning an `ExecutionEngineInstance` with all required
 *     members typecheck-compiles cleanly.
 *
 * NOT a runtime test — never run, never imported by vitest. Typecheck-only.
 * Covered by `( cd ts-packages/quarto-engine-host-deno && npx tsc --noEmit )`.
 *
 * See also: src/__type-tests__/b2-conformance.type-test.ts (producer side).
 */

import type {
  ExecutionEngineDiscovery,
  ExecutionEngineInstance,
  LanguageClaim,
  MappedString,
  ExecuteOptions,
  ExecuteResult,
  DependenciesOptions,
  DependenciesResult,
  PostProcessOptions,
  PartitionedMarkdown,
  ExecutionTarget,
  Format,
  QuartoAPI,
  EngineProjectContext,
} from "@quarto/types";

// Import the claim constructors from the package's public surface.
// Engine authors import from "@quarto/api" or "@quarto/api/claims".
import { primary, interop, fallback } from "@quarto/api";

// ── quarto reference stored by init() ────────────────────────────────────────
// Follows the contract: never accessed at module top-level, only inside init()
// or other method bodies.
let _quarto: QuartoAPI | undefined;

// ── Minimal MappedString helper (fixture-internal) ────────────────────────────
// A bare-minimum implementation that satisfies the MappedString interface for
// throwing stubs. Bodies throw — this is a TYPECHECK fixture, never run.
function _makeThrowing(): never {
  throw new Error("engine-fixture: not implemented (typecheck-only)");
}

// ── ExecutionEngineInstance (returned by launch) ───────────────────────────────
function _makeInstance(_context: EngineProjectContext): ExecutionEngineInstance {
  return {
    name: "fixture-engine",
    canFreeze: false,

    async markdownForFile(_file: string): Promise<MappedString> {
      _makeThrowing();
    },

    async target(
      _file: string,
      _quiet?: boolean,
      _markdown?: MappedString,
    ): Promise<ExecutionTarget | undefined> {
      _makeThrowing();
    },

    async partitionedMarkdown(
      _file: string,
      _format?: Format,
    ): Promise<PartitionedMarkdown> {
      _makeThrowing();
    },

    async execute(_options: ExecuteOptions): Promise<ExecuteResult> {
      _makeThrowing();
    },

    async dependencies(_options: DependenciesOptions): Promise<DependenciesResult> {
      _makeThrowing();
    },

    async postprocess(_options: PostProcessOptions): Promise<void> {
      _makeThrowing();
    },
  };
}

// ── The author-style engine implementation ─────────────────────────────────────
const fixtureEngine: ExecutionEngineDiscovery = {
  // ── Static discovery fields ──────────────────────────────────────────────────
  name: "fixture-engine",
  defaultExt: ".fixture.qmd",
  canFreeze: false,
  generatesFigures: false,

  defaultYaml(_kernel?: string): string[] {
    return ["engine: fixture"];
  },

  defaultContent(_kernel?: string): string[] {
    return ["```{fixture}", "# fixture code", "```"];
  },

  validExtensions(): string[] {
    return [".fixture.qmd", ".fq"];
  },

  claimsFile(_file: string, ext: string): boolean {
    return ext === ".fq";
  },

  // ── claimsLanguage — exercises the widened LanguageClaim return ──────────────
  //
  // This is the central type test for the widened return. The function returns:
  //   - primary() for "foo" (LanguageClaim with kind "primary")
  //   - interop() for "bar" (LanguageClaim with kind "interop")
  //   - fallback() for "baz" (LanguageClaim with kind "fallback")
  //   - true  for "fixture" (boolean shorthand — primary with priority 1)
  //   - 2     for "fixture-hi" (number — primary with explicit priority)
  //   - null  for everything else (not claimed)
  //
  // Return type is `boolean | number | LanguageClaim | null` — the full union
  // from the frozen contract in @quarto/types. All five branches are reachable.
  claimsLanguage(
    language: string,
    _firstClass?: string,
  ): boolean | number | LanguageClaim | null {
    if (language === "foo") return primary();
    if (language === "bar") return interop();
    if (language === "baz") return fallback();
    if (language === "fixture") return true;
    if (language === "fixture-hi") return 2;
    return null;
  },

  // ── init — exercises QuartoAPI namespaces from the author's perspective ───────
  //
  // Called by the harness after module load. Stores the API reference and calls
  // a few methods from different namespaces to prove they are accessible and
  // well-typed — author-facing classification contract from the author's side.
  init(quarto: QuartoAPI): void {
    _quarto = quarto;

    // Pure namespace — text.lines: string → string[]
    const _lines: string[] = quarto.text.lines("hello\nworld");
    void _lines;

    // Ambient method — path.resource: (...parts: string[]) => string
    const _resPath: string = quarto.path.resource("engines", "fixture.yaml");
    void _resPath;

    // Pure namespace — format.isHtmlCompatible: (format: Format) => boolean
    // We only have the type here, so we pass a minimal cast-free reference via
    // a typed parameter — this is how an engine author would call it in a real
    // claimsFile / launch body that receives a Format.
    function _checkFormat(f: Format): boolean {
      return quarto.format.isHtmlCompatible(f);
    }
    void _checkFormat;

    // Host namespace — system.onCleanup: (handler: () => void) => void
    // Exercises the synchronous `() => void` param (T-Gate-tsc binding target).
    quarto.system.onCleanup(() => {
      // synchronous cleanup: no return value
    });

    // system.isInteractiveSession / runningInCI — informational, touched for coverage
    const _interactive: boolean = quarto.system.isInteractiveSession();
    const _ci: boolean = quarto.system.runningInCI();
    void _interactive;
    void _ci;

    // jupyter namespace (Plan 3 Task 13) — prove the real namespace is reachable
    // and typed end-to-end. `notebookExtensions` is a real VALUE (string[]), not
    // a stub, so reading it exercises the wired makeJupyter(host) result.
    const _nbExts: string[] = quarto.jupyter.notebookExtensions;
    void _nbExts;
  },

  // ── launch — returns an ExecutionEngineInstance ───────────────────────────────
  launch(context: EngineProjectContext): ExecutionEngineInstance {
    return _makeInstance(context);
  },
};

// ── Export so the module is "used" and tsc won't flag it dead ─────────────────
// The export is the vacuity guard: tsc resolves and type-checks this file
// because it is in the `include` glob and is exported.
export const __engineFixture: ExecutionEngineDiscovery = fixtureEngine;

// Re-export the LanguageClaim type so the import is verified as non-dead
// (tsc would otherwise elide an unused type-only import without error).
export type { LanguageClaim as __LanguageClaimUsed };
