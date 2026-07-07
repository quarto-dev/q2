/**
 * behave — ONE configurable behavioral engine fixture (Plan 4b, Phase 4b-A / Task A2).
 *
 * Claims the synthetic language "behave" as Primary(1), echo-shaped like
 * `../../echo-engine/src/echo-engine.ts` and `../../alpha/src/alpha.ts`: no
 * daemon, transforms `{behave}` cells in-process, passes through everything
 * else.
 *
 * DELIBERATELY a single engine with multiple sentinel-gated branches — not a
 * family of engines (decision ratified 2026-07-08, see
 * `claude-notes/plans/2026-07-01-plan4b-shadow-engine-features.md`, Phase
 * 4b-A). Each branch is reached by a DIFFERENT fixture document hitting a
 * different sentinel, mirroring echo-engine's `QUARTO_ECHO_CRASH` +
 * `Deno.exit(1)` precedent (`echo-engine.ts:165-180`). Sentinels never
 * collide because Phase 4b-D/F drive each branch with its own document.
 *
 * Task A2 (this file) only binds the DEFAULT branch (see
 * `behave_engine_e2e.rs`'s smoke test). The other branches are bound later:
 *   - `intermediate_files` verb  -> Phase 4b-F
 *   - `QUARTO_PANDOC` / `QUARTO_EXEC` sentinels -> Phase 4b-D (optional stretch)
 *   - `QUARTO_SLOW` sentinel (cancellation)      -> Phase 4b-F (F-cancel/F-relaunch)
 *   - `BEHAVE_CRASH` sentinel                    -> Phase 4b-F (F-crash)
 * Until those phases land, this file is intentionally the ONLY consumer of
 * the default branch — the other branches are exercised by nothing yet,
 * which is expected (see the module comment on `behave_engine_e2e.rs`).
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

// Plan 4b Phase 4b-F (F-relaunch, F3/F4): module-level launch counter.
// `launch()` is called once per `LaunchEngine` verb the harness actually
// executes (i.e. NOT on a `launchedByName` cache hit on the TS side — see
// `@quarto/engine-host-deno/src/host.ts`'s `launchEngine` case). Logging the
// post-increment value to stderr on every real call gives the Rust-side E2E
// tests a relaunch WITNESS that requires no production-code change: stderr
// lines already forward to the `tracing` target `"engine_host"` via
// `ts_process.rs::stderr_loop` (unmodified). F3/F4 assert this counter
// advances between execute-1 and execute-2 — proof a FRESH `launch()` ran
// (transparent relaunch), not just that Rust's local cache was cleared.
let _behaveLaunchCount = 0;

/**
 * Reject `reject` with an Error whose `.name === "AbortError"`. REQUIRED so
 * the Rust host's `ts_process.rs::request` reader hits its clean
 * `{type:'cancelled'}` branch on cooperative cancellation; any other error
 * name becomes an `error` response, not a cancel
 * (`claude-notes/plans/2026-07-01-plan4b-shadow-engine-features.md`, the
 * "Long/cancellable execute" note).
 */
function rejectAbort(reject: (reason?: unknown) => void): void {
  const err = new Error(
    "behave: QUARTO_SLOW execute aborted (Phase 4b-F cancellation test)",
  );
  err.name = "AbortError";
  reject(err);
}

const behaveEngine: ExecutionEngineDiscovery = {
  name: "behave",
  defaultExt: ".behave",
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
    return language === "behave" ? primary(1) : false;
  },

  claimsFile(_file: string, _ext: string): boolean {
    return false;
  },

  launch(_context: EngineProjectContext): ExecutionEngineInstance {
    _behaveLaunchCount += 1;
    console.error(`BEHAVE_LAUNCH_MARKER:${_behaveLaunchCount}`);
    return {
      name: "behave",
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

        // BEHAVE_CRASH: crash mid-execute (mirrors echo-engine's
        // QUARTO_ECHO_CRASH). Bound in Phase 4b-F's F-crash relaunch e2e.
        // Writes an identifiable marker to stderr, THEN crashes via
        // Deno.exit(1) BEFORE returning any ExecuteResult.
        if (input.includes("BEHAVE_CRASH")) {
          console.error(
            "BEHAVE_CRASH_MARKER: intentional crash for Phase 4b-F crash-path e2e",
          );
          Deno.exit(1);
        }

        // QUARTO_SLOW: the long/cancellable path. Bound in Phase 4b-F's
        // F-cancel / F-relaunch tests. Modeled as an event-listener promise
        // (resolve after N ms OR reject on abort) rather than a busy poll.
        // `ExecuteOptions` (@quarto/types) declares no `signal` field; the
        // host splices a live `AbortSignal` onto the options at runtime via
        // `Object.assign` (`@quarto/engine-host-deno/src/host.ts`, "q2
        // extension: attach signal for cooperative cancellation"), so it's
        // read defensively via a cast.
        if (input.includes("QUARTO_SLOW")) {
          const signal = (opts as unknown as { signal?: AbortSignal }).signal;
          await new Promise<void>((resolve, reject) => {
            if (signal?.aborted) return rejectAbort(reject); // may be aborted while still queued
            const t = setTimeout(resolve, 60_000);
            signal?.addEventListener(
              "abort",
              () => {
                clearTimeout(t);
                rejectAbort(reject);
              },
              { once: true },
            );
          });
          return { markdown: input, supporting: [], filters: [] };
        }

        // QUARTO_PANDOC: exercises `_quarto.system.pandoc(...)`. Bound in
        // Phase 4b-D (optional stretch). One representative call.
        if (input.includes("QUARTO_PANDOC")) {
          await _quarto!.system.pandoc(["--version"]);
          return { markdown: input, supporting: [], filters: [] };
        }

        // QUARTO_EXEC: exercises `_quarto.system.execProcess(...)`. Bound in
        // Phase 4b-D (optional stretch). One representative call, each knob set.
        if (input.includes("QUARTO_EXEC")) {
          await _quarto!.system.execProcess(
            { cmd: "echo", args: ["behave-exec"], stdout: "piped", stderr: "piped" },
            undefined,
            "stderr>stdout",
            (s: string) => s,
            true,
            5_000,
          );
          return { markdown: input, supporting: [], filters: [] };
        }

        // default (no sentinel): echo-like passthrough of `{behave}` cells,
        // PLUS the FC-1 sentinel payload. Phase 4b-F's F6 asserts these
        // EXACT values verbatim — the fixture and the assertion share one
        // frozen contract (see the plan doc referenced in the module
        // comment). This is Task A2's bound branch (behave_engine_e2e.rs's
        // smoke test).
        const executed = input.replace(
          /```\{behave\}[\s\S]*?```/g,
          "::: {.cell}\n::: {.cell-output .cell-output-stdout}\n" +
            "**BEHAVE_EXECUTED**\n:::\n:::",
        );
        return {
          markdown: executed,
          supporting: [],
          filters: [],
          // FC-1 sentinel payload (author name -> real ExecuteResult field,
          // all direct matches except the last):
          //   metadata: {behaveFc1: "md"}                  -> metadata
          //   pandoc: {behaveFc1: "pandoc"}                 -> pandoc
          //   resourceFiles: ["behave.res"]                 -> resourceFiles
          //   preserve: {"BEHAVE_KEY": "behave-preserve"}   -> preserve
          //   postProcess: true                             -> postProcess
          //   dependencies: false                           -> engineDependencies
          // The plan's "dependencies: false" has no literal boolean field on
          // ExecuteResult (that shape lives on the INPUT, `ExecuteOptions
          // .dependencies: boolean`). The nearest ExecuteResult field is the
          // optional `engineDependencies?: Record<string, unknown[]>`
          // (wire: `TsExecuteResult.engine_dependencies`,
          // `#[serde(default)] Option<...>`), which the harness populates
          // ONLY for deferred-dependency engines. "false" is represented by
          // simply NOT setting it here (absent on the wire == no deferred
          // dependencies), which is this passthrough branch's natural,
          // already-default state. See the Task A2 report for the full
          // mapping table.
          metadata: { behaveFc1: "md" },
          pandoc: { behaveFc1: "pandoc" },
          resourceFiles: ["behave.res"],
          preserve: { "BEHAVE_KEY": "behave-preserve" },
          postProcess: true,
        };
      },

      async dependencies(
        _opts: DependenciesOptions,
      ): Promise<DependenciesResult> {
        return { includes: {} };
      },

      async postprocess(_opts: PostProcessOptions): Promise<void> {
        // nothing to do
      },

      // `intermediate_files` — implemented as the separate instance verb
      // (NOT an `execute` branch), so Phase 4b-F can round-trip it
      // independently of any sentinel document. Bound in Phase 4b-F.
      intermediateFiles(input: string): string[] | undefined {
        return [`${input}.behave-intermediate`];
      },
    };
  },
};

export default behaveEngine;
