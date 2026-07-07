/**
 * Tests for src/host.ts — runHost dispatch loop skeleton, discovery handlers,
 * and load/launch idempotency.
 *
 * TDD: tests written before the implementation. Named-revert markers document
 * which assertion goes RED when the corresponding revert is applied.
 *
 * Scope: vitest/Node tier only. No Deno.* APIs anywhere in this file.
 */

import { describe, it, expect, afterEach } from "vitest";
import { runHost, type RunHostDeps } from "./host.js";
import type { Request, Response } from "./types.js";
import type { FrameWriter } from "./framing.js";
import type {
  ExecutionEngineDiscovery,
  ExecutionEngineInstance,
  ExecuteResult,
  DependenciesResult,
  EngineProjectContext as RichProjectContext,
  QuartoAPI,
  LanguageClaim,
} from "@quarto/types";
import type { PlatformHost } from "@quarto/api/platform";
import { interop, fallback } from "@quarto/api";
import type { HtmlDependency, TsExecuteResult, TsPandocIncludes } from "./types.js";

// ===========================================================================
// Test helpers
// ===========================================================================

// ---------------------------------------------------------------------------
// In-memory framed-duplex channel
// ---------------------------------------------------------------------------

interface Duplex {
  reader: ReadableStream<Uint8Array>;
  push(req: Request): void;
  end(): void;
  writer: FrameWriter;
  responses: Response[];
}

function makeDuplex(): Duplex {
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  const reader = new ReadableStream<Uint8Array>({
    start(c) {
      controller = c;
    },
  });

  const enc = new TextEncoder();
  const dec = new TextDecoder();
  const responses: Response[] = [];
  let buf = "";

  const writer: FrameWriter = {
    write(bytes: Uint8Array): Promise<number> {
      // Accumulate decoded bytes; extract complete newline-terminated JSON frames.
      buf += dec.decode(bytes);
      let nl: number;
      while ((nl = buf.indexOf("\n")) !== -1) {
        const line = buf.slice(0, nl);
        buf = buf.slice(nl + 1);
        if (line.length > 0) {
          responses.push(JSON.parse(line) as Response);
        }
      }
      return Promise.resolve(bytes.length);
    },
  };

  return {
    reader,
    push(req: Request) {
      controller.enqueue(enc.encode(JSON.stringify(req) + "\n"));
    },
    end() {
      controller.close();
    },
    writer,
    responses,
  };
}

// ---------------------------------------------------------------------------
// Minimal fake PlatformHost
// ---------------------------------------------------------------------------

function makeFakeHost(): PlatformHost & { warnings: string[] } {
  const warnings: string[] = [];
  return {
    warnings,
    fs: {
      readTextFileSync: (_path: string) => "",
      writeFileSync: (_path: string, _content: string | Uint8Array) => {},
      exists: (_path: string) => false,
      ensureDir: (_path: string) => {},
      makeTempDir: (_opts?: { prefix?: string; dir?: string }) =>
        "/tmp/fake",
      makeTempFile: (_opts?: {
        prefix?: string;
        suffix?: string;
        dir?: string;
      }) => "/tmp/fake-file",
      remove: (_path: string, _opts?: { recursive?: boolean }) => {},
      walk: (
        _root: string,
        _opts?: { maxDepth?: number; includeDirs?: boolean },
      ) => [],
    },
    process: {
      exec: async () => ({ code: 0, success: true, stdout: "", stderr: "" }),
      onExit: (_handler: () => void) => {},
      exit: (_code: number): never => {
        throw new Error("exit called");
      },
    },
    env: { get: (_key: string) => undefined },
    log: {
      info: (_msg: string) => {},
      warning: (msg: string) => {
        warnings.push(msg);
      },
      error: (_msg: string) => {},
    },
    cwd: () => "/fake/cwd",
    realPath: (p: string) => p,
    isInteractive: false,
    isCI: false,
  };
}

// ---------------------------------------------------------------------------
// Standard init request fixture
// ---------------------------------------------------------------------------

const INIT_ID = 0;
const INIT_REQUEST: Request = {
  id: INIT_ID,
  msg: {
    type: "init",
    global: {
      resourceDir: "/res",
      runtimeDir: "/run",
      dataDir: "/data",
      pandocPath: null,
      isInteractiveSession: false,
      runningInCi: false,
      quartoVersion: "99.9.9",
    },
  },
};

// ---------------------------------------------------------------------------
// Deferred helper (controllable async resolution)
// ---------------------------------------------------------------------------

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(err: unknown): void;
}

function makeDeferred<T>(): Deferred<T> {
  let res!: (v: T) => void;
  let rej!: (e: unknown) => void;
  const promise = new Promise<T>((r, j) => {
    res = r;
    rej = j;
  });
  return { promise, resolve: res, reject: rej };
}

// ---------------------------------------------------------------------------
// Controllable fake ExecutionEngineDiscovery factory
// ---------------------------------------------------------------------------

interface FakeEngineSetup {
  /** The discovery object to return from the fake loader. */
  discovery: ExecutionEngineDiscovery;
  /** The instance object returned by discovery.launch(). */
  instance: ExecutionEngineInstance;
  /** Call counters (incremented inside the respective methods). */
  counters: { loader: number; init: number; launch: number };
  /** Deferred that controls when the loader promise resolves. */
  loaderDeferred: Deferred<void>;
  /** Deferred that controls when the launch promise resolves (used when blockLaunch=true). */
  launchDeferred: Deferred<void>;
  /** QuartoAPI captured by init (undefined until init runs). */
  capturedAPI: { value: QuartoAPI | undefined };
  /** Stash that init can write to for T-A5 verification. */
  initStash: { linesResult: string[] | undefined };
  /** How many times instance.dependencies() was called (spy counter). */
  dependenciesCallCount: { value: number };
  /**
   * Returns a loader function suitable for deps.loadEngineModule.
   * The loader blocks until loaderDeferred is resolved (if blockLoader is true).
   */
  makeLoader(opts?: { blockLoader?: boolean }): (path: string) => Promise<ExecutionEngineDiscovery>;
}

function makeFakeEngine(
  name: string,
  opts?: {
    noInit?: boolean;
    initThrows?: boolean;
    initStashLines?: boolean; // call quarto.text.lines("a\nb") in init and stash
    markdownValue?: string;
    /**
     * When true, discovery.launch() becomes async and blocks on launchDeferred.
     * Call setup.launchDeferred.resolve() to unblock it.
     * This creates a genuine in-flight window for concurrent-launch tests.
     */
    blockLaunch?: boolean;
    /**
     * When provided, instance.execute() returns this result (with htmlDependencies as extra field).
     * When absent, execute() throws "execute not configured in this fake engine".
     */
    executeResult?: Partial<ExecuteResult> & { htmlDependencies?: HtmlDependency[] };
    /**
     * When provided, instance.dependencies() returns this result (in addition to
     * incrementing the dependenciesCallCount counter).
     */
    dependenciesResult?: DependenciesResult;
  },
): FakeEngineSetup {
  const counters = { loader: 0, init: 0, launch: 0 };
  const capturedAPI: { value: QuartoAPI | undefined } = { value: undefined };
  const initStash: { linesResult: string[] | undefined } = {
    linesResult: undefined,
  };
  const loaderDeferred = makeDeferred<void>();
  const launchDeferred = makeDeferred<void>();
  const dependenciesCallCount = { value: 0 };

  const instance: ExecutionEngineInstance = {
    name,
    canFreeze: true,
    markdownForFile: async (file: string) => ({
      value: opts?.markdownValue ?? `md:${file}`,
      fileName: undefined,
      map: () => undefined,
    }),
    target: async () => undefined,
    partitionedMarkdown: async () => {
      throw new Error("not used in these tests");
    },
    execute: async () => {
      if (opts?.executeResult !== undefined) {
        // Merge with required defaults so the return type is valid.
        const base: ExecuteResult = {
          markdown: "",
          supporting: [],
          filters: [],
        };
        return { ...base, ...opts.executeResult } as ExecuteResult;
      }
      throw new Error("execute not configured in this fake engine");
    },
    dependencies: async () => {
      dependenciesCallCount.value++;
      if (opts?.dependenciesResult !== undefined) {
        return opts.dependenciesResult;
      }
      return { includes: {} };
    },
    postprocess: async () => {},
    intermediateFiles: (input: string) => [`${input}.out`],
  };

  const initFn = opts?.noInit
    ? undefined
    : async (quarto: QuartoAPI) => {
        counters.init++;
        capturedAPI.value = quarto;
        if (opts?.initThrows) throw new Error("init failed intentionally");
        if (opts?.initStashLines) {
          initStash.linesResult = quarto.text.lines("a\nb");
        }
      };

  const discovery: ExecutionEngineDiscovery = {
    name,
    defaultExt: ".qmd",
    defaultYaml: () => [],
    defaultContent: () => [],
    validExtensions: () => [".qmd", ".md"],
    claimsFile: (_file: string, ext: string) => ext === ".qmd",
    claimsLanguage: (language: string) =>
      language === "python" ? true : (language === "r" ? 2 : false),
    canFreeze: false,
    generatesFigures: true,
    quartoRequired: ">= 1.0.0",
    ...(initFn ? { init: initFn } : {}),
    // When blockLaunch is set, launch() is async and gates on launchDeferred,
    // creating a real in-flight window that concurrent callers share.
    // The async form is cast because the Q1 type declares sync return, but
    // dispatch() uses `await` defensively and accepts a thenable correctly.
    launch: opts?.blockLaunch
      ? (async (_ctx: unknown) => {
          counters.launch++;
          await launchDeferred.promise;
          return instance;
        }) as unknown as ExecutionEngineDiscovery["launch"]
      : (_ctx) => {
          counters.launch++;
          return instance;
        },
  };

  return {
    discovery,
    instance,
    counters,
    loaderDeferred,
    launchDeferred,
    capturedAPI,
    initStash,
    dependenciesCallCount,
    makeLoader(loaderOpts?: { blockLoader?: boolean }) {
      return async (_path: string): Promise<ExecutionEngineDiscovery> => {
        counters.loader++;
        if (loaderOpts?.blockLoader) {
          await loaderDeferred.promise;
        }
        return discovery;
      };
    },
  };
}

// ---------------------------------------------------------------------------
// Convenience: run runHost with init already sent, then more frames, then shutdown
// ---------------------------------------------------------------------------

async function runWithFrames(
  frames: Request[],
  deps?: RunHostDeps,
): Promise<{ responses: Response[]; warnings: string[] }> {
  const duplex = makeDuplex();
  const host = makeFakeHost();

  duplex.push(INIT_REQUEST);
  for (const frame of frames) {
    duplex.push(frame);
  }
  duplex.push({ id: 9999, msg: { type: "shutdown" } });
  duplex.end();

  await runHost(duplex.reader, duplex.writer, host, deps);
  return { responses: duplex.responses, warnings: host.warnings };
}

// ===========================================================================
// T3 — Handler dispatch and id correlation
// ===========================================================================

describe("T3 — handler dispatch and id correlation", () => {
  it("dispatches loadEngine → loaded response with matching id", async () => {
    const engine = makeFakeEngine("myEngine");
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [{ id: 10, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } }],
      { loadEngineModule: loader },
    );

    // Named revert: remove the 'loadEngine' case arm → this assertion RED
    const loaded = responses.find((r) => r.msg.type === "loaded");
    expect(loaded).toBeDefined();

    // Named revert: hard-code response id to 0 → this assertion RED
    expect(loaded?.id).toBe(10);
  });

  it("dispatches launchEngine → launched response with matching id", async () => {
    const engine = makeFakeEngine("myEngine");
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        {
          id: 2,
          msg: {
            type: "launchEngine",
            engine: "myEngine",
            project: { isSingleFile: true },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const launched = responses.find((r) => r.msg.type === "launched");
    expect(launched).toBeDefined();
    expect(launched?.id).toBe(2);
  });

  it("dispatches claimsLanguage → claimsLanguageResult with matching id", async () => {
    const engine = makeFakeEngine("myEngine");
    const loader = engine.makeLoader();

    // Named revert: remove the 'claimsLanguage' case arm → this assertion RED
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        {
          id: 20,
          msg: {
            type: "claimsLanguage",
            engine: "myEngine",
            language: "python",
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(result).toBeDefined();
    expect(result?.id).toBe(20);
    // python → true → { kind: 'primary', priority: 1 }
    expect(result?.msg).toEqual({
      type: "claimsLanguageResult",
      result: { kind: "primary", priority: 1 },
    });
  });

  it("maps claimsLanguage number result to primary claim", async () => {
    const engine = makeFakeEngine("myEngine");
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        {
          id: 21,
          msg: {
            type: "claimsLanguage",
            engine: "myEngine",
            language: "r", // returns 2
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(result?.msg).toEqual({
      type: "claimsLanguageResult",
      result: { kind: "primary", priority: 2 },
    });
  });

  it("maps claimsLanguage false to null", async () => {
    const engine = makeFakeEngine("myEngine");
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        {
          id: 22,
          msg: {
            type: "claimsLanguage",
            engine: "myEngine",
            language: "julia", // returns false
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(result?.msg).toEqual({ type: "claimsLanguageResult", result: null });
  });

  // ── object-form normalization (mapLanguageClaim object case) ────────────────

  it("maps LanguageClaim {kind:'primary'} → {kind:'primary', priority:1} (default)", async () => {
    const engine = makeFakeEngine("myEngine");
    const customDiscovery: ExecutionEngineDiscovery = {
      ...engine.discovery,
      claimsLanguage: (_language: string): LanguageClaim => ({ kind: "primary" }),
    };
    const loader = async (_path: string) => customDiscovery;

    // Named revert → RED: remove the object case from mapLanguageClaim → priority
    // becomes the raw object value (not 1), so this assertion fails.
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        { id: 23, msg: { type: "claimsLanguage", engine: "myEngine", language: "julia" } },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(result?.msg).toEqual({
      type: "claimsLanguageResult",
      result: { kind: "primary", priority: 1 },
    });
  });

  it("maps LanguageClaim {kind:'interop'} → {kind:'interop', priority:0} (default)", async () => {
    const engine = makeFakeEngine("myEngine");
    const customDiscovery: ExecutionEngineDiscovery = {
      ...engine.discovery,
      claimsLanguage: (_language: string): LanguageClaim => ({ kind: "interop" }),
    };
    const loader = async (_path: string) => customDiscovery;

    // Named revert → RED: remove the object case → wrong kind and priority.
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        { id: 24, msg: { type: "claimsLanguage", engine: "myEngine", language: "python" } },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(result?.msg).toEqual({
      type: "claimsLanguageResult",
      result: { kind: "interop", priority: 0 },
    });
  });

  it("maps LanguageClaim {kind:'interop', priority:3} → {kind:'interop', priority:3} (explicit)", async () => {
    const engine = makeFakeEngine("myEngine");
    const customDiscovery: ExecutionEngineDiscovery = {
      ...engine.discovery,
      claimsLanguage: (_language: string): LanguageClaim => ({ kind: "interop", priority: 3 }),
    };
    const loader = async (_path: string) => customDiscovery;

    // Named revert → RED: remove the object case → wrong kind/priority shape.
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        { id: 25, msg: { type: "claimsLanguage", engine: "myEngine", language: "python" } },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(result?.msg).toEqual({
      type: "claimsLanguageResult",
      result: { kind: "interop", priority: 3 },
    });
  });

  it("maps LanguageClaim {kind:'fallback'} → {kind:'fallback', priority:0} (default)", async () => {
    const engine = makeFakeEngine("myEngine");
    const customDiscovery: ExecutionEngineDiscovery = {
      ...engine.discovery,
      claimsLanguage: (_language: string): LanguageClaim => ({ kind: "fallback" }),
    };
    const loader = async (_path: string) => customDiscovery;

    // Named revert → RED: remove the object case → wrong kind/priority shape.
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        { id: 26, msg: { type: "claimsLanguage", engine: "myEngine", language: "python" } },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(result?.msg).toEqual({
      type: "claimsLanguageResult",
      result: { kind: "fallback", priority: 0 },
    });
  });

  it("maps LanguageClaim {kind:'primary', priority:0} → {kind:'primary', priority:0} (explicit 0 preserved)", async () => {
    const engine = makeFakeEngine("myEngine");
    const customDiscovery: ExecutionEngineDiscovery = {
      ...engine.discovery,
      claimsLanguage: (_language: string): LanguageClaim => ({ kind: "primary", priority: 0 }),
    };
    const loader = async (_path: string) => customDiscovery;

    // Revert → RED: change `?? (kind==='primary'?1:0)` to `|| ...` → explicit 0 becomes 1.
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        { id: 27, msg: { type: "claimsLanguage", engine: "myEngine", language: "python" } },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(result?.msg).toEqual({
      type: "claimsLanguageResult",
      result: { kind: "primary", priority: 0 },
    });
  });

  // ── author-constructor binding (Plan 4b-D2) ──────────────────────────────
  //
  // The rows above exercise mapLanguageClaim's object case with hand-written
  // `{kind: "interop"|"fallback"}` literals. These two tests instead call the
  // ACTUAL author-facing constructors from `@quarto/api` (`interop()`/
  // `fallback()`) — the same ones the A1 synthetic fixtures use
  // (`interop-r.ts`'s `claimsLanguage("pysynth") => interop()`,
  // `fallback-univ.ts`'s `claimsLanguage(_) => fallback(5)`), binding
  // mapLanguageClaim's revert seam (host.ts:~164-178) to the real author SDK
  // surface end-to-end: author LanguageClaim constructor → wire TsLanguageClaim
  // variant (per claude-notes/designs/engine-resolution.md §3.2). Do not
  // conflate this with the (unrelated) TsLanguageClaim type — these
  // constructors return @quarto/types' `LanguageClaim`, which mapLanguageClaim
  // normalizes into the wire `TsLanguageClaim` asserted below.

  it("author interop() constructor normalizes to wire {kind:'interop', priority:0} (D2)", async () => {
    const engine = makeFakeEngine("myEngine");
    const customDiscovery: ExecutionEngineDiscovery = {
      ...engine.discovery,
      claimsLanguage: (_language: string): LanguageClaim => interop(),
    };
    const loader = async (_path: string) => customDiscovery;

    // Named revert → RED: mapLanguageClaim's kind map (host.ts:~164-178)
    // mapped to the wrong wire variant (e.g. hard-coded 'primary') → wrong
    // `kind` on the wire despite the author calling interop().
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        { id: 40, msg: { type: "claimsLanguage", engine: "myEngine", language: "pysynth" } },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(result?.msg).toEqual({
      type: "claimsLanguageResult",
      result: { kind: "interop", priority: 0 },
    });
  });

  it("author fallback() constructor normalizes to wire {kind:'fallback', priority:0} (D2)", async () => {
    const engine = makeFakeEngine("myEngine");
    const customDiscovery: ExecutionEngineDiscovery = {
      ...engine.discovery,
      claimsLanguage: (_language: string): LanguageClaim => fallback(),
    };
    const loader = async (_path: string) => customDiscovery;

    // Named revert → RED: mapLanguageClaim's kind map (host.ts:~164-178)
    // mapped to the wrong wire variant → wrong `kind` on the wire despite the
    // author calling fallback().
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        { id: 41, msg: { type: "claimsLanguage", engine: "myEngine", language: "anylang" } },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(result?.msg).toEqual({
      type: "claimsLanguageResult",
      result: { kind: "fallback", priority: 0 },
    });
  });

  it("dispatches claimsFile → claimsFileResult with matching id", async () => {
    const engine = makeFakeEngine("myEngine");
    const loader = engine.makeLoader();

    // Named revert: remove the 'claimsFile' case arm → this assertion RED
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        {
          id: 30,
          msg: {
            type: "claimsFile",
            engine: "myEngine",
            file: "doc.qmd",
            ext: ".qmd",
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "claimsFileResult");
    expect(result).toBeDefined();
    expect(result?.id).toBe(30);
    expect(result?.msg).toEqual({ type: "claimsFileResult", result: true });
  });

  it("dispatches markdownForFile → markdownForFileResult with matching id", async () => {
    const engine = makeFakeEngine("myEngine", { markdownValue: "# Hello" });
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        {
          id: 2,
          msg: {
            type: "launchEngine",
            engine: "myEngine",
            project: { isSingleFile: true },
          },
        },
        {
          id: 40,
          msg: {
            type: "markdownForFile",
            engine: "myEngine",
            file: "doc.qmd",
          },
        },
      ],
      { loadEngineModule: loader },
    );

    // Named revert: remove the 'markdownForFile' case arm → this assertion RED
    const result = responses.find((r) => r.msg.type === "markdownForFileResult");
    expect(result).toBeDefined();
    expect(result?.id).toBe(40);
    expect((result?.msg as { type: string; result: { value: string } }).result.value).toBe("# Hello");
  });

  it("dispatches intermediateFiles → intermediateFilesResult with matching id", async () => {
    const engine = makeFakeEngine("myEngine");
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        {
          id: 2,
          msg: {
            type: "launchEngine",
            engine: "myEngine",
            project: { isSingleFile: true },
          },
        },
        {
          id: 50,
          msg: {
            type: "intermediateFiles",
            engine: "myEngine",
            input: "doc.qmd",
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find(
      (r) => r.msg.type === "intermediateFilesResult",
    );
    expect(result).toBeDefined();
    expect(result?.id).toBe(50);
    expect(
      (result?.msg as { type: string; result: string[] }).result,
    ).toEqual(["doc.qmd.out"]);
  });

  it("returns error for claimsLanguage before loadEngine (negative path — state guard)", async () => {
    const { responses } = await runWithFrames(
      [
        {
          id: 77,
          msg: {
            type: "claimsLanguage",
            engine: "unknown",
            language: "python",
          },
        },
      ],
      { loadEngineModule: async () => { throw new Error("should not call"); } },
    );

    // Named revert: remove the 'engine not loaded' guard in claimsLanguage → this RED
    const err = responses.find((r) => r.msg.type === "error");
    expect(err).toBeDefined();
    expect(err?.id).toBe(77);
    expect((err?.msg as { type: string; message: string }).message).toMatch(
      /engine not loaded/,
    );
  });

  it("returns error for markdownForFile before launchEngine (state guard)", async () => {
    const engine = makeFakeEngine("myEngine");
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        // Note: no launchEngine frame
        {
          id: 88,
          msg: {
            type: "markdownForFile",
            engine: "myEngine",
            file: "doc.qmd",
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const err = responses.find((r) => r.msg.type === "error" && r.id === 88);
    expect(err).toBeDefined();
    expect((err?.msg as { type: string; message: string }).message).toMatch(
      /engine not launched/,
    );
  });

  it("execute frame → executeResult response with matching id", async () => {
    // Named revert: stub throws instead → response is 'error' not 'executeResult' → RED
    const engine = makeFakeEngine("myEngine", {
      executeResult: { markdown: "# rendered" },
    });
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/engines/my.ts" } },
        {
          id: 2,
          msg: {
            type: "launchEngine",
            engine: "myEngine",
            project: { isSingleFile: true },
          },
        },
        {
          id: 99,
          msg: {
            type: "execute",
            engine: "myEngine",
            options: {
              input: "# hello",
              sourcePath: "/proj/doc.qmd",
              format: {
                identifier: {
                  "base-format": "html",
                  "target-format": "html",
                  "display-name": "HTML",
                },
                metadata: {},
              },
              tempDir: "/tmp",
              cwd: "/proj",
              libDir: "/proj/libs",
              quiet: false,
              handledLanguages: [],
              dependencies: true,
              sourceMap: [],
            },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const result = responses.find((r) => r.msg.type === "executeResult");
    expect(result).toBeDefined();
    expect(result?.id).toBe(99);
    expect(
      (result?.msg as { type: string; result: TsExecuteResult }).result.markdown,
    ).toBe("# rendered");
  });
});

// ===========================================================================
// T-A5 — Init consumed, response-less, gates
// ===========================================================================

describe("T-A5 — Init consumed, response-less, gates", () => {
  it("Init is response-less: no frame carries the Init id", async () => {
    const engine = makeFakeEngine("initEngine", { initStashLines: true });
    const loader = engine.makeLoader();

    const duplex = makeDuplex();
    const host = makeFakeHost();
    const SPECIAL_INIT_ID = 42;

    // Send init with a specific id we'll check for
    duplex.push({
      id: SPECIAL_INIT_ID,
      msg: {
        type: "init",
        global: {
          resourceDir: "/res",
          runtimeDir: "/run",
          dataDir: "/data",
          pandocPath: null,
          isInteractiveSession: false,
          runningInCi: false,
          quartoVersion: "99.9.9",
        },
      },
    });
    duplex.push({
      id: 1,
      msg: { type: "loadEngine", enginePath: "/e/init-engine.ts" },
    });
    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    duplex.end();

    await runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: loader,
    });

    // Named revert: write a Response for the Init id → this assertion RED
    const hasInitIdResponse = duplex.responses.some(
      (r) => r.id === SPECIAL_INIT_ID,
    );
    expect(hasInitIdResponse).toBe(false);

    // Named revert: skip buildQuartoAPI (pass undefined to init) → this RED
    // (stashed result would be undefined or throw, not ["a","b"])
    expect(engine.initStash.linesResult).toEqual(["a", "b"]);
  });

  it("messages before Init get error 'message before Init'", async () => {
    const duplex = makeDuplex();
    const host = makeFakeHost();

    // Send a claimsLanguage WITHOUT sending init first
    duplex.push({
      id: 55,
      msg: { type: "claimsLanguage", engine: "x", language: "python" },
    });
    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    duplex.end();

    await runHost(duplex.reader, duplex.writer, host);

    // Named revert: remove the !quartoAPI guard → this RED
    const err = duplex.responses.find((r) => r.id === 55);
    expect(err).toBeDefined();
    expect(err?.msg.type).toBe("error");
    expect((err?.msg as { type: string; message: string }).message).toMatch(
      /message before Init/,
    );
  });
});

// ===========================================================================
// Init contract
// ===========================================================================

describe("Init contract", () => {
  it("init() called exactly once per loadEngine; repeat loadEngine (cache hit) does not re-call init", async () => {
    const engine = makeFakeEngine("engine1");
    const loader = engine.makeLoader();

    // Named revert: drop the cache-hit short-circuit → initCallCount > 1 → RED
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/engine1.ts" } },
        { id: 2, msg: { type: "loadEngine", enginePath: "/e/engine1.ts" } },
      ],
      { loadEngineModule: loader },
    );

    expect(engine.counters.init).toBe(1);
    expect(responses.filter((r) => r.msg.type === "loaded").length).toBe(2);
  });

  it("engine without init() loads cleanly, claimsLanguage succeeds", async () => {
    const engine = makeFakeEngine("engine2", { noInit: true });
    const loader = engine.makeLoader();

    // Named revert: make init non-optional (throw if absent) → this RED
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/engine2.ts" } },
        {
          id: 2,
          msg: {
            type: "claimsLanguage",
            engine: "engine2",
            language: "python",
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const loaded = responses.find((r) => r.msg.type === "loaded");
    expect(loaded).toBeDefined();
    const claims = responses.find((r) => r.msg.type === "claimsLanguageResult");
    expect(claims).toBeDefined();
    expect(engine.counters.init).toBe(0); // no init was defined
  });

  it("init() throwing → loadEngine responds error, engine NOT registered", async () => {
    const engine = makeFakeEngine("engine3", { initThrows: true });
    const loader = engine.makeLoader();

    // Named revert: drop the try/catch around init → error not sent → this RED
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/engine3.ts" } },
        // Subsequent claimsLanguage should also fail (engine not registered)
        {
          id: 2,
          msg: {
            type: "claimsLanguage",
            engine: "engine3",
            language: "python",
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const loadErr = responses.find(
      (r) => r.msg.type === "error" && r.id === 1,
    );
    expect(loadErr).toBeDefined();

    const claimsErr = responses.find(
      (r) => r.msg.type === "error" && r.id === 2,
    );
    // Named revert: don't remove engine from registry on init failure → claimsLanguage
    // would return a result instead of an error → this RED
    expect(claimsErr).toBeDefined();
    expect(
      (claimsErr?.msg as { type: string; message: string }).message,
    ).toMatch(/engine not loaded/);
  });

  it("init() throwing removes loadedByPath entry → retry with corrected engine succeeds", async () => {
    // Named revert: drop the try/catch that calls loadedByPath.delete(path) on init failure
    // → entry stays in map → retry gets the cached rejected promise → error again → RED
    let callCount = 0;

    const fakeInstance: ExecutionEngineInstance = {
      name: "retryEngine",
      canFreeze: false,
      markdownForFile: async (f) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async () => { throw new Error("n/a"); },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    const loader = async (_path: string): Promise<ExecutionEngineDiscovery> => {
      callCount++;
      if (callCount === 1) {
        // First call: init throws
        return {
          name: "retryEngine",
          defaultExt: ".qmd",
          defaultYaml: () => [],
          defaultContent: () => [],
          validExtensions: () => [".qmd"],
          claimsFile: () => false,
          claimsLanguage: () => false,
          canFreeze: false,
          generatesFigures: false,
          init: async () => {
            throw new Error("init failed on first attempt");
          },
          launch: () => fakeInstance,
        };
      }
      // Second call: succeeds (no init or safe init)
      return {
        name: "retryEngine",
        defaultExt: ".qmd",
        defaultYaml: () => [],
        defaultContent: () => [],
        validExtensions: () => [".qmd"],
        claimsFile: () => false,
        claimsLanguage: () => false,
        canFreeze: false,
        generatesFigures: false,
        launch: () => fakeInstance,
      };
    };

    const duplex = makeDuplex();
    const host = makeFakeHost();

    duplex.push(INIT_REQUEST);
    duplex.push({ id: 1, msg: { type: "loadEngine", enginePath: "/e/retry.ts" } });
    // Do NOT push the second frame yet — start runHost, let the first fail

    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: loader,
    });

    // Wait for first frame to complete (fail)
    await new Promise((r) => setTimeout(r, 20));

    // Now push the retry and shutdown
    duplex.push({ id: 2, msg: { type: "loadEngine", enginePath: "/e/retry.ts" } });
    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    duplex.end();

    await runPromise;

    const resp1 = duplex.responses.find((r) => r.id === 1);
    const resp2 = duplex.responses.find((r) => r.id === 2);

    expect(resp1?.msg.type).toBe("error"); // first attempt fails as expected
    // Named revert: drop try/catch → loadedByPath.delete not called → retry gets
    // cached rejected promise → also error → this assertion RED
    expect(resp2?.msg.type).toBe("loaded");
  });

  it("async init() is awaited — loaded written only AFTER async init resolves", async () => {
    // This engine's init blocks until we resolve the loaderDeferred.
    // Since the loader itself is non-blocking, only init blocks here.
    const engine = makeFakeEngine("engine4");
    // The init in the engine is async and blocks on engine.loaderDeferred.

    // Build a custom loader: returns immediately, but init is async and blocking
    let initDeferred = makeDeferred<void>();
    let loadedBeforeInitResolve = false;
    const discovery: ExecutionEngineDiscovery = {
      name: "engine4",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      init: async (_quarto) => {
        await initDeferred.promise; // blocks until we resolve
      },
      launch: () => engine.instance,
    };
    const loader = async (_path: string) => discovery;

    const duplex = makeDuplex();
    const host = makeFakeHost();

    duplex.push(INIT_REQUEST);
    duplex.push({ id: 1, msg: { type: "loadEngine", enginePath: "/e/engine4.ts" } });
    duplex.end();

    // Start runHost (will process init + loadEngine, then block on initDeferred)
    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: loader,
    });

    // Give the loop a chance to start and get blocked on initDeferred
    await new Promise((r) => setTimeout(r, 10));

    // Named revert: drop the defensive await on init → loaded is written before
    // init resolves → this check (no loaded yet) would still pass... but the
    // assertion on the sequence would fail. Actually, the key thing: loaded must
    // not appear before init resolves. Let's check:
    loadedBeforeInitResolve = duplex.responses.some(
      (r) => r.msg.type === "loaded",
    );
    expect(loadedBeforeInitResolve).toBe(false);

    // Now resolve init
    initDeferred.resolve(undefined);

    // Wait for runHost to finish (EOF closes the stream after the shutdown)
    await runPromise;

    // Now loaded should appear
    const loaded = duplex.responses.find((r) => r.msg.type === "loaded");
    expect(loaded).toBeDefined();
  });
});

// ===========================================================================
// Idempotency — sequential
// ===========================================================================

describe("Idempotency — sequential", () => {
  it("two loadEngine(samePath) → loader called once, init once, identical LoadEngineResult", async () => {
    const engine = makeFakeEngine("engineSeq");
    const loader = engine.makeLoader();

    // Named revert (load): drop the loadedByPath.has(path) cache-hit short-circuit
    // → loader/init count > 1 → RED
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/seq.ts" } },
        { id: 2, msg: { type: "loadEngine", enginePath: "/e/seq.ts" } },
      ],
      { loadEngineModule: loader },
    );

    expect(engine.counters.loader).toBe(1);
    expect(engine.counters.init).toBe(1);

    const loadedResponses = responses.filter((r) => r.msg.type === "loaded");
    expect(loadedResponses.length).toBe(2);

    // Both results must be identical
    expect(loadedResponses[0]?.msg).toEqual(loadedResponses[1]?.msg);
  });

  it("two launchEngine(sameName) → launch called once, identical LaunchEngineResult", async () => {
    const engine = makeFakeEngine("engineSeq2");
    const loader = engine.makeLoader();

    // Named revert (launch): drop the launchedByName.has(name) cache-hit short-circuit
    // → launch count > 1 → RED
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/seq2.ts" } },
        {
          id: 2,
          msg: {
            type: "launchEngine",
            engine: "engineSeq2",
            project: { isSingleFile: false },
          },
        },
        {
          id: 3,
          msg: {
            type: "launchEngine",
            engine: "engineSeq2",
            project: { isSingleFile: false },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    expect(engine.counters.launch).toBe(1);

    const launchedResponses = responses.filter((r) => r.msg.type === "launched");
    expect(launchedResponses.length).toBe(2);
    expect(launchedResponses[0]?.msg).toEqual(launchedResponses[1]?.msg);
  });
});

// ===========================================================================
// Idempotency — concurrent
// ===========================================================================

describe("Idempotency — concurrent (synchronous in-flight registration)", () => {
  it("K=3 loadEngine(samePath) concurrent → loader called once, init once, all K loaded responses identical", async () => {
    const K = 3;
    const engine = makeFakeEngine("engineConc");
    // The loader BLOCKS until we manually resolve → all K frames dispatched before resolution
    const loader = engine.makeLoader({ blockLoader: true });

    const duplex = makeDuplex();
    const host = makeFakeHost();

    duplex.push(INIT_REQUEST);
    for (let i = 0; i < K; i++) {
      duplex.push({
        id: 100 + i,
        msg: { type: "loadEngine", enginePath: "/e/conc.ts" },
      });
    }
    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    duplex.end();

    // Start runHost (will block inside loader until we resolve)
    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: loader,
    });

    // Give the loop time to dispatch all K frames (they're all in the queue,
    // loop processes them without awaiting the handlers).
    await new Promise((r) => setTimeout(r, 20));

    // Named revert: drop the synchronous in-flight registration (use resolved-only
    // cache check) → loader/init called K times → this assertion RED
    expect(engine.counters.loader).toBe(1);
    expect(engine.counters.init).toBe(0); // init runs inside the loader promise

    // Resolve the loader so all K handlers can complete
    engine.loaderDeferred.resolve(undefined);
    await runPromise;

    // After resolution, init should have run once
    expect(engine.counters.init).toBe(1);

    const loadedResponses = duplex.responses.filter(
      (r) => r.msg.type === "loaded",
    );
    // Named revert same as above: count < K → RED
    expect(loadedResponses.length).toBe(K);

    // All K results must be identical
    const first = JSON.stringify(loadedResponses[0]?.msg);
    for (const r of loadedResponses) {
      expect(JSON.stringify(r.msg)).toBe(first);
    }
  });

  it("K=3 launchEngine(sameName) concurrent — blocking launch, real in-flight window, launch called once", async () => {
    // STRONGER THAN THE OLD TEST: uses a blocking launch so K concurrent
    // launchEngine frames all arrive while the first call is still in-flight.
    // A "resolved-only cache" implementation would call launch K times; the
    // in-flight registration (launchedByName.set BEFORE first await) must
    // deduplicate them down to exactly 1 call.
    const K = 3;
    const engine = makeFakeEngine("engineConcLaunch", { blockLaunch: true });
    const loader = engine.makeLoader(); // non-blocking loader

    const duplex = makeDuplex();
    const host = makeFakeHost();

    // Phase 1: Load the engine (non-concurrent; wait for it to settle).
    duplex.push(INIT_REQUEST);
    duplex.push({
      id: 1,
      msg: { type: "loadEngine", enginePath: "/e/conc-launch.ts" },
    });

    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: loader,
    });

    // Give the loop time to process init + loadEngine fully.
    await new Promise((r) => setTimeout(r, 20));

    // Phase 2: Enqueue K concurrent launchEngine frames before resolving the deferred.
    for (let i = 0; i < K; i++) {
      duplex.push({
        id: 200 + i,
        msg: {
          type: "launchEngine",
          engine: "engineConcLaunch",
          project: { isSingleFile: false },
        },
      });
    }
    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    duplex.end();

    // Give the loop time to dispatch all K frames (fire-and-forget; loop does not
    // await their handlers, so all K are enqueued before any handler completes).
    await new Promise((r) => setTimeout(r, 20));

    // KEY ASSERTION: launch was called exactly ONCE while still in-flight.
    // Named revert (drop in-flight registration, use resolved-only cache):
    //   each of the K handlers sees no registered promise, calls launch itself
    //   → counters.launch === K → expect(...).toBe(1) goes RED.
    expect(engine.counters.launch).toBe(1);

    // Resolve the deferred — all K handlers can now complete.
    engine.launchDeferred.resolve(undefined);
    await runPromise;

    const launchedResponses = duplex.responses.filter(
      (r) => r.msg.type === "launched",
    );
    // All K frames must have received a response.
    expect(launchedResponses.length).toBe(K);

    // All K results must be identical (same in-flight promise shared).
    const first = JSON.stringify(launchedResponses[0]?.msg);
    for (const r of launchedResponses) {
      expect(JSON.stringify(r.msg)).toBe(first);
    }
  });
});

// ===========================================================================
// Additional correctness checks
// ===========================================================================

describe("LoadEngineResult correctness", () => {
  it("LoadEngineResult includes all required fields", async () => {
    const engine = makeFakeEngine("fieldCheck");
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [{ id: 1, msg: { type: "loadEngine", enginePath: "/e/fc.ts" } }],
      { loadEngineModule: loader },
    );

    const loaded = responses.find((r) => r.msg.type === "loaded");
    const disc = (loaded?.msg as { type: string; discovery: unknown }).discovery as {
      name: string;
      validExtensions: string[];
      generatesFigures: boolean;
      canFreeze: boolean;
      quartoRequired?: string | null;
    };
    expect(disc.name).toBe("fieldCheck");
    expect(disc.validExtensions).toEqual([".qmd", ".md"]);
    expect(disc.generatesFigures).toBe(true);
    expect(disc.canFreeze).toBe(false);
    expect(disc.quartoRequired).toBe(">= 1.0.0");
  });
});

describe("LaunchEngineResult correctness", () => {
  it("LaunchEngineResult carries canFreeze from instance", async () => {
    const engine = makeFakeEngine("launchCheck");
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/lc.ts" } },
        {
          id: 2,
          msg: {
            type: "launchEngine",
            engine: "launchCheck",
            project: { isSingleFile: true },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const launched = responses.find((r) => r.msg.type === "launched");
    const inst = (launched?.msg as { type: string; instance: unknown }).instance as {
      canFreeze: boolean;
    };
    // Our fake instance has canFreeze: true
    expect(inst.canFreeze).toBe(true);
  });
});

// ===========================================================================
// Drain-before-exit (Fix #2a)
// ===========================================================================

describe("Drain-before-exit", () => {
  it("in-flight handler response is preserved on EOF: runHost does not return until drain completes", async () => {
    // An engine whose markdownForFile blocks on a deferred.
    // runHost must not return (drain phase) until the handler finishes.
    const markdownDeferred = makeDeferred<void>();

    const instance: ExecutionEngineInstance = {
      name: "drainEngine",
      canFreeze: true,
      markdownForFile: async (_file: string) => {
        await markdownDeferred.promise; // blocks until resolved
        return {
          value: "# drained",
          fileName: undefined,
          map: () => undefined,
        };
      },
      target: async () => undefined,
      partitionedMarkdown: async () => {
        throw new Error("n/a");
      },
      execute: async () => {
        throw new Error("n/a");
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
      intermediateFiles: () => [],
    };

    const discovery: ExecutionEngineDiscovery = {
      name: "drainEngine",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: (_file: string, ext: string) => ext === ".qmd",
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: () => instance,
    };
    const loader = async (_path: string) => discovery;

    const duplex = makeDuplex();
    const host = makeFakeHost();

    // Send init, load, launch.
    duplex.push(INIT_REQUEST);
    duplex.push({
      id: 1,
      msg: { type: "loadEngine", enginePath: "/e/drain.ts" },
    });
    duplex.push({
      id: 2,
      msg: {
        type: "launchEngine",
        engine: "drainEngine",
        project: { isSingleFile: true },
      },
    });

    let runHostCompleted = false;
    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: loader,
    }).then(() => {
      runHostCompleted = true;
    });

    // Wait for load and launch to settle.
    await new Promise((r) => setTimeout(r, 20));

    // Push a blocking markdownForFile, then close the stream (EOF).
    // The loop will read markdownForFile (dispatch fire-and-forget), then hit
    // EOF and break. It then enters the drain phase waiting for the handler.
    duplex.push({
      id: 40,
      msg: { type: "markdownForFile", engine: "drainEngine", file: "doc.qmd" },
    });
    duplex.end(); // EOF — no shutdown frame; the stream just closes

    // Give the loop time to read all frames and enter the drain phase.
    await new Promise((r) => setTimeout(r, 20));

    // Named revert (remove `await Promise.allSettled([...inFlight])`):
    //   runHost returns immediately after EOF without waiting for the handler,
    //   so runHostCompleted would already be true here → RED.
    expect(runHostCompleted).toBe(false);

    // Resolve the deferred — the markdownForFile handler completes, drain settles.
    markdownDeferred.resolve(undefined);
    await runPromise;

    // Response must be present: drain kept it alive.
    const result = duplex.responses.find(
      (r) => r.msg.type === "markdownForFileResult",
    );
    expect(result).toBeDefined();
    expect(result?.id).toBe(40);
    expect(
      (
        result?.msg as {
          type: string;
          result: { value: string };
        }
      ).result.value,
    ).toBe("# drained");
  });
});

// ===========================================================================
// Shutdown-break (Fix #2b)
// ===========================================================================

describe("Shutdown-break", () => {
  it("shutdown causes loop exit even when stream stays open (no EOF)", async () => {
    // Keep the stream open after sending shutdown — if the `break` is missing,
    // the loop would block waiting for the next frame indefinitely.
    const engine = makeFakeEngine("sdBreakEngine");
    const loader = engine.makeLoader();

    const duplex = makeDuplex();
    const host = makeFakeHost();

    duplex.push(INIT_REQUEST);
    duplex.push({
      id: 1,
      msg: { type: "loadEngine", enginePath: "/e/sd-break.ts" },
    });
    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    // Deliberately do NOT call duplex.end() — the stream stays open.

    let runHostCompleted = false;
    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: loader,
    }).then(() => {
      runHostCompleted = true;
    });

    // Named revert (remove `if (msg.type === "shutdown") { break; }`):
    //   the loop tries to read the next frame from the open stream and blocks
    //   forever → runHostCompleted remains false after the wait → RED.
    await new Promise((r) => setTimeout(r, 50));
    expect(runHostCompleted).toBe(true);

    await runPromise;

    const loaded = duplex.responses.find((r) => r.msg.type === "loaded");
    expect(loaded).toBeDefined();
  });

  it("shutdown drains in-flight response: response is flushed before runHost returns", async () => {
    // An in-flight markdownForFile handler is blocked when shutdown arrives.
    // runHost must drain (wait for the handler) before returning.
    const markdownDeferred = makeDeferred<void>();

    const instance: ExecutionEngineInstance = {
      name: "sdDrainEngine",
      canFreeze: true,
      markdownForFile: async (_file: string) => {
        await markdownDeferred.promise;
        return {
          value: "# shutdown drain",
          fileName: undefined,
          map: () => undefined,
        };
      },
      target: async () => undefined,
      partitionedMarkdown: async () => {
        throw new Error("n/a");
      },
      execute: async () => {
        throw new Error("n/a");
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
      intermediateFiles: () => [],
    };

    const discovery: ExecutionEngineDiscovery = {
      name: "sdDrainEngine",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: (_file: string, ext: string) => ext === ".qmd",
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: () => instance,
    };
    const loader = async (_path: string) => discovery;

    const duplex = makeDuplex();
    const host = makeFakeHost();

    duplex.push(INIT_REQUEST);
    duplex.push({
      id: 1,
      msg: { type: "loadEngine", enginePath: "/e/sd-drain.ts" },
    });
    duplex.push({
      id: 2,
      msg: {
        type: "launchEngine",
        engine: "sdDrainEngine",
        project: { isSingleFile: true },
      },
    });

    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: loader,
    });

    // Wait for load / launch to settle.
    await new Promise((r) => setTimeout(r, 20));

    // Push the blocking markdownForFile, then shutdown immediately.
    // Stream stays open — shutdown is the exit signal, not EOF.
    duplex.push({
      id: 40,
      msg: {
        type: "markdownForFile",
        engine: "sdDrainEngine",
        file: "doc.qmd",
      },
    });
    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    // Do NOT call duplex.end() — loop must exit via the shutdown break.

    // Give the loop time to process both frames and enter the drain phase.
    await new Promise((r) => setTimeout(r, 20));

    // runHost should be in drain, waiting for the in-flight markdownForFile.
    // Named revert (remove drain): runHost returns immediately after shutdown
    //   break → the race below resolves "done" before markdownDeferred resolves
    //   → but then the response is absent → RED on the response assertion.
    // Named revert (remove shutdown break): loop blocks on next frame →
    //   runHost never returns → the Promise.race fires "timeout" → RED.
    const done = await Promise.race([
      (async () => {
        markdownDeferred.resolve(undefined);
        await runPromise;
        return "done" as const;
      })(),
      new Promise<"timeout">((r) => setTimeout(() => r("timeout"), 500)),
    ]);
    expect(done).toBe("done");

    // Response must be flushed.
    const result = duplex.responses.find(
      (r) => r.msg.type === "markdownForFileResult",
    );
    expect(result).toBeDefined();
    expect(result?.id).toBe(40);
    expect(
      (
        result?.msg as {
          type: string;
          result: { value: string };
        }
      ).result.value,
    ).toBe("# shutdown drain");
  });
});

// ===========================================================================
// Non-blocking (multiplexing) — Fix #3
// ===========================================================================

describe("Non-blocking dispatch (multiplexing)", () => {
  it("engine B's synchronous claimsFile response arrives before engine A's blocking handler resolves", async () => {
    // Engine A: markdownForFile blocks on a deferred.
    // Engine B: claimsFile is synchronous (fast).
    // The loop dispatches A's frame then B's frame WITHOUT awaiting A's handler,
    // so B's response must appear while A is still in-flight.
    //
    // Named revert (replace fire-and-forget with `await dispatch(id, msg)`):
    //   the loop blocks on A's handler; B's frame is never read until A resolves
    //   → bClaimsResult is absent when we assert before resolving A's deferred → RED.

    const engineAMarkdownDeferred = makeDeferred<void>();

    const instanceA: ExecutionEngineInstance = {
      name: "muxEngineA",
      canFreeze: true,
      markdownForFile: async (_file: string) => {
        await engineAMarkdownDeferred.promise;
        return {
          value: "# A done",
          fileName: undefined,
          map: () => undefined,
        };
      },
      target: async () => undefined,
      partitionedMarkdown: async () => {
        throw new Error("n/a");
      },
      execute: async () => {
        throw new Error("n/a");
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    const discoveryA: ExecutionEngineDiscovery = {
      name: "muxEngineA",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".a"],
      claimsFile: (_file: string, ext: string) => ext === ".a",
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: () => instanceA,
    };

    const discoveryB: ExecutionEngineDiscovery = {
      name: "muxEngineB",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".b"],
      claimsFile: (_file: string, ext: string) => ext === ".b",
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: () => {
        throw new Error("muxEngineB.launch not used");
      },
    };

    const loaderMap = new Map<string, ExecutionEngineDiscovery>([
      ["/e/mux-a.ts", discoveryA],
      ["/e/mux-b.ts", discoveryB],
    ]);
    const loader = async (path: string): Promise<ExecutionEngineDiscovery> => {
      const d = loaderMap.get(path);
      if (!d) throw new Error(`unknown engine path: ${path}`);
      return d;
    };

    const duplex = makeDuplex();
    const host = makeFakeHost();

    // Phase 1: load both engines and launch A.
    duplex.push(INIT_REQUEST);
    duplex.push({
      id: 1,
      msg: { type: "loadEngine", enginePath: "/e/mux-a.ts" },
    });
    duplex.push({
      id: 2,
      msg: { type: "loadEngine", enginePath: "/e/mux-b.ts" },
    });
    duplex.push({
      id: 3,
      msg: {
        type: "launchEngine",
        engine: "muxEngineA",
        project: { isSingleFile: true },
      },
    });

    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: loader,
    });

    // Wait for load/launch to settle.
    await new Promise((r) => setTimeout(r, 20));

    // Phase 2: dispatch A's blocking markdownForFile then B's synchronous claimsFile.
    duplex.push({
      id: 40,
      msg: { type: "markdownForFile", engine: "muxEngineA", file: "doc.a" },
    });
    duplex.push({
      id: 50,
      msg: {
        type: "claimsFile",
        engine: "muxEngineB",
        file: "doc.b",
        ext: ".b",
      },
    });
    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    duplex.end();

    // Give the loop time to dispatch both frames (fire-and-forget) and for
    // B's synchronous handler to complete.
    await new Promise((r) => setTimeout(r, 20));

    // B's claimsFile must be done (it's fast) — arrived before A's deferred resolves.
    const bResult = duplex.responses.find(
      (r) => r.msg.type === "claimsFileResult",
    );
    expect(bResult).toBeDefined();
    expect(bResult?.id).toBe(50);
    expect(
      (bResult?.msg as { type: string; result: boolean }).result,
    ).toBe(true);

    // A's markdownForFile must NOT be done yet (deferred still unresolved).
    const aResultBefore = duplex.responses.find(
      (r) => r.msg.type === "markdownForFileResult",
    );
    expect(aResultBefore).toBeUndefined();

    // Resolve A's deferred — both handlers complete.
    engineAMarkdownDeferred.resolve(undefined);
    await runPromise;

    const aResultAfter = duplex.responses.find(
      (r) => r.msg.type === "markdownForFileResult",
    );
    expect(aResultAfter).toBeDefined();
    expect(aResultAfter?.id).toBe(40);
  });
});

// ===========================================================================
// Error-precision: "not loaded" vs "not launched" (Fix #6)
// ===========================================================================

describe("Error precision: not-loaded vs not-launched", () => {
  it("markdownForFile on a never-loaded engine → 'engine not loaded' error (not 'not launched')", async () => {
    const { responses } = await runWithFrames(
      [
        {
          id: 88,
          msg: {
            type: "markdownForFile",
            engine: "ghostEngine",
            file: "doc.qmd",
          },
        },
      ],
      {
        loadEngineModule: async () => {
          throw new Error("should not be called");
        },
      },
    );

    // Named revert (remove engineByName check in markdownForFile):
    //   falls through to launchedByName check → throws "engine not launched"
    //   → message does NOT match /engine not loaded/ → RED.
    const err = responses.find((r) => r.msg.type === "error" && r.id === 88);
    expect(err).toBeDefined();
    expect(
      (err?.msg as { type: string; message: string }).message,
    ).toMatch(/engine not loaded/);
  });

  it("markdownForFile on a loaded-but-not-launched engine → 'engine not launched' error", async () => {
    const engine = makeFakeEngine("notLaunchedEngine");
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        {
          id: 1,
          msg: { type: "loadEngine", enginePath: "/e/not-launched.ts" },
        },
        // intentionally NO launchEngine
        {
          id: 88,
          msg: {
            type: "markdownForFile",
            engine: "notLaunchedEngine",
            file: "doc.qmd",
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const err = responses.find((r) => r.msg.type === "error" && r.id === 88);
    expect(err).toBeDefined();
    expect(
      (err?.msg as { type: string; message: string }).message,
    ).toMatch(/engine not launched/);
    expect(
      (err?.msg as { type: string; message: string }).message,
    ).not.toMatch(/engine not loaded/);
  });

  it("intermediateFiles on a never-loaded engine → 'engine not loaded' error", async () => {
    const { responses } = await runWithFrames(
      [
        {
          id: 99,
          msg: {
            type: "intermediateFiles",
            engine: "ghostEngine2",
            input: "doc.qmd",
          },
        },
      ],
      {
        loadEngineModule: async () => {
          // this comment preserves spacing
          throw new Error("should not be called");
        },
      },
    );

    const err = responses.find((r) => r.msg.type === "error" && r.id === 99);
    expect(err).toBeDefined();
    expect(
      (err?.msg as { type: string; message: string }).message,
    ).toMatch(/engine not loaded/);
  });
});

// ===========================================================================
// Execute field routing
// ===========================================================================

describe("Execute field routing", () => {
  it("routes every ExecuteResult field to TsExecuteResult with correct key-rename and defaults", async () => {
    // Full ExecuteResult with every field set, including engine (which must be DROPPED)
    // and htmlDependencies (q2-native extra field, default [] for Q1 engines).
    const fullResult: Partial<ExecuteResult> & { htmlDependencies?: HtmlDependency[] } = {
      markdown: "# full result",
      supporting: ["support.html", "support.js"],
      filters: ["my-filter.lua"],
      metadata: { "my-key": "my-val" },
      pandoc: { toc: true },
      includes: {
        "include-in-header": ["<style>h1{color:red}</style>"],
        "include-before-body": ["<div class='before'>"],
        "include-after-body": ["</div>"],
      },
      engine: "fullEngine", // must be DROPPED on the wire
      engineDependencies: { knitr: [{ type: "package", name: "ggplot2" }] },
      preserve: { abc123: "<pre>preserved</pre>" },
      postProcess: true,
      resourceFiles: ["fig.png", "data.csv"],
      // htmlDependencies is q2-native (not in Q1 ExecuteResult) — here we use absolute paths
      // to avoid the libDir normalization logic (that's tested separately).
      htmlDependencies: [{ name: "bootstrap", stylesheets: ["/abs/b.css"], scripts: ["/abs/b.js"] }],
    };

    const engine = makeFakeEngine("fullEngine", { executeResult: fullResult });
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/full.ts" } },
        { id: 2, msg: { type: "launchEngine", engine: "fullEngine", project: { isSingleFile: true } } },
        {
          id: 99,
          msg: {
            type: "execute",
            engine: "fullEngine",
            options: {
              input: "# content",
              sourcePath: "/proj/doc.qmd",
              format: {
                identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" },
                metadata: {},
              },
              tempDir: "/tmp",
              cwd: "/proj",
              libDir: "/proj/libs",
              quiet: false,
              handledLanguages: [],
              dependencies: true,
              sourceMap: [],
            },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const resp = responses.find((r) => r.msg.type === "executeResult" && r.id === 99);
    expect(resp).toBeDefined();
    const result = (resp?.msg as { type: string; result: TsExecuteResult }).result;

    // markdown
    expect(result.markdown).toBe("# full result");

    // Named revert: drop the `supporting` forward → its assertion RED
    expect(result.supporting).toEqual(["support.html", "support.js"]);

    // filters forwarded
    expect(result.filters).toEqual(["my-filter.lua"]);

    // metadata carried
    expect(result.metadata).toEqual({ "my-key": "my-val" });

    // pandoc carried
    expect(result.pandoc).toEqual({ toc: true });

    // Named revert: leave includes keys un-renamed (Q1 keys on the wire) → inHeader assertion RED
    expect(result.includes?.inHeader).toEqual(["<style>h1{color:red}</style>"]);
    expect(result.includes?.beforeBody).toEqual(["<div class='before'>"]);
    expect(result.includes?.afterBody).toEqual(["</div>"]);
    // Q1 keys must NOT appear on the wire
    expect((result.includes as Record<string, unknown>)?.["include-in-header"]).toBeUndefined();

    // Named revert: copy `engine` onto the wire → the "engine absent" assertion RED
    expect((result as unknown as Record<string, unknown>).engine).toBeUndefined();

    // engineDependencies forwarded verbatim
    expect(result.engineDependencies).toEqual({ knitr: [{ type: "package", name: "ggplot2" }] });

    // preserve carried
    expect(result.preserve).toEqual({ abc123: "<pre>preserved</pre>" });

    // postProcess carried
    expect(result.postProcess).toBe(true);

    // resourceFiles forwarded
    expect(result.resourceFiles).toEqual(["fig.png", "data.csv"]);

    // htmlDependencies forwarded (absolute paths unchanged)
    expect(result.htmlDependencies).toEqual([
      { name: "bootstrap", stylesheets: ["/abs/b.css"], scripts: ["/abs/b.js"] },
    ]);
  });

  it("missing optional ExecuteResult fields get correct defaults", async () => {
    // Engine returns ONLY the required fields; test that defaults are applied.
    const engine = makeFakeEngine("defaultsEngine", {
      executeResult: { markdown: "# minimal" },
    });
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/defaults.ts" } },
        { id: 2, msg: { type: "launchEngine", engine: "defaultsEngine", project: { isSingleFile: true } } },
        {
          id: 50,
          msg: {
            type: "execute",
            engine: "defaultsEngine",
            options: {
              input: "# min",
              sourcePath: "/proj/doc.qmd",
              format: {
                identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" },
                metadata: {},
              },
              tempDir: "/tmp",
              cwd: "/proj",
              libDir: "/proj/libs",
              quiet: false,
              handledLanguages: [],
              dependencies: true,
              sourceMap: [],
            },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const resp = responses.find((r) => r.msg.type === "executeResult" && r.id === 50);
    const result = (resp?.msg as { type: string; result: TsExecuteResult }).result;

    // Q1 optionals → wire required defaults
    expect(result.supporting).toEqual([]);
    expect(result.filters).toEqual([]);
    expect(result.resourceFiles).toEqual([]);
    expect(result.preserve).toEqual({});
    expect(result.postProcess).toBe(false);
    expect(result.htmlDependencies).toEqual([]);
    expect(result.includes).toBeUndefined();
    expect(result.metadata).toBeUndefined();
    expect(result.pandoc).toBeUndefined();
    expect(result.engineDependencies).toBeUndefined();
  });
});

// ===========================================================================
// F2b deferred-deps — engineDependencies forwarding + dependencies verb
// ===========================================================================

describe("F2b deferred-deps", () => {
  // Shared bespoke F2b engine: execute inspects options.dependencies flag,
  // dependencies() is tracked with a counter.
  function makeF2bEngine() {
    const dependenciesCallCount = { value: 0 };

    const instance: ExecutionEngineInstance = {
      name: "f2bEngine",
      canFreeze: false,
      markdownForFile: async (f: string) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async (options) => {
        if (!options.dependencies) {
          // Deferred mode: return engineDependencies for later resolution
          return {
            markdown: "# deferred mode",
            supporting: [],
            filters: [],
            engineDependencies: { knitr: [{ type: "package", name: "ggplot2" }] },
          };
        }
        // Inline mode: no engineDependencies
        return { markdown: "# inline mode", supporting: [], filters: [] };
      },
      dependencies: async () => {
        dependenciesCallCount.value++;
        return {
          includes: { "include-in-header": ["<style>knitr{}</style>"] },
        };
      },
      postprocess: async () => {},
      intermediateFiles: () => [],
    };

    const discovery: ExecutionEngineDiscovery = {
      name: "f2bEngine",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: () => instance,
    };

    return { discovery, instance, dependenciesCallCount };
  }

  it("execute with dependencies:false forwards engineDependencies verbatim AND did NOT call instance.dependencies()", async () => {
    const { discovery, dependenciesCallCount } = makeF2bEngine();
    const loader = async (_p: string) => discovery;

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/f2b.ts" } },
        { id: 2, msg: { type: "launchEngine", engine: "f2bEngine", project: { isSingleFile: true } } },
        {
          id: 10,
          msg: {
            type: "execute",
            engine: "f2bEngine",
            options: {
              input: "# content",
              sourcePath: "/proj/doc.qmd",
              format: {
                identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" },
                metadata: {},
              },
              tempDir: "/tmp",
              cwd: "/proj",
              libDir: "/proj/libs",
              quiet: false,
              handledLanguages: [],
              dependencies: false, // deferred mode
              sourceMap: [],
            },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const resp = responses.find((r) => r.msg.type === "executeResult" && r.id === 10);
    expect(resp).toBeDefined();
    const result = (resp?.msg as { type: string; result: TsExecuteResult }).result;

    // Named revert: drop `engineDependencies` from the forwarded result → forwarding RED
    expect(result.engineDependencies).toEqual({ knitr: [{ type: "package", name: "ggplot2" }] });

    // Named revert: make `execute` call `dependencies()` itself (the old fold) → "did not
    // call dependencies()" RED
    expect(dependenciesCallCount.value).toBe(0);
  });

  it("execute with dependencies:true → engineDependencies absent in response", async () => {
    const { discovery } = makeF2bEngine();
    const loader = async (_p: string) => discovery;

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/f2b.ts" } },
        { id: 2, msg: { type: "launchEngine", engine: "f2bEngine", project: { isSingleFile: true } } },
        {
          id: 11,
          msg: {
            type: "execute",
            engine: "f2bEngine",
            options: {
              input: "# content",
              sourcePath: "/proj/doc.qmd",
              format: {
                identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" },
                metadata: {},
              },
              tempDir: "/tmp",
              cwd: "/proj",
              libDir: "/proj/libs",
              quiet: false,
              handledLanguages: [],
              dependencies: true, // inline mode → no engineDependencies returned
              sourceMap: [],
            },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const resp = responses.find((r) => r.msg.type === "executeResult" && r.id === 11);
    expect(resp).toBeDefined();
    const result = (resp?.msg as { type: string; result: TsExecuteResult }).result;

    // Engine did not include engineDependencies in inline mode
    expect(result.engineDependencies == null).toBe(true);
  });

  it("dependencies verb calls instance.dependencies() and responds with key-renamed includes", async () => {
    const { discovery, dependenciesCallCount } = makeF2bEngine();
    const loader = async (_p: string) => discovery;

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/f2b.ts" } },
        { id: 2, msg: { type: "launchEngine", engine: "f2bEngine", project: { isSingleFile: true } } },
        {
          id: 20,
          msg: {
            type: "dependencies",
            engine: "f2bEngine",
            options: {
              input: "# content",
              sourcePath: "/proj/doc.qmd",
              sourceMap: [],
              format: {
                identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" },
                metadata: {},
              },
              output: "/proj/doc.html",
              tempDir: "/tmp",
              dependencies: [{ type: "package", name: "ggplot2" }],
              quiet: false,
            },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    // Named revert: remove the `dependencies` case arm → the verb-response RED
    const resp = responses.find((r) => r.msg.type === "dependenciesResult" && r.id === 20);
    expect(resp).toBeDefined();
    const includes = (resp?.msg as { type: string; includes: TsPandocIncludes }).includes;

    // Key-renamed: "include-in-header" → inHeader
    expect(includes.inHeader).toEqual(["<style>knitr{}</style>"]);
    expect((includes as Record<string, unknown>)["include-in-header"]).toBeUndefined();

    // The harness called instance.dependencies() exactly once
    expect(dependenciesCallCount.value).toBe(1);
  });
});

// ===========================================================================
// htmlDependencies forwarding
// ===========================================================================

describe("htmlDependencies forwarding", () => {
  it("normalizes relative stylesheet/script paths to absolute vs libDir", async () => {
    // Named revert A: drop the `result.htmlDependencies` read → empty → "entries appear" RED
    // Named revert B: drop the rel→abs normalization → paths stay relative → "resolved" RED
    const relResult: Partial<ExecuteResult> & { htmlDependencies?: HtmlDependency[] } = {
      markdown: "# html deps",
      supporting: [],
      filters: [],
      htmlDependencies: [
        {
          name: "myDep",
          stylesheets: ["s.css"],    // relative → must become /lib/s.css
          scripts: ["j.js"],          // relative → must become /lib/j.js
        },
      ],
    };

    const engine = makeFakeEngine("htmlDepsEngine", { executeResult: relResult });
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/htmldeps.ts" } },
        { id: 2, msg: { type: "launchEngine", engine: "htmlDepsEngine", project: { isSingleFile: true } } },
        {
          id: 30,
          msg: {
            type: "execute",
            engine: "htmlDepsEngine",
            options: {
              input: "# content",
              sourcePath: "/proj/doc.qmd",
              format: {
                identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" },
                metadata: {},
              },
              tempDir: "/tmp",
              cwd: "/proj",
              libDir: "/lib", // the base for relative resolution
              quiet: false,
              handledLanguages: [],
              dependencies: true,
              sourceMap: [],
            },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const resp = responses.find((r) => r.msg.type === "executeResult" && r.id === 30);
    expect(resp).toBeDefined();
    const result = (resp?.msg as { type: string; result: TsExecuteResult }).result;

    // Named revert A: drop htmlDependencies read → this assertion RED
    expect(result.htmlDependencies).toHaveLength(1);

    const dep = result.htmlDependencies[0];
    expect(dep?.name).toBe("myDep");

    // Named revert B: drop normalization → paths stay "s.css"/"j.js" → RED
    expect(dep?.stylesheets).toEqual(["/lib/s.css"]);
    expect(dep?.scripts).toEqual(["/lib/j.js"]);
  });

  it("absolute htmlDep paths are passed through unchanged", async () => {
    const absResult: Partial<ExecuteResult> & { htmlDependencies?: HtmlDependency[] } = {
      markdown: "# abs",
      supporting: [],
      filters: [],
      htmlDependencies: [
        {
          name: "absDep",
          stylesheets: ["/already/absolute.css"],
          scripts: ["/already/absolute.js"],
        },
      ],
    };

    const engine = makeFakeEngine("absHtmlDepsEngine", { executeResult: absResult });
    const loader = engine.makeLoader();

    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/abs.ts" } },
        { id: 2, msg: { type: "launchEngine", engine: "absHtmlDepsEngine", project: { isSingleFile: true } } },
        {
          id: 31,
          msg: {
            type: "execute",
            engine: "absHtmlDepsEngine",
            options: {
              input: "# content",
              sourcePath: "/proj/doc.qmd",
              format: {
                identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" },
                metadata: {},
              },
              tempDir: "/tmp",
              cwd: "/proj",
              libDir: "/lib",
              quiet: false,
              handledLanguages: [],
              dependencies: true,
              sourceMap: [],
            },
          },
        },
      ],
      { loadEngineModule: loader },
    );

    const resp = responses.find((r) => r.msg.type === "executeResult" && r.id === 31);
    const result = (resp?.msg as { type: string; result: TsExecuteResult }).result;

    const dep = result.htmlDependencies[0];
    // Absolute paths must not be modified
    expect(dep?.stylesheets).toEqual(["/already/absolute.css"]);
    expect(dep?.scripts).toEqual(["/already/absolute.js"]);
  });
});

// ===========================================================================
// T-A2 — project context threading
// ===========================================================================

describe("T-A2 — project context threading", () => {
  it("launchEngine reconstructs rich project: config.outputDir declared vs getOutputDirectory() resolved — two distinct values", async () => {
    let capturedCtx: RichProjectContext | undefined;

    const fakeInstance: ExecutionEngineInstance = {
      name: "ta2Engine",
      canFreeze: false,
      markdownForFile: async (f: string) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async () => ({ markdown: "", supporting: [], filters: [] }),
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    const ta2Discovery: ExecutionEngineDiscovery = {
      name: "ta2Engine",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: (ctx: RichProjectContext) => {
        capturedCtx = ctx;
        return fakeInstance;
      },
    };

    const ta2Loader = async (_p: string) => ta2Discovery;

    // Named revert A: stop threading msg.project into reconstruction (pass undefined/empty)
    //   → the "matches project" assertion RED
    // Named revert B: source both declared outputDir and resolved outputDir from the same field
    //   → the distinctness assertion RED
    await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/ta2.ts" } },
        {
          id: 2,
          msg: {
            type: "launchEngine",
            engine: "ta2Engine",
            project: {
              projectDir: "/abs/proj",
              isSingleFile: false,
              config: { "output-dir": "_site" },
              outputDir: "/abs/proj/_site",
            },
          },
        },
      ],
      { loadEngineModule: ta2Loader },
    );

    expect(capturedCtx).toBeDefined();

    // Declared outputDir (from config key "output-dir") — the raw key value
    expect(capturedCtx?.config?.project?.outputDir).toBe("_site");

    // Resolved outputDir (from wire outputDir field) — the absolute resolved path
    expect(capturedCtx?.getOutputDirectory()).toBe("/abs/proj/_site");

    // The two must be DISTINCT (not collapsed onto a single field)
    expect(capturedCtx?.config?.project?.outputDir).not.toBe(capturedCtx?.getOutputDirectory());
  });
});

// ===========================================================================
// T-A1 — ambient API accessible pre-launch (Plan 2 Phase A: path.runtime real)
// ===========================================================================

describe("T-A1 — ambient API accessible pre-launch", () => {
  it("path.runtime() is callable pre-launch and returns the runtime path (no error)", async () => {
    // Engine stores the QuartoAPI from init() then calls path.runtime() inside claimsLanguage().
    // path.runtime() is now a real implementation (Plan 2 Phase A) — it must work pre-launch,
    // returning the runtimeDir path from the init global config, NOT throw a gating error.
    let ta1API: QuartoAPI | undefined;
    let capturedRuntimePath: string | undefined;

    const ta1Instance: ExecutionEngineInstance = {
      name: "ta1Engine",
      canFreeze: false,
      markdownForFile: async (f: string) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async () => ({ markdown: "", supporting: [], filters: [] }),
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    const ta1Discovery: ExecutionEngineDiscovery = {
      name: "ta1Engine",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      // claimsLanguage calls path.runtime() — now returns the runtime path, does not throw
      claimsLanguage: () => {
        capturedRuntimePath = ta1API!.path.runtime();
        return false;
      },
      canFreeze: false,
      generatesFigures: false,
      init: (quarto: QuartoAPI) => {
        ta1API = quarto;
      },
      launch: () => ta1Instance,
    };

    const ta1Loader = async (_p: string) => ta1Discovery;

    // No launchEngine — path.runtime() must work (not throw), returning the global runtimeDir
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/ta1.ts" } },
        { id: 2, msg: { type: "claimsLanguage", engine: "ta1Engine", language: "python" } },
      ],
      { loadEngineModule: ta1Loader },
    );

    // Named revert: introduce a launch-state gate on path.runtime → returns an error
    // response instead of a claimsLanguageResult → RED (err would be defined, result undefined)
    const err = responses.find((r) => r.msg.type === "error" && r.id === 2);
    expect(err).toBeUndefined(); // No error — path.runtime() succeeded

    const result = responses.find((r) => r.msg.type === "claimsLanguageResult" && r.id === 2);
    expect(result).toBeDefined(); // Got a valid claimsLanguageResult

    // path.runtime() returned the runtimeDir from the init global config ("/run" from INIT_REQUEST)
    expect(capturedRuntimePath).toBe("/run");
  });
});

// ===========================================================================
// execute projectDir from launch context (fix: use richProject.dir, not opts.projectDir)
// ===========================================================================

describe("execute projectDir from launch context", () => {
  it("execute options.projectDir comes from launch context dir, not wire execute options", async () => {
    // Capture the ExecuteOptions.projectDir received by the fake engine's execute().
    let capturedProjectDir: string | undefined;

    const fakeInstance: ExecutionEngineInstance = {
      name: "projDirEngine",
      canFreeze: false,
      markdownForFile: async (f: string) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async (options) => {
        capturedProjectDir = options.projectDir;
        return { markdown: "# done", supporting: [], filters: [] };
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    const fakeDiscovery: ExecutionEngineDiscovery = {
      name: "projDirEngine",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: () => fakeInstance,
    };

    await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/projdir.ts" } },
        {
          id: 2,
          msg: {
            type: "launchEngine",
            engine: "projDirEngine",
            project: { projectDir: "/abs/proj", isSingleFile: false },
          },
        },
        {
          id: 3,
          msg: {
            type: "execute",
            engine: "projDirEngine",
            options: {
              input: "# content",
              sourcePath: "/abs/proj/doc.qmd",
              format: {
                identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" },
                metadata: {},
              },
              tempDir: "/tmp",
              cwd: "/abs/proj",
              libDir: "/abs/proj/libs",
              quiet: false,
              handledLanguages: [],
              dependencies: true,
              sourceMap: [],
              // NOTE: projectDir intentionally ABSENT from wire execute options —
              // the host must source it from the launch context, not the wire.
            },
          },
        },
      ],
      { loadEngineModule: async (_p: string) => fakeDiscovery },
    );

    // Named revert: change `richProject.dir` back to `opts.projectDir ?? undefined`
    // → execute options omit projectDir → capturedProjectDir is undefined → RED.
    expect(capturedProjectDir).toBe("/abs/proj");
  });
});

// ===========================================================================
// T-A4 — no shared cross-engine state (Plan 2 Phase A: path.runtime real)
// ===========================================================================

describe("T-A4 — no shared cross-engine state", () => {
  it("engine B path.runtime() returns the same path before and after engine A launches (no shared mutable state)", async () => {
    // Two independent engines; each captures the QuartoAPI it received in init().
    // Engine B calls path.runtime() inside claimsLanguage().
    // We test that before A launches and after A launches, B gets the SAME path result —
    // demonstrating no shared mutable context between engines.
    let apiB: QuartoAPI | undefined;
    const capturedPaths: string[] = [];

    const minimalInstance = (engineName: string): ExecutionEngineInstance => ({
      name: engineName,
      canFreeze: false,
      markdownForFile: async (f: string) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async () => ({ markdown: "", supporting: [], filters: [] }),
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    });

    const ta4DiscoveryA: ExecutionEngineDiscovery = {
      name: "ta4EngineA",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: () => minimalInstance("ta4EngineA"),
    };

    const ta4DiscoveryB: ExecutionEngineDiscovery = {
      name: "ta4EngineB",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      // claimsLanguage calls path.runtime() on B's API — now returns the runtime path
      claimsLanguage: () => {
        capturedPaths.push(apiB!.path.runtime());
        return false;
      },
      canFreeze: false,
      generatesFigures: false,
      init: (quarto: QuartoAPI) => {
        apiB = quarto;
      },
      launch: () => minimalInstance("ta4EngineB"),
    };

    const loaderMap = new Map<string, ExecutionEngineDiscovery>([
      ["/e/ta4-a.ts", ta4DiscoveryA],
      ["/e/ta4-b.ts", ta4DiscoveryB],
    ]);
    const ta4Loader = async (path: string) => {
      const d = loaderMap.get(path);
      if (!d) throw new Error(`unknown: ${path}`);
      return d;
    };

    // Sequence:
    //   id=1: loadA, id=2: loadB
    //   id=3: claimsLanguageB (pre-A-launch) → result1
    //   id=4: launchA
    //   id=5: claimsLanguageB (post-A-launch) → result2
    const { responses } = await runWithFrames(
      [
        { id: 1, msg: { type: "loadEngine", enginePath: "/e/ta4-a.ts" } },
        { id: 2, msg: { type: "loadEngine", enginePath: "/e/ta4-b.ts" } },
        { id: 3, msg: { type: "claimsLanguage", engine: "ta4EngineB", language: "python" } },
        { id: 4, msg: { type: "launchEngine", engine: "ta4EngineA", project: { isSingleFile: true } } },
        { id: 5, msg: { type: "claimsLanguage", engine: "ta4EngineB", language: "python" } },
      ],
      { loadEngineModule: ta4Loader },
    );

    // No errors from engine B's claimsLanguage calls
    const err3 = responses.find((r) => r.msg.type === "error" && r.id === 3);
    const err5 = responses.find((r) => r.msg.type === "error" && r.id === 5);
    expect(err3).toBeUndefined();
    expect(err5).toBeUndefined();

    // Named revert: introduce a shared mutable context slot set by A's launch() + a gate
    // consulting it → B's pre-A-launch and post-A-launch calls return different values
    // → capturedPaths[0] !== capturedPaths[1] → RED
    expect(capturedPaths).toHaveLength(2);
    expect(capturedPaths[0]).toBe("/run"); // runtimeDir from INIT_REQUEST
    expect(capturedPaths[1]).toBe("/run"); // same after engine A launches
    expect(capturedPaths[0]).toBe(capturedPaths[1]); // no shared mutable state
  });
});

// ===========================================================================
// T5 — per-engine execute serialization queue
// ===========================================================================

describe("T5 — per-engine execute serialization queue", () => {
  // Standard minimal TsExecuteOptions reused across T5/T6/T7 tests.
  // The explicit `: Request` return type lets TypeScript contextually type
  // `metadata: {}` as `Record<string, TsMetadataValue>` (empty literal satisfies
  // any record type, but only when contextually typed).
  function makeExecMsg(id: number, engine: string): Request {
    return {
      id,
      msg: {
        type: "execute" as const,
        engine,
        options: {
          input: "# hello",
          sourcePath: "/proj/doc.qmd",
          format: {
            identifier: {
              "base-format": "html",
              "target-format": "html",
              "display-name": "HTML",
            },
            metadata: {},
          },
          tempDir: "/tmp",
          cwd: "/proj",
          libDir: "/proj/libs",
          quiet: false,
          handledLanguages: [],
          dependencies: true,
          sourceMap: [],
        },
      },
    };
  }

  it("cross-engine concurrency: B's execute starts while A is in-flight", async () => {
    // Engine A blocks on a deferred; Engine B completes immediately.
    // A and B are different engines → their executes should run concurrently on the
    // event loop (independent per-engine queues).
    //
    // Named revert: make loop `await dispatch(id, msg)` before reading the next
    // frame → B's frame is never read until A's handler resolves → RED.
    const aDeferred = makeDeferred<void>();
    let bStarted = false;

    const aInstance: ExecutionEngineInstance = {
      name: "t5EngineA",
      canFreeze: false,
      markdownForFile: async (f) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async () => {
        await aDeferred.promise; // blocks until resolved
        return { markdown: "# A done", supporting: [], filters: [] };
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    const bInstance: ExecutionEngineInstance = {
      name: "t5EngineB",
      canFreeze: false,
      markdownForFile: async (f) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async () => {
        bStarted = true; // side-effect that fires as soon as B's execute starts
        return { markdown: "# B done", supporting: [], filters: [] };
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    const makeDiscovery = (name: string, instance: ExecutionEngineInstance) => ({
      name,
      defaultExt: ".qmd" as const,
      defaultYaml: () => [] as string[],
      defaultContent: () => [] as string[],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: () => instance,
    });

    const discoveryA = makeDiscovery("t5EngineA", aInstance);
    const discoveryB = makeDiscovery("t5EngineB", bInstance);

    const loaderMap = new Map([
      ["/e/a.ts", discoveryA],
      ["/e/b.ts", discoveryB],
    ]);
    const loader = async (p: string) => {
      const d = loaderMap.get(p);
      if (!d) throw new Error(`unknown: ${p}`);
      return d;
    };

    const duplex = makeDuplex();
    const host = makeFakeHost();

    duplex.push(INIT_REQUEST);
    duplex.push({ id: 1, msg: { type: "loadEngine", enginePath: "/e/a.ts" } });
    duplex.push({ id: 2, msg: { type: "loadEngine", enginePath: "/e/b.ts" } });
    duplex.push({ id: 3, msg: { type: "launchEngine", engine: "t5EngineA", project: { isSingleFile: true } } });
    duplex.push({ id: 4, msg: { type: "launchEngine", engine: "t5EngineB", project: { isSingleFile: true } } });

    const runPromise = runHost(duplex.reader, duplex.writer, host, { loadEngineModule: loader });

    // Wait for load/launch to settle.
    await new Promise((r) => setTimeout(r, 20));

    // Dispatch A (blocks) and B (fast) — both fire-and-forget from the loop.
    duplex.push(makeExecMsg(10, "t5EngineA"));
    duplex.push(makeExecMsg(20, "t5EngineB"));

    // Give event loop time for B's fast execute to complete while A is still blocking.
    await new Promise((r) => setTimeout(r, 20));

    // B's execute must have started (and completed) while A is still in-flight.
    // Named revert: await dispatch → B not yet dispatched → bStarted === false → RED.
    expect(bStarted).toBe(true);

    // A is still in-flight (deferred not resolved).
    const aResult = duplex.responses.find((r) => r.id === 10);
    expect(aResult).toBeUndefined(); // A not done yet

    // B should be done.
    const bResult = duplex.responses.find((r) => r.id === 20);
    expect(bResult?.msg.type).toBe("executeResult");

    // Resolve A, drain.
    aDeferred.resolve();
    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    duplex.end();
    await runPromise;

    // Both results present.
    expect(duplex.responses.find((r) => r.id === 10)?.msg.type).toBe("executeResult");
    expect(duplex.responses.find((r) => r.id === 20)?.msg.type).toBe("executeResult");
  });

  it("same-engine serialization: second execute does not start until first resolves", async () => {
    // Both executes target the same engine. With the per-engine queue, the second
    // execute is chained behind the first and cannot start until the first settles.
    //
    // Named revert: remove the per-engine queue (dispatch same-engine executes
    // concurrently) → second starts immediately alongside first → ordering RED.
    const firstDeferred = makeDeferred<void>();
    const executionOrder: string[] = [];

    let callIndex = 0;

    const zInstance: ExecutionEngineInstance = {
      name: "t5EngineZ",
      canFreeze: false,
      markdownForFile: async (f) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async () => {
        const n = ++callIndex;
        executionOrder.push(`start-${n}`);
        if (n === 1) {
          await firstDeferred.promise; // first execute blocks
        }
        executionOrder.push(`end-${n}`);
        return { markdown: `# done-${n}`, supporting: [], filters: [] };
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    const discoveryZ: ExecutionEngineDiscovery = {
      name: "t5EngineZ",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: () => zInstance,
    };

    const duplex = makeDuplex();
    const host = makeFakeHost();

    duplex.push(INIT_REQUEST);
    duplex.push({ id: 1, msg: { type: "loadEngine", enginePath: "/e/z.ts" } });
    duplex.push({ id: 2, msg: { type: "launchEngine", engine: "t5EngineZ", project: { isSingleFile: true } } });

    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: async () => discoveryZ,
    });

    // Wait for load/launch.
    await new Promise((r) => setTimeout(r, 20));

    // Send two executes to the SAME engine — second must queue behind first.
    duplex.push(makeExecMsg(10, "t5EngineZ"));
    duplex.push(makeExecMsg(20, "t5EngineZ"));

    // Wait for both to be dispatched and for the first to start (but not finish).
    await new Promise((r) => setTimeout(r, 20));

    // First execute started but blocked.
    // Second has NOT started yet (serialized behind first).
    // Named revert: without queue, second starts immediately → "start-2" already in order → RED.
    expect(executionOrder).not.toContain("start-2");
    expect(executionOrder).toContain("start-1"); // first did start

    // Resolve first; second should then run.
    firstDeferred.resolve();
    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    duplex.end();
    await runPromise;

    // Named revert: without queue, second starts before first ends → order is wrong → RED.
    expect(executionOrder).toEqual(["start-1", "end-1", "start-2", "end-2"]);

    // Both responses present.
    expect(duplex.responses.find((r) => r.id === 10)?.msg.type).toBe("executeResult");
    expect(duplex.responses.find((r) => r.id === 20)?.msg.type).toBe("executeResult");
  });
});

// ===========================================================================
// T6 — cooperative Cancel via AbortController
// ===========================================================================

describe("T6 — cooperative Cancel", () => {
  it("Cancel{target:N} fires signal; N responds cancelled; sibling on other engine unaffected", async () => {
    // Engine N: execute blocks on a deferred and listens for abort → throws AbortError
    //   when the signal fires. Engine M: immediate, unaffected by the cancel.
    //
    // Named reverts:
    //   ▸ drop the cancel arm (no .abort()) → signal never fires → N's execute never
    //     rejects → N doesn't get a 'cancelled' response → RED.
    //   ▸ abort ALL inflight controllers (not just target) → M's signal also fires →
    //     M's execute also rejects → M responds 'cancelled' instead of 'executeResult'
    //     → "sibling unaffected" assertion RED.
    const nDeferred = makeDeferred<void>();
    let nSignalFired = false;

    const nInstance: ExecutionEngineInstance = {
      name: "t6EngineN",
      canFreeze: false,
      markdownForFile: async (f) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async (options) => {
        // q2 extension signal — cast to access it (Q1 engines ignore the field)
        const signal = (options as unknown as { signal?: AbortSignal }).signal;
        await new Promise<void>((resolve, reject) => {
          if (signal?.aborted) {
            nSignalFired = true;
            reject(Object.assign(new Error("Aborted"), { name: "AbortError" }));
            return;
          }
          if (signal) {
            signal.addEventListener(
              "abort",
              () => {
                nSignalFired = true;
                reject(Object.assign(new Error("Aborted"), { name: "AbortError" }));
              },
              { once: true },
            );
          }
          nDeferred.promise.then(resolve, reject);
        });
        return { markdown: "# N done", supporting: [], filters: [] };
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    let mStarted = false;
    const mInstance: ExecutionEngineInstance = {
      name: "t6EngineM",
      canFreeze: false,
      markdownForFile: async (f) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async () => {
        mStarted = true;
        return { markdown: "# M done", supporting: [], filters: [] };
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    const makeDisc = (n: string, inst: ExecutionEngineInstance): ExecutionEngineDiscovery => ({
      name: n,
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: () => inst,
    });

    const loaderMap = new Map([
      ["/e/n.ts", makeDisc("t6EngineN", nInstance)],
      ["/e/m.ts", makeDisc("t6EngineM", mInstance)],
    ]);

    const duplex = makeDuplex();
    const host = makeFakeHost();

    duplex.push(INIT_REQUEST);
    duplex.push({ id: 1, msg: { type: "loadEngine", enginePath: "/e/n.ts" } });
    duplex.push({ id: 2, msg: { type: "loadEngine", enginePath: "/e/m.ts" } });
    duplex.push({ id: 3, msg: { type: "launchEngine", engine: "t6EngineN", project: { isSingleFile: true } } });
    duplex.push({ id: 4, msg: { type: "launchEngine", engine: "t6EngineM", project: { isSingleFile: true } } });

    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: async (p) => {
        const d = loaderMap.get(p);
        if (!d) throw new Error(`unknown: ${p}`);
        return d;
      },
    });

    // Wait for load/launch.
    await new Promise((r) => setTimeout(r, 20));

    // Dispatch N (blocks) and M (fast/immediate) — both fire-and-forget.
    duplex.push({ id: 100, msg: { type: "execute", engine: "t6EngineN", options: {
      input: "# hello", sourcePath: "/proj/doc.qmd",
      format: { identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" }, metadata: {} },
      tempDir: "/tmp", cwd: "/proj", libDir: "/lib",
      quiet: false, handledLanguages: [], dependencies: true, sourceMap: [],
    }}});
    duplex.push({ id: 200, msg: { type: "execute", engine: "t6EngineM", options: {
      input: "# hello", sourcePath: "/proj/doc.qmd",
      format: { identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" }, metadata: {} },
      tempDir: "/tmp", cwd: "/proj", libDir: "/lib",
      quiet: false, handledLanguages: [], dependencies: true, sourceMap: [],
    }}});

    // Wait for M to complete and N to be blocked.
    await new Promise((r) => setTimeout(r, 20));

    // M completed normally (it's fast).
    expect(mStarted).toBe(true);
    expect(duplex.responses.find((r) => r.id === 200)?.msg.type).toBe("executeResult");

    // N is still in-flight.
    expect(duplex.responses.find((r) => r.id === 100)).toBeUndefined();

    // Send Cancel{target:100} — targets N.
    duplex.push({ id: 999, msg: { type: "cancel", target: 100 } });

    // Give the event loop time to process the cancel and N's AbortError path.
    await new Promise((r) => setTimeout(r, 20));

    // Named revert: drop cancel arm (no .abort()) → nSignalFired stays false → RED.
    expect(nSignalFired).toBe(true);

    // N responds as 'cancelled' under id=100.
    const nResp = duplex.responses.find((r) => r.id === 100);
    expect(nResp?.msg.type).toBe("cancelled");

    // M is unaffected.
    const mResp = duplex.responses.find((r) => r.id === 200);
    expect(mResp?.msg.type).toBe("executeResult");

    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    duplex.end();
    await runPromise;
  });

  it("Cancel{target} while sibling STILL in-flight: target cancelled, sibling completes normally", async () => {
    // N and P are both blocking. We cancel N while P is still running.
    // P must complete normally (abort ALL controllers revert → P also cancelled → RED).
    //
    // Named reverts:
    //   ▸ drop cancel arm → N's signal never fires → N not cancelled → RED
    //   ▸ abort ALL controllers → P's signal fires → P also cancelled → RED
    const nDeferred = makeDeferred<void>();
    const pDeferred = makeDeferred<void>();
    let nSignalFiredP2 = false;
    let pSignalFired = false;

    const makeAbortAwareInstance = (
      name: string,
      deferred: Deferred<void>,
      onSignal: () => void,
    ): ExecutionEngineInstance => ({
      name,
      canFreeze: false,
      markdownForFile: async (f) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async (options) => {
        const signal = (options as unknown as { signal?: AbortSignal }).signal;
        await new Promise<void>((resolve, reject) => {
          if (signal?.aborted) { onSignal(); reject(Object.assign(new Error("Aborted"), { name: "AbortError" })); return; }
          if (signal) {
            signal.addEventListener("abort", () => {
              onSignal();
              reject(Object.assign(new Error("Aborted"), { name: "AbortError" }));
            }, { once: true });
          }
          deferred.promise.then(resolve, reject);
        });
        return { markdown: `# ${name} done`, supporting: [], filters: [] };
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    });

    const nInstance = makeAbortAwareInstance("t6EngineN2", nDeferred, () => { nSignalFiredP2 = true; });
    const pInstance = makeAbortAwareInstance("t6EngineP", pDeferred, () => { pSignalFired = true; });

    const loaderMap2 = new Map([
      ["/e/n2.ts", { name: "t6EngineN2", defaultExt: ".qmd", defaultYaml: () => [], defaultContent: () => [], validExtensions: () => [".qmd"], claimsFile: () => false, claimsLanguage: () => false, canFreeze: false, generatesFigures: false, launch: () => nInstance }],
      ["/e/p.ts",  { name: "t6EngineP",  defaultExt: ".qmd", defaultYaml: () => [], defaultContent: () => [], validExtensions: () => [".qmd"], claimsFile: () => false, claimsLanguage: () => false, canFreeze: false, generatesFigures: false, launch: () => pInstance }],
    ] as [string, ExecutionEngineDiscovery][]);

    const duplex2 = makeDuplex();
    const host2 = makeFakeHost();

    duplex2.push(INIT_REQUEST);
    duplex2.push({ id: 1, msg: { type: "loadEngine", enginePath: "/e/n2.ts" } });
    duplex2.push({ id: 2, msg: { type: "loadEngine", enginePath: "/e/p.ts" } });
    duplex2.push({ id: 3, msg: { type: "launchEngine", engine: "t6EngineN2", project: { isSingleFile: true } } });
    duplex2.push({ id: 4, msg: { type: "launchEngine", engine: "t6EngineP",  project: { isSingleFile: true } } });

    const opts6b = { input: "# hi", sourcePath: "/p/doc.qmd", format: { identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" }, metadata: {} }, tempDir: "/tmp", cwd: "/p", libDir: "/p/libs", quiet: false, handledLanguages: [], dependencies: true, sourceMap: [] };

    const runPromise2 = runHost(duplex2.reader, duplex2.writer, host2, {
      loadEngineModule: async (p) => { const d = loaderMap2.get(p); if (!d) throw new Error(`unknown: ${p}`); return d; },
    });

    await new Promise((r) => setTimeout(r, 20));

    duplex2.push({ id: 100, msg: { type: "execute", engine: "t6EngineN2", options: opts6b } });
    duplex2.push({ id: 200, msg: { type: "execute", engine: "t6EngineP",  options: opts6b } });

    await new Promise((r) => setTimeout(r, 20));

    // Both N and P are in-flight and blocking.
    expect(duplex2.responses.find((r) => r.id === 100)).toBeUndefined();
    expect(duplex2.responses.find((r) => r.id === 200)).toBeUndefined();

    // Cancel only N.
    duplex2.push({ id: 999, msg: { type: "cancel", target: 100 } });

    await new Promise((r) => setTimeout(r, 20));

    // Named revert (no .abort()): nSignalFiredP2 stays false → RED.
    expect(nSignalFiredP2).toBe(true);

    // N is cancelled.
    expect(duplex2.responses.find((r) => r.id === 100)?.msg.type).toBe("cancelled");

    // Named revert (abort ALL): pSignalFired becomes true → P also cancelled → RED.
    expect(pSignalFired).toBe(false);
    // P should still be running (not yet cancelled, not yet completed).
    expect(duplex2.responses.find((r) => r.id === 200)).toBeUndefined();

    // Now let P complete normally.
    pDeferred.resolve();
    duplex2.push({ id: 9999, msg: { type: "shutdown" } });
    duplex2.end();
    await runPromise2;

    // P completed normally after N was cancelled — sibling unaffected.
    expect(duplex2.responses.find((r) => r.id === 200)?.msg.type).toBe("executeResult");
  });
});

// ===========================================================================
// T7 — poison + transparent re-launch
// ===========================================================================

describe("T7 — poison + transparent re-launch", () => {
  it("Cancel A → B transparently re-launches (exactly once) → C reuses rebuilt instance → all ordered", async () => {
    // One engine ("t7Julia"). A's execute blocks and is abort-aware. B and C queue
    // behind A. Cancelling A poisons the instance. B's dequeue finds the instance
    // missing and reconstructs exactly once. C runs on the already-rebuilt instance.
    //
    // Named reverts:
    //   ▸ fail instead of reconstruct (throw "engine not launched" on missing instance)
    //     → B gets an 'error' response, not 'executeResult' → RED.
    //   ▸ move check-and-reconstruct into the synchronous dispatch prologue
    //     (before chaining onto queue) → B and C both observe the missing instance in
    //     their prologues and both call launch() → launchCallCount becomes 3 → RED.
    const aDeferred = makeDeferred<void>();
    let aSignalFired = false;
    let launchCallCount = 0;

    // Track what each execute call returned.
    const executeResults: string[] = [];

    // First launch (original): abort-aware blocking instance for A's execute.
    // Subsequent executes on this instance never run (A is cancelled and instance poisoned).
    const aInstance: ExecutionEngineInstance = {
      name: "t7Julia",
      canFreeze: false,
      markdownForFile: async (f) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async (options) => {
        const signal = (options as unknown as { signal?: AbortSignal }).signal;
        await new Promise<void>((resolve, reject) => {
          if (signal?.aborted) { aSignalFired = true; reject(Object.assign(new Error("Aborted"), { name: "AbortError" })); return; }
          if (signal) {
            signal.addEventListener("abort", () => {
              aSignalFired = true;
              reject(Object.assign(new Error("Aborted"), { name: "AbortError" }));
            }, { once: true });
          }
          aDeferred.promise.then(resolve, reject);
        });
        return { markdown: "# A done", supporting: [], filters: [] };
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    // Re-launched instance (after poison): immediate — B and C run on this.
    const bcInstance: ExecutionEngineInstance = {
      name: "t7Julia",
      canFreeze: false,
      markdownForFile: async (f) => ({ value: f, map: () => undefined }),
      target: async () => undefined,
      partitionedMarkdown: async () => { throw new Error("n/a"); },
      execute: async (_options) => {
        const mark = `BC-${executeResults.length}`;
        executeResults.push(mark);
        return { markdown: `# ${mark}`, supporting: [], filters: [] };
      },
      dependencies: async () => ({ includes: {} }),
      postprocess: async () => {},
    };

    const t7Discovery: ExecutionEngineDiscovery = {
      name: "t7Julia",
      defaultExt: ".qmd",
      defaultYaml: () => [],
      defaultContent: () => [],
      validExtensions: () => [".qmd"],
      claimsFile: () => false,
      claimsLanguage: () => false,
      canFreeze: false,
      generatesFigures: false,
      launch: (_ctx) => {
        launchCallCount++;
        // First launch returns the blocking abort-aware instance (for A).
        // Re-launch (launchCallCount >= 2) returns the immediate instance (for B, C).
        return launchCallCount === 1 ? aInstance : bcInstance;
      },
    };

    const duplex = makeDuplex();
    const host = makeFakeHost();

    duplex.push(INIT_REQUEST);
    duplex.push({ id: 1, msg: { type: "loadEngine", enginePath: "/e/julia.ts" } });
    duplex.push({ id: 2, msg: { type: "launchEngine", engine: "t7Julia", project: { isSingleFile: false } } });

    const runPromise = runHost(duplex.reader, duplex.writer, host, {
      loadEngineModule: async () => t7Discovery,
    });

    // Wait for load/launch to settle.
    await new Promise((r) => setTimeout(r, 20));

    // Confirm initial launch happened exactly once.
    expect(launchCallCount).toBe(1);

    const opts7 = {
      input: "# hello", sourcePath: "/proj/doc.qmd",
      format: { identifier: { "base-format": "html", "target-format": "html", "display-name": "HTML" }, metadata: {} },
      tempDir: "/tmp", cwd: "/proj", libDir: "/proj/libs",
      quiet: false, handledLanguages: [], dependencies: true, sourceMap: [],
    };

    // A: blocks on abort-aware deferred.
    duplex.push({ id: 10, msg: { type: "execute", engine: "t7Julia", options: opts7 } });
    // B: queued behind A.
    duplex.push({ id: 20, msg: { type: "execute", engine: "t7Julia", options: opts7 } });
    // C: queued behind B.
    duplex.push({ id: 30, msg: { type: "execute", engine: "t7Julia", options: opts7 } });

    // Wait for A to be dispatched and start executing (B and C are queued).
    await new Promise((r) => setTimeout(r, 20));

    // A is in-flight, B and C queued — no results yet.
    expect(duplex.responses.find((r) => r.id === 10)).toBeUndefined();
    expect(duplex.responses.find((r) => r.id === 20)).toBeUndefined();
    expect(duplex.responses.find((r) => r.id === 30)).toBeUndefined();

    // Cancel A.
    duplex.push({ id: 999, msg: { type: "cancel", target: 10 } });

    // Give the event loop time to:
    //   cancel A → A AbortError → write cancelled → poison instance
    //   B dequeues → missing instance → re-launch (launchCallCount → 2) → execute B → write result
    //   C dequeues → instance present → execute C → write result
    await new Promise((r) => setTimeout(r, 30));

    // A was cancelled.
    expect(aSignalFired).toBe(true);
    const aResp = duplex.responses.find((r) => r.id === 10);
    expect(aResp?.msg.type).toBe("cancelled");

    // Named revert (fail instead of reconstruct): B fails → type === 'error' → RED.
    const bResp = duplex.responses.find((r) => r.id === 20);
    expect(bResp?.msg.type).toBe("executeResult");

    // C also completed normally on the rebuilt instance.
    const cResp = duplex.responses.find((r) => r.id === 30);
    expect(cResp?.msg.type).toBe("executeResult");

    // The reconstruct happened exactly ONCE (B triggered it; C found the instance present).
    // Total launchCallCount: 1 (initial) + 1 (re-launch by B) = 2.
    // Named revert (reconstruct in prologue): C's prologue also sees missing instance
    //   and calls launch() → launchCallCount becomes 3 → RED.
    expect(launchCallCount).toBe(2);

    duplex.push({ id: 9999, msg: { type: "shutdown" } });
    duplex.end();
    await runPromise;
  });
});
