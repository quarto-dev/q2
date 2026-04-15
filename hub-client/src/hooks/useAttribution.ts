/**
 * React hook for per-character attribution from Automerge history.
 *
 * Provides an AttributionMap (per-character actor + timestamp) for the
 * current document, built asynchronously on mount and updated incrementally
 * on edits.
 */

import { useState, useEffect, useRef, useCallback, createContext } from 'react';
import {
  buildAttributionMap,
  updateAttributionMap,
  buildByteToCharMap,
  HistoryCompactedError,
} from '../services/attribution';
import type { AttributionMap } from '../services/attribution';
import { getFileHandle } from '../services/automergeSync';
import type { ActorIdentity } from '../services/automergeSync';

// ---------------------------------------------------------------------------
// Context — shared with the Ast component tree
// ---------------------------------------------------------------------------

export const AttributionContext = createContext<{
  attributionMap: AttributionMap;
  byteToCharMap: number[];
  identities: Record<string, ActorIdentity>;
  sourceText: string;
} | null>(null);

// ---------------------------------------------------------------------------
// Hook return type
// ---------------------------------------------------------------------------

interface UseAttributionResult {
  attributionMap: AttributionMap;
  byteToCharMap: number[];
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

  // Ref to hold the latest map — avoids stale closures in debounced callbacks
  const mapRef = useRef<AttributionMap | null>(null);

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

    const handle = getFileHandle(path);
    if (!handle) {
      setResult(null);
      mapRef.current = null;
      return;
    }

    buildAttributionMap(handle, 'text', controller.signal).then(map => {
      if (controller.signal.aborted) return;

      if (map) {
        const byteToChar = buildByteToCharMap(sourceTextRef.current);
        mapRef.current = map;
        setResult({ attributionMap: map, byteToCharMap: byteToChar });
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
        const updatedMap = updateAttributionMap(mapRef.current, handle, 'text');
        const byteToChar = buildByteToCharMap(sourceText);
        mapRef.current = updatedMap;
        setResult({ attributionMap: updatedMap, byteToCharMap: byteToChar });
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
