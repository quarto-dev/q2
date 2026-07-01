/**
 * @quarto/engine-host-deno — runHost dispatch loop (Parts 1 and 2 of 3).
 *
 * Implements the stream-injected, non-blocking, multiplexed runHost function
 * plus all discovery/instance handlers (loadEngine, launchEngine, claimsLanguage,
 * claimsFile, markdownForFile, intermediateFiles) with id correlation, state
 * guards, and concurrent-safe load/launch idempotency.
 *
 * Also implements: execute dispatch flow (7-step: rehydrate → target → format →
 * ExecuteOptions → execute() → field-routing → response), the `dependencies` verb
 * (thin pass-through), and return-based htmlDependencies forwarding with rel→abs
 * normalization vs libDir.
 *
 * cancel is STUBBED here — 6c fills it in.
 *
 * IMPORTANT: NO Deno.* APIs anywhere in this file. The Deno entry lives in
 * src/main.ts (excluded from tsconfig via the exclude list).
 */

import type {
  QuartoAPI,
  ExecutionEngineDiscovery,
  ExecutionEngineInstance,
  EngineProjectContext as RichProjectContext,
  ExternalEngine,
  ExecuteOptions,
  ExecuteResult,
  DependenciesOptions,
  ExecutionTarget,
  PandocIncludes,
  HtmlDependency,
  LanguageClaim,
} from "@quarto/types";
import type { PlatformHost } from "@quarto/api/platform";

import type {
  FromEngine,
  HostGlobalConfig,
  LoadEngineResult,
  LaunchEngineResult,
  TsLanguageClaim,
  TsMappedStringWithMap,
  TsExecuteResult,
  TsPandocIncludes,
  EngineProjectContext as WireProjectContext,
  ToEngine,
} from "./types.js";
import { readFrames, writeFrame, type FrameWriter } from "./framing.js";
import { rehydrateMappedString, serializeMappedString } from "./mapped-source.js";
import { applyExecuteDefaults, metadataAsFormat } from "./metadata-as-format.js";
import { buildQuartoAPI } from "./quarto-api.js";
import { loadEngineModule as defaultLoadEngineModule } from "./engine-loader.js";

// ==================== RunHostDeps ====================

/**
 * Dependency-injection seam for runHost.
 * Tests inject a controllable in-memory engine loader; production uses the
 * real disk importer from engine-loader.ts.
 */
export interface RunHostDeps {
  /**
   * Override the engine-module loader.
   * Tests inject an in-memory ExecutionEngineDiscovery with controllable
   * deferreds + call counters. Must NOT be used in production entry (main.ts).
   */
  loadEngineModule?: (enginePath: string) => Promise<ExecutionEngineDiscovery>;
}

// ==================== Internal state records ====================

/** Fully-resolved state for a loaded engine (post-init). */
interface LoadedEngineEntry {
  /** Absolute path the engine was imported from. */
  path: string;
  /** The discovery surface (used for claimsFile, claimsLanguage, launch). */
  discovery: ExecutionEngineDiscovery;
  /** Wire result sent in the `loaded` response. */
  result: LoadEngineResult;
}

/** Fully-resolved state for a launched engine (post-launch). */
interface LaunchedEngineEntry {
  /** The live engine instance. */
  instance: ExecutionEngineInstance;
  /**
   * The RICH Q1 EngineProjectContext built at launch time and passed to launch().
   * Reused in execute() options — Q1 narrowed both ExecuteOptions.project and
   * launch()'s arg to EngineProjectContext, so the launch context is the right object.
   */
  richProject: RichProjectContext;
  /** Stash the thin wire project for transparent re-launch (6c). */
  project: WireProjectContext;
  /** Wire result sent in the `launched` response. */
  result: LaunchEngineResult;
}

// ==================== Helpers ====================

/**
 * Write an `error` response under the given request id.
 * Called from the generic dispatch-catch path and from guard checks.
 */
async function writeError(
  writer: FrameWriter,
  id: number,
  message: string,
  stack?: string | null,
): Promise<void> {
  const msg: FromEngine = { type: "error", message, stack: stack ?? null };
  await writeFrame(writer, { id, msg });
}

/**
 * Rename Q1 PandocIncludes keys to the camelCase wire TsPandocIncludes keys.
 *
 * Q1:   "include-in-header" / "include-before-body" / "include-after-body"
 * Wire: inHeader            / beforeBody             / afterBody
 *
 * Returns undefined if includes is undefined or has no non-empty arrays.
 * Named revert: skip renaming → Q1 kebab-case keys land on the wire → RED.
 */
function renameIncludes(
  includes: PandocIncludes | undefined,
): TsPandocIncludes | undefined {
  if (!includes) return undefined;
  const result: TsPandocIncludes = {};
  const header = includes["include-in-header"];
  const before = includes["include-before-body"];
  const after = includes["include-after-body"];
  if (header?.length) result.inHeader = header;
  if (before?.length) result.beforeBody = before;
  if (after?.length) result.afterBody = after;
  return Object.keys(result).length > 0 ? result : undefined;
}

/**
 * Resolve an htmlDependency file path to absolute vs libDir.
 *
 * If the path is already absolute (starts with "/" or Windows drive), return as-is.
 * Otherwise, join with libDir.
 * Named revert: remove this call → relative paths stay relative → RED.
 */
function resolveHtmlDepPath(libDir: string, filePath: string): string {
  if (filePath.startsWith("/") || /^[A-Za-z]:[\\/]/.test(filePath)) {
    return filePath; // already absolute
  }
  return libDir.endsWith("/") ? libDir + filePath : libDir + "/" + filePath;
}

/**
 * Map a `claimsLanguage` return value to the wire `TsLanguageClaim | null`.
 *
 * Mapping (§3.2 of claude-notes/designs/engine-resolution.md):
 *   false/null/undefined → null
 *   true → { kind: 'primary', priority: 1 }
 *   number n → { kind: 'primary', priority: n }  (negative = low-priority primary, NEVER interop)
 *   LanguageClaim object → wire TsLanguageClaim, filling §3.2 default priorities:
 *     primary → priority ?? 1
 *     interop → priority ?? 0
 *     fallback → priority ?? 0
 *   An explicit `priority` on the object is passed through unchanged (including 0 / negatives).
 */
function mapLanguageClaim(
  raw: boolean | number | LanguageClaim | null | undefined,
): TsLanguageClaim | null {
  if (raw === false || raw === null || raw === undefined) return null;
  if (raw === true) return { kind: "primary", priority: 1 };
  // Object form: LanguageClaim — fill §3.2 defaults (primary→1, interop/fallback→0).
  if (typeof raw === "object") {
    return {
      kind: raw.kind,
      priority: raw.priority ?? (raw.kind === "primary" ? 1 : 0),
    };
  }
  // raw is a number (positive or negative — always 'primary', never interop)
  return { kind: "primary", priority: raw };
}

/**
 * Reconstruct the RICH Q1 EngineProjectContext from the THIN wire context.
 *
 * Wire (thin):  { projectDir?, isSingleFile, config?: Record<string,TsMetadataValue>, outputDir? }
 * Rich (Q1):   { dir, isSingleFile, config?, fileInformationCache, getOutputDirectory, resolveFullMarkdownForFile }
 *
 * Config mapping: pull `engines` array and `output-dir`/`outputDir` from the wire config
 * map if present; leave undefined if absent.
 *
 * resolveFullMarkdownForFile uses the DQ-1 push model: returns `markdown ?? fromString('')`.
 * No engine→host callback in v1.
 */
function reconstructRichProject(
  thin: WireProjectContext,
  quartoAPI: QuartoAPI,
): RichProjectContext {
  let richConfig: RichProjectContext["config"] | undefined;

  if (thin.config) {
    richConfig = {};
    const cfg = thin.config;

    // engines block: pass through as (string | ExternalEngine)[]
    if (Array.isArray(cfg["engines"])) {
      richConfig.engines = cfg["engines"] as (string | ExternalEngine)[];
    }

    // output-dir / outputDir key variants
    const outputDirRaw =
      cfg["output-dir"] !== undefined ? cfg["output-dir"] : cfg["outputDir"];
    if (typeof outputDirRaw === "string") {
      richConfig.project = { outputDir: outputDirRaw };
    }
  }

  const configOutputDir = richConfig?.project?.outputDir;
  const resolvedOutputDir = thin.outputDir ?? configOutputDir ?? "";

  return {
    dir: thin.projectDir ?? "",
    isSingleFile: thin.isSingleFile,
    config: richConfig,
    fileInformationCache: new Map(),
    getOutputDirectory: () => resolvedOutputDir,
    // DQ-1 push model: return the pushed markdown (or an empty MappedString default)
    resolveFullMarkdownForFile: async (
      _engine,
      _file,
      markdown?,
      _force?,
    ) => markdown ?? quartoAPI.mappedString.fromString(""),
  };
}

// ==================== runHost ====================

/**
 * The core dispatch loop. Reads newline-framed JSON Request objects from `reader`,
 * dispatches each to the appropriate handler (fire-and-forget, non-blocking), and
 * writes newline-framed JSON Response objects to `writer`.
 *
 * Contract:
 *   - `init` is consumed immediately (no response, builds QuartoAPI).
 *   - Any message before `init` → `error` "message before Init".
 *   - `shutdown` breaks the loop; outstanding tasks drain before returning.
 *   - EOF also drains and returns.
 *   - Handlers never crash the loop: thrown errors become `error` responses.
 *   - Load/launch idempotency is CONCURRENT-SAFE: the in-flight promise is
 *     registered SYNCHRONOUSLY (before the first await) so K concurrent same-path
 *     loadEngine (or same-name launchEngine) frames import/launch exactly once.
 *
 * NO Deno.* references in this file. The Deno entry (main.ts) is excluded from tsc.
 */
export async function runHost(
  reader: ReadableStream<Uint8Array>,
  writer: FrameWriter,
  host: PlatformHost,
  deps?: RunHostDeps,
): Promise<void> {
  const loaderFn = deps?.loadEngineModule ?? defaultLoadEngineModule;

  // ── Engine registry ───────────────────────────────────────────────────────
  //
  // loadedByPath:   keyed by enginePath. Stores the in-flight-or-resolved load
  //                 promise. Registered SYNCHRONOUSLY before the first await so
  //                 K concurrent loadEngine(samePath) frames share one promise.
  //
  // engineByName:   keyed by discovery.name. Populated when the load promise
  //                 resolves successfully. Used by launch/claims*/markdown handlers
  //                 for name-based lookup.
  //
  // launchedByName: keyed by engine name. Stores the in-flight-or-resolved launch
  //                 promise. Registered SYNCHRONOUSLY before the first await so
  //                 K concurrent launchEngine(sameName) frames share one promise.
  const loadedByPath = new Map<string, Promise<LoadedEngineEntry>>();
  const engineByName = new Map<string, LoadedEngineEntry>();
  const launchedByName = new Map<string, Promise<LaunchedEngineEntry>>();

  // ── 6c: concurrency layer ─────────────────────────────────────────────────
  //
  // perEngineQueue: per-engine promise tail for serializing executes.
  //   Same-engine executes run one-at-a-time; cross-engine executes are concurrent
  //   because each engine has its own independent tail. Tails are pruned when no
  //   further execute is chained (GC). Unbounded depth accepted for v1.
  //
  // inflight: per-request AbortControllers, registered BEFORE the queue admits a
  //   task so a queued (not-yet-executing) request is cancellable immediately.
  //
  // stashedContextByName: per-engine launch context that survives instance poison
  //   (launchedByName deletion). Populated once at launchEngine time; never cleared.
  //   Used by the transparent re-launch path inside the serialized queue task.
  const perEngineQueue = new Map<string, Promise<unknown>>();
  const inflight = new Map<number, AbortController>();
  const stashedContextByName = new Map<
    string,
    { richProject: RichProjectContext; project: WireProjectContext }
  >();

  let quartoAPI: QuartoAPI | undefined;
  // Retained from Init.global for execute/dependencies (resourceDir is ambient, not on wire).
  let globalConfig: HostGlobalConfig | undefined;

  // Track outstanding dispatch tasks so we can drain before returning.
  const inFlight = new Set<Promise<unknown>>();

  // ── Dispatch function ─────────────────────────────────────────────────────
  //
  // Called fire-and-forget from the main loop. Errors are caught externally and
  // converted to `error` responses under the original request id.
  //
  // Note: `quartoAPI` is guaranteed non-null here because the loop guards with
  // `!quartoAPI` before calling dispatch. The `!` assertions below are safe.
  // `msg` is typed as `ToEngine` so TypeScript narrows each case cleanly —
  // no `any` cast needed. `init` and `shutdown` are filtered before dispatch
  // and fall to the `default` branch (runtime guard) if somehow reached.
  async function dispatch(id: number, msg: ToEngine): Promise<void> {
    switch (msg.type) {
      // ── loadEngine ──────────────────────────────────────────────────────────
      case "loadEngine": {
        const path: string = msg.enginePath;

        // CONCURRENT-SAFE IDEMPOTENCY:
        // If the path is already registered (in-flight or resolved), return the
        // same LoadEngineResult without re-importing or re-calling init.
        if (loadedByPath.has(path)) {
          const entry = await loadedByPath.get(path)!;
          await writeFrame(writer, {
            id,
            msg: { type: "loaded", discovery: entry.result },
          });
          return;
        }

        // Register the in-flight promise SYNCHRONOUSLY (before the first await)
        // so concurrent same-path frames all share this exact promise.
        const loadPromise = (async (): Promise<LoadedEngineEntry> => {
          // Step 1: Import the engine module.
          let discovery: ExecutionEngineDiscovery;
          try {
            discovery = await loaderFn(path);
          } catch (err) {
            // Import failed — remove in-flight entry so a later retry can succeed.
            loadedByPath.delete(path);
            throw err;
          }

          // Step 2: Name-collision guard.
          // Same name, different path → configuration error (not retryable).
          const existingByName = engineByName.get(discovery.name);
          if (existingByName !== undefined && existingByName.path !== path) {
            loadedByPath.delete(path);
            throw new Error(
              `engine name reused with different path: ${existingByName.path} vs ${path}`,
            );
          }

          // Step 3: Run init (optional; defensive await tolerates async init).
          if (discovery.init) {
            try {
              await discovery.init(quartoAPI!);
            } catch (err) {
              // init failed — remove in-flight entry so a later correct load can succeed.
              loadedByPath.delete(path);
              throw err;
            }
          }

          // Step 4: Build and cache the result.
          const result: LoadEngineResult = {
            name: discovery.name,
            validExtensions: discovery.validExtensions(),
            generatesFigures: discovery.generatesFigures,
            canFreeze: discovery.canFreeze,
            quartoRequired: discovery.quartoRequired ?? undefined,
          };

          const entry: LoadedEngineEntry = { path, discovery, result };
          // Register by name so launch/claims handlers can look up by name.
          engineByName.set(discovery.name, entry);
          return entry;
        })();

        // CRITICAL: register synchronously before the first await of this handler.
        // Frame 2 arriving while frame 1 is awaiting loadPromise will see this entry
        // and share the same in-flight promise.
        loadedByPath.set(path, loadPromise);

        const entry = await loadPromise;
        await writeFrame(writer, {
          id,
          msg: { type: "loaded", discovery: entry.result },
        });
        return;
      }

      // ── launchEngine ────────────────────────────────────────────────────────
      case "launchEngine": {
        const name: string = msg.engine;
        const wireProject: WireProjectContext = msg.project;

        // Guard: engine must be fully loaded (name registered in engineByName).
        const engineEntry = engineByName.get(name);
        if (engineEntry === undefined) {
          throw new Error(`engine not loaded: ${name}`);
        }

        // CONCURRENT-SAFE IDEMPOTENCY:
        // If the name is already registered (in-flight or resolved launch), return
        // the same LaunchEngineResult without re-running engine.launch.
        if (launchedByName.has(name)) {
          const entry = await launchedByName.get(name)!;

          // Dev warning: if the repeat project identity doesn't match the original,
          // warn but do NOT error (non-fatal mismatch).
          const cached = entry.project;
          if (
            cached.projectDir !== wireProject.projectDir ||
            cached.isSingleFile !== wireProject.isSingleFile
          ) {
            host.log.warning(
              `launchEngine: project identity mismatch for engine "${name}". ` +
                `Cached: projectDir=${cached.projectDir}, isSingleFile=${cached.isSingleFile}. ` +
                `Repeat: projectDir=${wireProject.projectDir}, isSingleFile=${wireProject.isSingleFile}.`,
            );
          }

          await writeFrame(writer, {
            id,
            msg: { type: "launched", instance: entry.result },
          });
          return;
        }

        // Register the in-flight promise SYNCHRONOUSLY (before the first await).
        const launchPromise = (async (): Promise<LaunchedEngineEntry> => {
          const richProject = reconstructRichProject(wireProject, quartoAPI!);

          let instance: ExecutionEngineInstance;
          try {
            // Q1 launch() is SYNC but we await defensively in case an engine uses async.
            instance = await engineEntry.discovery.launch(richProject);
          } catch (err) {
            launchedByName.delete(name);
            throw err;
          }

          const result: LaunchEngineResult = { canFreeze: instance.canFreeze };
          // Stash richProject for execute/dependencies (same context passed to launch).
          return { instance, richProject, project: wireProject, result };
        })();

        // CRITICAL: register synchronously before the first await of this handler.
        launchedByName.set(name, launchPromise);

        const entry = await launchPromise;
        // Stash launch context for transparent re-launch after poison (6c).
        // stashedContextByName survives launchedByName deletion so the next
        // execute can reconstruct the instance without a new launchEngine round-trip.
        stashedContextByName.set(name, {
          richProject: entry.richProject,
          project: entry.project,
        });
        await writeFrame(writer, {
          id,
          msg: { type: "launched", instance: entry.result },
        });
        return;
      }

      // ── claimsLanguage ──────────────────────────────────────────────────────
      case "claimsLanguage": {
        const name: string = msg.engine;
        const engineEntry = engineByName.get(name);
        if (engineEntry === undefined) {
          throw new Error(`engine not loaded: ${name}`);
        }

        // Fix #5: msg.firstClass is string | null | undefined (wire type);
        // claimsLanguage expects string | undefined — use ?? to convert null → undefined.
        const raw = engineEntry.discovery.claimsLanguage(
          msg.language,
          msg.firstClass ?? undefined,
        );
        const result = mapLanguageClaim(raw);

        await writeFrame(writer, {
          id,
          msg: { type: "claimsLanguageResult", result },
        });
        return;
      }

      // ── claimsFile ──────────────────────────────────────────────────────────
      case "claimsFile": {
        const name: string = msg.engine;
        const engineEntry = engineByName.get(name);
        if (engineEntry === undefined) {
          throw new Error(`engine not loaded: ${name}`);
        }

        const result = engineEntry.discovery.claimsFile(
          msg.file,
          msg.ext,
        );

        await writeFrame(writer, {
          id,
          msg: { type: "claimsFileResult", result },
        });
        return;
      }

      // ── markdownForFile ─────────────────────────────────────────────────────
      case "markdownForFile": {
        const name: string = msg.engine;
        // Fix #6: distinguish "not loaded" from "loaded but not launched".
        if (!engineByName.has(name)) {
          throw new Error(`engine not loaded: ${name}`);
        }
        const launchPromise = launchedByName.get(name);
        if (launchPromise === undefined) {
          throw new Error(`engine not launched: ${name}`);
        }

        const launchEntry = await launchPromise;
        const ms = await launchEntry.instance.markdownForFile(msg.file);

        const result: TsMappedStringWithMap = {
          value: ms.value,
          fileName: ms.fileName ?? null,
          sourceMap: serializeMappedString(ms),
        };

        await writeFrame(writer, {
          id,
          msg: { type: "markdownForFileResult", result },
        });
        return;
      }

      // ── intermediateFiles ───────────────────────────────────────────────────
      case "intermediateFiles": {
        const name: string = msg.engine;
        // Fix #6: distinguish "not loaded" from "loaded but not launched".
        if (!engineByName.has(name)) {
          throw new Error(`engine not loaded: ${name}`);
        }
        const launchPromise = launchedByName.get(name);
        if (launchPromise === undefined) {
          throw new Error(`engine not launched: ${name}`);
        }

        const launchEntry = await launchPromise;
        const result =
          launchEntry.instance.intermediateFiles?.(msg.input) ?? null;

        await writeFrame(writer, {
          id,
          msg: { type: "intermediateFilesResult", result },
        });
        return;
      }

      // ── execute ─────────────────────────────────────────────────────────────
      case "execute": {
        const name: string = msg.engine;
        const opts = msg.options;

        // Guard: engine must be loaded (check loaded first for precision).
        if (!engineByName.has(name)) {
          throw new Error(`engine not loaded: ${name}`);
        }
        // Guard: engine must have been launched at some point — either the instance
        // is currently live (launchedByName) or was launched then poisoned
        // (stashedContextByName). A transparent re-launch handles the latter inside
        // the serialized queue task below.
        if (!launchedByName.has(name) && !stashedContextByName.has(name)) {
          throw new Error(`engine not launched: ${name}`);
        }

        if (!globalConfig) {
          throw new Error("execute: Init not received");
        }

        // Register AbortController BEFORE the per-engine queue admits the task.
        // A request still QUEUED behind a sibling must be cancellable — the Rust
        // window ticks during queue-wait, so a Cancel message may arrive while this
        // request is waiting. q2 extension: cooperative cancel via AbortController
        // is not in Q1's protocol.
        const controller = new AbortController();
        inflight.set(id, controller);

        // Per-engine serialization queue: chain this execute onto the previous tail.
        // Same-engine executes run one-at-a-time; cross-engine executes run
        // concurrently (each engine has its own independent queue entry).
        // NOTE: unbounded tail depth is accepted for v1 (no depth cap).
        const prev = perEngineQueue.get(name) ?? Promise.resolve();

        const queuedWork = prev.then(async () => {
          try {
            // ── Step 0: Reconstruct poisoned instance (INSIDE queue = serialized) ──
            //
            // When a previous execute was cancelled, its instance entry is dropped
            // (poisoned). The FIRST queued execute that finds the entry missing
            // re-launches using the stashed context; subsequent serialized executes
            // find the re-set entry and skip this step.
            //
            // CRITICAL: this check-and-reconstruct MUST run inside the serialized
            // continuation (here), NOT in the synchronous dispatch prologue above.
            // Doing it in the prologue would race two adjacent same-engine requests:
            // both would observe the missing entry and both would call launch()
            // (double-launch). T7 binds this invariant with the "exactly once" assertion.
            let launchEntry: LaunchedEngineEntry;
            const currentLaunchPromise = launchedByName.get(name);
            if (currentLaunchPromise === undefined) {
              // Instance was poisoned; transparently reconstruct it.
              const stash = stashedContextByName.get(name)!;
              const engineEntry = engineByName.get(name)!;
              // Q1 launch() is SYNC but we await defensively (same as launchEngine handler).
              const instance = await engineEntry.discovery.launch(stash.richProject);
              const result: LaunchEngineResult = { canFreeze: instance.canFreeze };
              launchEntry = {
                instance,
                richProject: stash.richProject,
                project: stash.project,
                result,
              };
              // Register reconstructed entry so subsequent serialized executes see it.
              launchedByName.set(name, Promise.resolve(launchEntry));
            } else {
              launchEntry = await currentLaunchPromise;
            }

            const { instance, richProject } = launchEntry;

            // Step 1: Rehydrate MappedString from wire input + sourceMap.
            const rdr = {
              readTextFileSync: host.fs.readTextFileSync.bind(host.fs),
              logInfo: host.log.info.bind(host.log),
            };
            const rehydratedMarkdown = rehydrateMappedString(
              opts.input,
              opts.sourceMap,
              rdr,
            );

            // Step 2: Build ExecutionTarget.
            // target() is a REQUIRED method on ExecutionEngineInstance; always call it.
            // Call FRESH per execute — no memoization.
            const engineTarget = await instance.target(
              opts.sourcePath,
              opts.quiet,
              rehydratedMarkdown,
            );
            const target: ExecutionTarget = engineTarget ?? {
              source: opts.sourcePath,
              input: opts.sourcePath,
              markdown: rehydratedMarkdown,
              metadata: opts.format.metadata,
              data: undefined,
            };

            // Step 3: Partition format metadata into Q1 Format shape, then fill
            // absent execute-visibility keys with Q1's base defaults (q2 has no
            // writer-format-defaults merge, so without this every executed cell
            // is dropped by the engine's includeCell/includeOutput gates).
            const format = applyExecuteDefaults(metadataAsFormat(opts.format));

            // Step 4: Assemble ExecuteOptions.
            const executeOptions: ExecuteOptions = {
              target,
              format,
              resourceDir: globalConfig!.resourceDir, // ambient from Init — not on wire
              tempDir: opts.tempDir,
              libDir: opts.libDir,
              // Use the launch context's dir — established at launch time.
              projectDir: richProject.dir,
              cwd: opts.cwd,
              params: opts.params ?? undefined, // OWN field — NEVER from format.metadata
              quiet: opts.quiet,
              handledLanguages: opts.handledLanguages,
              dependencies: opts.dependencies,
              project: richProject,
              previewServer: false,
            };

            // Step 5: Execute.
            // q2 extension: attach signal for cooperative cancellation.
            // Q1's ExecuteOptions has no `signal` field — Q1 engines ignore unknown
            // extra properties; q2-native engines read options.signal. Cast via
            // Object.assign + unknown to satisfy the Q1 type without widening it.
            const executeOptionsWithSignal = Object.assign(
              {},
              executeOptions,
              { signal: controller.signal },
            ) as unknown as ExecuteOptions;
            const result: ExecuteResult = await instance.execute(executeOptionsWithSignal);

            // Step 6: Route every ExecuteResult field to the wire TsExecuteResult.
            const tsIncludes = renameIncludes(result.includes);

            // htmlDependencies: q2-native return field (not in Q1 ExecuteResult).
            // Named revert: drop this read → htmlDependencies empty → RED.
            const rawHtmlDeps = (result as unknown as Record<string, unknown>)
              .htmlDependencies as HtmlDependency[] | undefined;
            const htmlDependencies: HtmlDependency[] = (rawHtmlDeps ?? []).map(
              (dep) => ({
                name: dep.name,
                // Named revert: drop resolveHtmlDepPath → paths stay relative → RED.
                stylesheets: (dep.stylesheets ?? []).map((p) =>
                  resolveHtmlDepPath(opts.libDir, p),
                ),
                scripts: (dep.scripts ?? []).map((p) =>
                  resolveHtmlDepPath(opts.libDir, p),
                ),
              }),
            );

            const tsResult: TsExecuteResult = {
              markdown: result.markdown,
              // Named revert: drop `supporting` forward → its assertion RED.
              supporting: result.supporting ?? [],
              filters: result.filters ?? [],
              htmlDependencies,
              resourceFiles: result.resourceFiles ?? [],
              preserve: result.preserve ?? {},
              postProcess: result.postProcess ?? false,
              ...(tsIncludes !== undefined ? { includes: tsIncludes } : {}),
              ...(result.metadata !== undefined
                ? { metadata: result.metadata as TsExecuteResult["metadata"] }
                : {}),
              ...(result.pandoc !== undefined
                ? { pandoc: result.pandoc as TsExecuteResult["pandoc"] }
                : {}),
              // Forward engineDependencies verbatim when present.
              ...(result.engineDependencies !== undefined
                ? {
                    engineDependencies:
                      result.engineDependencies as TsExecuteResult["engineDependencies"],
                  }
                : {}),
              // NOTE: result.engine is DROPPED — known from routing; not on the wire.
            };

            // Step 7: Respond.
            await writeFrame(writer, {
              id,
              msg: { type: "executeResult", result: tsResult },
            });
          } catch (err) {
            if (err instanceof Error && err.name === "AbortError") {
              // Cooperative cancel: the engine honored the abort signal (or the
              // request was aborted before its execute() even started, i.e.
              // signal.aborted was true on entry).
              //
              // Write Cancelled under the execute's id (= the Rust pending slot).
              // A late Cancelled (after Rust already freed the slot on timeout) is
              // dropped by the Rust reader as an unknown id — by design.
              await writeFrame(writer, { id, msg: { type: "cancelled" } });
              //
              // Poison: drop the instance entry. The daemon's state is ambiguous after
              // an interrupted execute — aborting the JS-side AbortSignal never proves
              // the daemon went idle. The next execute for this engine will
              // transparently reconstruct the instance (Step 0 above).
              launchedByName.delete(name);
              // stashedContextByName retains richProject/project for reconstruction.
            } else {
              // Non-abort error: re-throw so the outer dispatch .catch writes an
              // `error` response under the same id.
              throw err;
            }
          } finally {
            // Always remove the controller (success, cancel, or non-abort error).
            inflight.delete(id);
          }
        });

        // Store a non-rejecting tail as the queue entry for the next execute to chain onto.
        // If queuedWork rejects (non-abort error), the catch here swallows it so the
        // queue chain doesn't break for subsequent requests. The error propagates via
        // the outer dispatch .catch (below, via `await queuedWork`).
        const queueTail = queuedWork.catch(() => {});
        // Prune settled chains: when this tail resolves and nothing else was enqueued
        // after it, delete the entry (avoid retaining it for the process lifetime).
        void queueTail.finally(() => {
          if (perEngineQueue.get(name) === queueTail) {
            perEngineQueue.delete(name);
          }
        });
        perEngineQueue.set(name, queueTail);

        // Await the actual work. This keeps the dispatch promise pending (and thus the
        // inFlight task tracked) until the execute fully completes — required for the
        // drain-before-exit to cover queued executes via the inFlight set.
        await queuedWork;
        return;
      }

      // ── dependencies (thin pass-through — 6b implements) ────────────────────
      case "dependencies": {
        const name: string = msg.engine;
        const opts = msg.options;

        // Guard: engine must be launched.
        if (!engineByName.has(name)) {
          throw new Error(`engine not loaded: ${name}`);
        }
        const launchPromise = launchedByName.get(name);
        if (launchPromise === undefined) {
          throw new Error(`engine not launched: ${name}`);
        }

        const launchEntry = await launchPromise;
        const { instance } = launchEntry;

        if (!globalConfig) {
          throw new Error("dependencies: Init not received");
        }

        // Rebuild ExecutionTarget from wire fields (same pattern as execute step 2).
        const reader = {
          readTextFileSync: host.fs.readTextFileSync.bind(host.fs),
          logInfo: host.log.info.bind(host.log),
        };
        const rehydratedMarkdown = rehydrateMappedString(
          opts.input,
          opts.sourceMap,
          reader,
        );

        let target: ExecutionTarget;
        if (instance.target) {
          const engineTarget = await instance.target(
            opts.sourcePath,
            opts.quiet,
            rehydratedMarkdown,
          );
          target = engineTarget ?? {
            source: opts.sourcePath,
            input: opts.sourcePath,
            markdown: rehydratedMarkdown,
            metadata: opts.format.metadata,
            data: undefined,
          };
        } else {
          target = {
            source: opts.sourcePath,
            input: opts.sourcePath,
            markdown: rehydratedMarkdown,
            metadata: opts.format.metadata,
            data: undefined,
          };
        }

        const format = applyExecuteDefaults(metadataAsFormat(opts.format));

        const depOptions: DependenciesOptions = {
          target,
          format,
          output: opts.output,
          resourceDir: globalConfig.resourceDir, // ambient from Init — not on wire
          tempDir: opts.tempDir,
          projectDir: opts.projectDir ?? undefined,
          libDir: opts.libDir ?? undefined,
          // TsMetadataValue[] is assignable to Array<unknown>
          dependencies: opts.dependencies,
          quiet: opts.quiet,
        };

        // Thin pass-through: the harness does NOT drive the round-trip.
        // q2's orchestrator decides when and how to resolve engineDependencies.
        const dr = await instance.dependencies(depOptions);

        // Key-rename includes (same pattern as execute).
        const tsIncludes: TsPandocIncludes = renameIncludes(dr.includes) ?? {};

        await writeFrame(writer, {
          id,
          msg: { type: "dependenciesResult", includes: tsIncludes },
        });
        return;
      }

      // ── cancel (cooperative, fire-and-forget) ────────────────────────────────
      case "cancel": {
        // Abort the in-flight execute identified by msg.target.
        // The cancel frame itself gets NO response.
        //
        // Only the specified target's controller is aborted — sibling in-flight
        // requests on the same or other engines are unaffected.
        //
        // If the target is still queued (not yet executing), the AbortController
        // was registered before the queue admitted it, so abort() fires the signal
        // immediately even while the task waits behind a predecessor.
        //
        // The execute handler's AbortError path writes { type: 'cancelled' } under
        // the target's id. A Cancelled that arrives after Rust already freed the
        // slot (timeout) is dropped as an unknown id — by design.
        inflight.get(msg.target)?.abort();
        return;
      }

      // ── init / shutdown / unknown ────────────────────────────────────────────
      // `init` and `shutdown` are handled in the main loop before dispatch is
      // called; reaching here is a programming error (or unknown future variant).
      default: {
        throw new Error(`unknown message type: ${msg.type}`);
      }
    }
  }

  // ── Main loop ─────────────────────────────────────────────────────────────
  for await (const frame of readFrames(reader)) {
    const { id, msg } = frame;

    // ── Init: consumed synchronously, NO response, builds QuartoAPI ──────────
    if (msg.type === "init") {
      globalConfig = msg.global;
      quartoAPI = buildQuartoAPI(msg.global, host);
      continue; // no response for init
    }

    // ── Guard: any non-init message before Init ───────────────────────────────
    if (!quartoAPI) {
      await writeError(writer, id, "message before Init");
      continue;
    }

    // ── Shutdown: break the loop (drain below) ───────────────────────────────
    if (msg.type === "shutdown") {
      break;
    }

    // ── Fire-and-forget dispatch (NON-BLOCKING) ───────────────────────────────
    // We do NOT await the handler here — this is what enables multiplexing.
    // Errors are caught and written as `error` responses under the same id.
    const task = dispatch(id, msg).catch((err: unknown) => {
      const e = err as { message?: string; stack?: string } | null | undefined;
      return writeError(
        writer,
        id,
        String(e?.message ?? err),
        e?.stack ?? null,
      );
    });
    inFlight.add(task);
    task.finally(() => inFlight.delete(task));
  }

  // ── Drain: wait for all outstanding tasks before returning ────────────────
  // This ensures slow handlers (e.g. a long execute) still flush their response
  // even when shutdown/EOF arrives first.
  await Promise.allSettled([...inFlight]);
}
