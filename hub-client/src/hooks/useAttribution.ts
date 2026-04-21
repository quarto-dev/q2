/**
 * React hook for per-character attribution from Automerge history.
 *
 * Builds a run-list attribution map (see attribution-runs.ts) asynchronously
 * on mount and refreshes it incrementally on edits.
 */

import { useState, useEffect, useRef, useCallback, useMemo, createContext } from 'react';
import { buildByteToCharMap, getNodeAttribution, HistoryCompactedError } from '../services/attribution';
import type { AttributionSource, NodeAttribution } from '../services/attribution';
import {
  buildRunListAttribution,
  updateRunListAttribution,
  makeRunListSource,
} from '../services/attribution-runs';
import type { RunListAttribution } from '../services/attribution-runs';
import { getFileHandle } from '../services/automergeSync';
import type { ActorIdentity } from '../services/automergeSync';
import type { SerializableSourceInfo, RustFileInfo, SourceContext } from '@quarto/pandoc-types';
import { SourceInfoReconstructor } from '@quarto/annotated-qmd';

// ---------------------------------------------------------------------------
// Context — shared with the Ast component tree
// ---------------------------------------------------------------------------

export interface AttributionValue {
  source: AttributionSource;
  identities: Record<string, ActorIdentity>;
  sourceText: string;
}

export const AttributionContext = createContext<AttributionValue | null>(null);

/** Resolver produced by `useNodeAttributionResolver` — what a render subtree consumes. */
export interface NodeAttributionResolver {
  getNodeAttribution: (sourceInfoId: number) => NodeAttribution | null;
}

export const NodeAttributionContext = createContext<NodeAttributionResolver | null>(null);

/** Shape of the `astContext` field on a parsed AST. */
export interface AstAttributionContext {
  sourceInfoPool: SerializableSourceInfo[];
  files: RustFileInfo[];
}

/**
 * Build a memoized per-render resolver that maps a `sourceInfoId` to
 * `NodeAttribution`. Encapsulates `SourceInfoReconstructor` construction
 * (including the `files[0].content` injection required because WASM JSON
 * omits file content), and caches results within a single `useMemo` lifetime.
 *
 * Returns `null` when either input is missing or the reconstructor throws.
 */
export function useNodeAttributionResolver(
  astContext: AstAttributionContext | null | undefined,
  attributionCtx: AttributionValue | null,
): NodeAttributionResolver | null {
  return useMemo(() => {
    if (!astContext || !attributionCtx) return null;

    try {
      const sourceContext: SourceContext = {
        files: astContext.files.map((f, idx) => ({
          id: idx,
          path: f.name,
          content: idx === 0 ? attributionCtx.sourceText : (f.content ?? ''),
        })),
      };

      const reconstructor = new SourceInfoReconstructor(
        astContext.sourceInfoPool,
        sourceContext,
      );

      const cache = new Map<number, NodeAttribution | null>();

      return {
        getNodeAttribution: (sourceInfoId: number) => {
          const cached = cache.get(sourceInfoId);
          if (cached !== undefined) return cached;
          const result = getNodeAttribution(
            sourceInfoId,
            reconstructor,
            attributionCtx.source,
            attributionCtx.identities,
          );
          cache.set(sourceInfoId, result);
          return result;
        },
      };
    } catch {
      return null;
    }
  }, [astContext, attributionCtx]);
}

// ---------------------------------------------------------------------------
// Hook return type
// ---------------------------------------------------------------------------

interface UseAttributionResult {
  source: AttributionSource;
}

// ---------------------------------------------------------------------------
// Debounce delay for incremental updates (ms)
// ---------------------------------------------------------------------------

const DEBOUNCE_MS = 500;

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

export function useAttribution(
  filePath: string | null,
  sourceText: string,
): UseAttributionResult | null {
  const [result, setResult] = useState<UseAttributionResult | null>(null);

  // Ref to hold the latest run list — avoids stale closures in debounced callbacks
  const mapRef = useRef<RunListAttribution | null>(null);

  // Abort controller for cancelling in-flight builds
  const abortRef = useRef<AbortController | null>(null);

  // Debounce timer for incremental updates
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Ref for sourceText — avoids re-creating startBuild when text changes
  const sourceTextRef = useRef(sourceText);
  sourceTextRef.current = sourceText;

  // Start a fresh async build — stable callback (no sourceText dependency)
  const startBuild = useCallback((path: string) => {
    // Abort any in-flight build
    if (abortRef.current) {
      abortRef.current.abort();
    }

    const controller = new AbortController();
    abortRef.current = controller;

    // Clear any stale attribution up-front. Without this, a previous file's
    // `source` (with its own byteToChar map baked in) briefly renders
    // against the new file's AST on re-navigation, which flashes colors on
    // the first block before the new build lands.
    setResult(null);
    mapRef.current = null;

    const handle = getFileHandle(path);
    if (!handle) return;

    buildRunListAttribution(handle, 'text', controller.signal).then(map => {
      if (controller.signal.aborted) return;

      if (map) {
        const byteToChar = buildByteToCharMap(sourceTextRef.current);
        mapRef.current = map;
        setResult({ source: makeRunListSource(map.runs, byteToChar) });
      } else {
        mapRef.current = null;
        setResult(null);
      }
    }).catch(() => {
      // Build failed — leave result as-is
    });
  }, []);

  // Initial build on mount or filePath change
  useEffect(() => {
    if (!filePath) {
      setResult(null);
      mapRef.current = null;
      return;
    }

    startBuild(filePath);

    return () => {
      // Cancel in-flight build on unmount or path change
      if (abortRef.current) {
        abortRef.current.abort();
      }
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
      }
    };
  }, [filePath, startBuild]);

  // Incremental update on sourceText change (debounced)
  useEffect(() => {
    if (!filePath || !mapRef.current) return;

    // Clear any pending debounce
    if (debounceRef.current) {
      clearTimeout(debounceRef.current);
    }

    debounceRef.current = setTimeout(() => {
      const handle = getFileHandle(filePath);
      if (!handle || !mapRef.current) return;

      try {
        const updatedMap = updateRunListAttribution(mapRef.current, handle, 'text');
        const byteToChar = buildByteToCharMap(sourceText);
        mapRef.current = updatedMap;
        setResult({ source: makeRunListSource(updatedMap.runs, byteToChar) });
      } catch (err) {
        if (err instanceof HistoryCompactedError) {
          // History was compacted — need a full rebuild
          mapRef.current = null;
          setResult(null);
          startBuild(filePath);
        }
      }
    }, DEBOUNCE_MS);

    return () => {
      if (debounceRef.current) {
        clearTimeout(debounceRef.current);
      }
    };
  }, [sourceText, filePath, startBuild]);

  return result;
}
