/**
 * useIntelligence Hook
 *
 * React hook for accessing the intelligence subsystem (document symbols,
 * folding ranges, diagnostics). Provides debounced refresh when document
 * content changes.
 */

import { useState, useEffect, useCallback, useRef } from 'react';
import {
  analyzeDocument,
  type Symbol,
  type Diagnostic,
  type FoldingRange,
  type DocumentAnalysis,
} from '../services/intelligenceService';

/**
 * Options for the useIntelligence hook.
 */
export interface UseIntelligenceOptions {
  /** File path to analyze (null to disable). */
  path: string | null;
  /** Debounce delay in ms. Default: 300 */
  debounceMs?: number;
  /** Whether to fetch symbols. Default: true */
  enableSymbols?: boolean;
  /** Whether to fetch diagnostics. Default: false */
  enableDiagnostics?: boolean;
  /** Whether to fetch folding ranges. Default: false */
  enableFoldingRanges?: boolean;
}

/**
 * Return value from useIntelligence hook.
 */
export interface UseIntelligenceResult {
  /** Document symbols (outline). Empty if enableSymbols is false. */
  symbols: Symbol[];
  /** Diagnostics from LSP analysis. Empty if enableDiagnostics is false. */
  diagnostics: Diagnostic[];
  /** Folding ranges. Empty if enableFoldingRanges is false. */
  foldingRanges: FoldingRange[];
  /** Whether data is currently being loaded. */
  loading: boolean;
  /** Error message if analysis failed. */
  error: string | null;
  /** Force a refresh of the analysis. */
  refresh: () => void;
}

/**
 * Hook for accessing intelligence subsystem data.
 *
 * Automatically refreshes when path changes.
 * Uses debouncing to avoid excessive parsing.
 *
 * @example
 * ```tsx
 * const { symbols, loading, refresh } = useIntelligence({
 *   path: currentFile?.path ?? null,
 *   enableSymbols: true,
 * });
 *
 * // Trigger refresh when content changes
 * useEffect(() => {
 *   refresh();
 * }, [fileContent, refresh]);
 * ```
 */
export function useIntelligence(
  options: UseIntelligenceOptions
): UseIntelligenceResult {
  const {
    path,
    debounceMs = 300,
    enableSymbols = true,
    enableDiagnostics = false,
    enableFoldingRanges = false,
  } = options;

  // State
  const [symbols, setSymbols] = useState<Symbol[]>([]);
  const [diagnostics, setDiagnostics] = useState<Diagnostic[]>([]);
  const [foldingRanges, setFoldingRanges] = useState<FoldingRange[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Track the current path to avoid stale updates
  const currentPathRef = useRef<string | null>(null);

  // Track pending refresh
  const refreshTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const refreshCounterRef = useRef(0);

  /**
   * Perform the actual analysis.
   */
  const doAnalyze = useCallback(async () => {
    if (!path) {
      setSymbols([]);
      setDiagnostics([]);
      setFoldingRanges([]);
      setError(null);
      return;
    }

    // Skip if nothing is enabled
    if (!enableSymbols && !enableDiagnostics && !enableFoldingRanges) {
      return;
    }

    const thisRefresh = ++refreshCounterRef.current;
    currentPathRef.current = path;
    setLoading(true);
    setError(null);

    try {
      // Use the combined analysis function for efficiency
      const analysis: DocumentAnalysis = await analyzeDocument(path);

      // Check if this is still the current request
      if (thisRefresh !== refreshCounterRef.current || currentPathRef.current !== path) {
        return; // Stale response, ignore
      }

      // Update state based on what's enabled
      if (enableSymbols) {
        setSymbols(analysis.symbols);
      }
      if (enableDiagnostics) {
        setDiagnostics(analysis.diagnostics);
      }
      if (enableFoldingRanges) {
        setFoldingRanges(analysis.foldingRanges);
      }
    } catch (err) {
      // Check if this is still the current request
      if (thisRefresh !== refreshCounterRef.current || currentPathRef.current !== path) {
        return;
      }

      const errorMsg = err instanceof Error ? err.message : String(err);
      console.error('Intelligence analysis failed:', errorMsg);
      setError(errorMsg);
      setSymbols([]);
      setDiagnostics([]);
      setFoldingRanges([]);
    } finally {
      // Only clear loading if this is still the current request
      if (thisRefresh === refreshCounterRef.current) {
        setLoading(false);
      }
    }
  }, [path, enableSymbols, enableDiagnostics, enableFoldingRanges]);

  /**
   * Debounced refresh function.
   */
  const refresh = useCallback(() => {
    // Clear any pending refresh
    if (refreshTimeoutRef.current) {
      clearTimeout(refreshTimeoutRef.current);
    }

    // Schedule new refresh
    refreshTimeoutRef.current = setTimeout(() => {
      refreshTimeoutRef.current = null;
      doAnalyze();
    }, debounceMs);
  }, [doAnalyze, debounceMs]);

  /**
   * Immediate refresh (bypasses debounce).
   * Used when path changes.
   */
  const immediateRefresh = useCallback(() => {
    // Clear any pending refresh
    if (refreshTimeoutRef.current) {
      clearTimeout(refreshTimeoutRef.current);
      refreshTimeoutRef.current = null;
    }
    doAnalyze();
  }, [doAnalyze]);

  // Refresh when path changes (immediate, no debounce)
  useEffect(() => {
    immediateRefresh();
  }, [path, immediateRefresh]);

  // Cleanup timeout on unmount
  useEffect(() => {
    return () => {
      if (refreshTimeoutRef.current) {
        clearTimeout(refreshTimeoutRef.current);
      }
    };
  }, []);

  // Clear state when path becomes null
  useEffect(() => {
    if (!path) {
      setSymbols([]);
      setDiagnostics([]);
      setFoldingRanges([]);
      setError(null);
      setLoading(false);
    }
  }, [path]);

  return {
    symbols,
    diagnostics,
    foldingRanges,
    loading,
    error,
    refresh,
  };
}

// Re-export types for convenience
export type { Symbol, Diagnostic, FoldingRange } from '../services/intelligenceService';
