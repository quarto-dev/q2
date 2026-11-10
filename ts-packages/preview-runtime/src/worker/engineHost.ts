/**
 * The render engine's side of the worker boundary (preview VFS contract
 * D3, D9). `serveEngine` answers the main thread's calls against the
 * engine (`wasmRenderer.ts` running inside the worker):
 *
 * - VFS writes from the sync client;
 * - `renderPreview`, which renders and carries back the render artifacts
 *   under `/.quarto/` that changed, for the main thread's mirror;
 * - the pure helpers the SPA's edit path needs.
 *
 * Requests run one at a time, in arrival order, so a write that arrives
 * while a render is running is applied after that render and before the
 * next: every render sees a fixed VFS.
 */

import type { PreviewRenderResponse, VfsResponse } from '../wasmRenderer';
import type {
  AstResponse,
  ResolvePandocFormatsResponse,
  RenderPageForPreviewOptions,
} from 'wasm-quarto-hub-client';
import { Rpc, type Endpoint } from './rpc';
import type { ArtifactDelta } from './vfsMirror';
import { createXhrMissHandler, type MissHandlerObject } from './missShim';

/** The engine surface the worker serves. `wasmRenderer.ts` satisfies it. */
export interface EngineApi {
  initWasm(): Promise<void>;
  vfsAddFile(path: string, content: string): VfsResponse;
  vfsAddBinaryFile(path: string, content: Uint8Array): VfsResponse;
  vfsRemoveFile(path: string): VfsResponse;
  vfsClear(): VfsResponse;
  vfsListFiles(): VfsResponse;
  vfsReadFile(path: string): VfsResponse;
  vfsReadBinaryFile(path: string): VfsResponse;
  renderPageForPreview(
    path: string,
    userGrammars?: unknown,
    captureGzJson?: Uint8Array,
    options?: RenderPageForPreviewOptions,
  ): Promise<PreviewRenderResponse>;
  resolvePandocFormats(path: string, options?: unknown): ResolvePandocFormatsResponse;
  parseQmdContentSync(text: string): AstResponse;
  applyNodeEdit(
    content: string,
    untransformedAstJson: string,
    destinationSourceInfoJson: string,
    modifiedSubtreeJson: string,
  ): string;
  regenerateNestedBuffers(content: string, untransformedAstJson: string): Record<string, string>;
  vfsSetMissHandler(handler: unknown): void;
}

/** What the main thread sends at start-up: flags a worker cannot read itself. */
export interface EngineInit {
  /** `globalThis.__Q2_EPHEMERAL_STORAGE__` on the host page. */
  ephemeralStorage: boolean;
  /** Whether perf counters are on in the host page. */
  perf: boolean;
  /**
   * The preview server's miss routes, ending in `/api/preview/` (preview
   * VFS contract D2). Absent: no miss handler, every answer is local.
   */
  missBaseUrl?: string;
}

/** A `renderPreview` answer. */
export interface RenderPreviewResult {
  response: PreviewRenderResponse;
  artifacts: ArtifactDelta;
  /** Files pulled from the server during the render, for the page's mirror. */
  pulled: Array<{ path: string; bytes: Uint8Array }>;
}

/** Hooks the worker entry installs (the perf flag, the rasterizer bridge). */
export interface EngineHostHooks {
  setPerf?(enabled: boolean): void;
  /** Called once with a rasterizer that asks the main thread. */
  installRasterizer?(
    rasterize: (svg: Uint8Array, maxSide: number) => Promise<Uint8Array>,
  ): void | Promise<void>;
}

/** VFS prefixes whose files are render artifacts the main thread reads. */
const ARTIFACT_PREFIXES = ['/.quarto/', '/project/.quarto/'];

export function serveEngine(
  endpoint: Endpoint,
  engine: EngineApi,
  hooks: EngineHostHooks & {
    /** Build the miss handler (tests substitute a fake transport). */
    createMissHandler?: typeof createXhrMissHandler;
  } = {},
): Rpc {
  let missHandler: MissHandlerObject | null = null;
  let pulled: RenderPreviewResult['pulled'] = [];
  // What the main thread's mirror holds for each artifact, as last sent.
  const sent = new Map<string, string>();

  const artifactDelta = (): ArtifactDelta => {
    const listed = engine.vfsListFiles().files ?? [];
    const present = new Set<string>();
    const changed: ArtifactDelta['changed'] = [];
    for (const path of listed) {
      if (!ARTIFACT_PREFIXES.some((p) => path.startsWith(p))) continue;
      present.add(path);
      const text = engine.vfsReadFile(path);
      let key: string;
      let entry: ArtifactDelta['changed'][number];
      if (text.success && text.content !== undefined) {
        key = `t:${text.content}`;
        entry = { path, text: text.content };
      } else {
        const binary = engine.vfsReadBinaryFile(path);
        if (!binary.success || binary.content === undefined) continue;
        key = `b:${binary.content}`;
        entry = { path, base64: binary.content };
      }
      if (sent.get(path) !== key) {
        sent.set(path, key);
        changed.push(entry);
      }
    }
    const removed = [...sent.keys()].filter((path) => !present.has(path));
    for (const path of removed) sent.delete(path);
    return { changed, removed };
  };

  const rpc: Rpc = new Rpc(
    endpoint,
    {
      init: async (init: EngineInit) => {
        (globalThis as Record<string, unknown>).__Q2_EPHEMERAL_STORAGE__ = init.ephemeralStorage;
        hooks.setPerf?.(init.perf);
        await hooks.installRasterizer?.((svg, maxSide) =>
          rpc.call<Uint8Array>('rasterize', [svg, maxSide]),
        );
        await engine.initWasm();
        if (init.missBaseUrl) {
          missHandler = (hooks.createMissHandler ?? createXhrMissHandler)(init.missBaseUrl, {
            onPull: (path, bytes) => pulled.push({ path, bytes }),
          });
          engine.vfsSetMissHandler(missHandler);
        }
      },
      missStats: () => missHandler?.counts() ?? {},
      vfsAddFile: (path: string, content: string) => engine.vfsAddFile(path, content),
      vfsAddBinaryFile: (path: string, content: Uint8Array) => engine.vfsAddBinaryFile(path, content),
      vfsRemoveFile: (path: string) => engine.vfsRemoveFile(path),
      vfsClear: () => {
        sent.clear();
        return engine.vfsClear();
      },
      renderPreview: async (
        path: string,
        captureGzJson?: Uint8Array,
        options?: RenderPageForPreviewOptions,
      ): Promise<RenderPreviewResult> => {
        pulled = [];
        const response = await engine.renderPageForPreview(path, undefined, captureGzJson, options);
        const result = { response, artifacts: artifactDelta(), pulled };
        pulled = [];
        return result;
      },
      resolvePandocFormats: (path: string, options?: unknown) =>
        engine.resolvePandocFormats(path, options),
      parseQmdContent: (text: string) => engine.parseQmdContentSync(text),
      applyNodeEdit: (
        content: string,
        untransformedAstJson: string,
        destinationSourceInfoJson: string,
        modifiedSubtreeJson: string,
      ) => engine.applyNodeEdit(content, untransformedAstJson, destinationSourceInfoJson, modifiedSubtreeJson),
      regenerateNestedBuffers: (content: string, untransformedAstJson: string) =>
        engine.regenerateNestedBuffers(content, untransformedAstJson),
    },
    { serial: true },
  );
  return rpc;
}
