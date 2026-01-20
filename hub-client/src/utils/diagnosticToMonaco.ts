/**
 * Convert Quarto diagnostics to Monaco editor markers.
 *
 * This module provides utilities for converting structured diagnostic information
 * from the WASM rendering layer to Monaco editor markers (squiggly underlines).
 *
 * Supports both:
 * - Legacy format (types/diagnostic.ts): snake_case, 1-based positions
 * - New format (types/intelligence.ts): camelCase, 0-based positions (LSP spec)
 */

import type * as Monaco from 'monaco-editor';
import type { Diagnostic as LegacyDiagnostic, DiagnosticKind } from '../types/diagnostic';
import type {
  Diagnostic as LspDiagnostic,
  DiagnosticSeverity,
} from '../types/intelligence';

// Re-export legacy type with alias for backwards compatibility
export type { Diagnostic as LegacyDiagnostic } from '../types/diagnostic';

/**
 * Result of converting diagnostics to Monaco markers.
 *
 * Diagnostics without source locations cannot become inline markers,
 * so they are returned separately for banner/notification display.
 */
export interface DiagnosticsResult<T = LegacyDiagnostic> {
  /** Markers that can be shown inline in the editor. */
  markers: Monaco.editor.IMarkerData[];
  /** Diagnostics without locations, for banner display. */
  unlocatedDiagnostics: T[];
}

/**
 * Convert a diagnostic kind to Monaco marker severity.
 */
function kindToSeverity(kind: DiagnosticKind): Monaco.MarkerSeverity {
  // Monaco.MarkerSeverity values:
  // Hint = 1, Info = 2, Warning = 4, Error = 8
  switch (kind) {
    case 'error':
      return 8; // MarkerSeverity.Error
    case 'warning':
      return 4; // MarkerSeverity.Warning
    case 'info':
      return 2; // MarkerSeverity.Info
    case 'note':
      return 1; // MarkerSeverity.Hint
    default:
      return 2; // Default to Info
  }
}

/**
 * Format a diagnostic message for display in Monaco hover (legacy format).
 *
 * Merges the title, problem, details, and hints into a single message.
 * Per design decision, details are merged into the main message rather than
 * shown as separate markers.
 */
function formatMessage(diag: LegacyDiagnostic): string {
  let msg = diag.title;

  if (diag.problem) {
    msg += '\n' + diag.problem;
  }

  // Merge details into the message (per design decision #3)
  if (diag.details.length > 0) {
    msg +=
      '\n\nDetails:\n' + diag.details.map((d) => '  \u2022 ' + d.content).join('\n');
  }

  if (diag.hints.length > 0) {
    msg +=
      '\n\nSuggestions:\n' + diag.hints.map((h) => '  \u2192 ' + h).join('\n');
  }

  return msg;
}

/**
 * Convert an array of legacy diagnostics to Monaco markers and unlocated diagnostics.
 *
 * @param diagnostics - Array of legacy diagnostics from the WASM layer (1-based positions)
 * @returns Object with markers for Monaco and unlocated diagnostics for banner display
 * @deprecated Use lspDiagnosticsToMarkers() for new code
 */
export function diagnosticsToMarkers(
  diagnostics: LegacyDiagnostic[]
): DiagnosticsResult<LegacyDiagnostic> {
  const markers: Monaco.editor.IMarkerData[] = [];
  const unlocatedDiagnostics: LegacyDiagnostic[] = [];

  for (const diag of diagnostics) {
    // Main diagnostic location
    if (diag.start_line != null) {
      markers.push({
        severity: kindToSeverity(diag.kind),
        message: formatMessage(diag),
        startLineNumber: diag.start_line,
        startColumn: diag.start_column ?? 1,
        endLineNumber: diag.end_line ?? diag.start_line,
        endColumn: diag.end_column ?? 1000, // End of line if not specified
        code: diag.code ?? undefined,
        source: 'quarto',
      });
    } else {
      // No location - collect for banner display (per design decision #2)
      unlocatedDiagnostics.push(diag);
    }
  }

  return { markers, unlocatedDiagnostics };
}

// ============================================================================
// New LSP Diagnostic Format Support
// ============================================================================

/**
 * Convert LSP diagnostic severity to Monaco marker severity.
 */
function lspSeverityToMonaco(severity: DiagnosticSeverity): Monaco.MarkerSeverity {
  // Monaco.MarkerSeverity values:
  // Hint = 1, Info = 2, Warning = 4, Error = 8
  switch (severity) {
    case 'error':
      return 8; // MarkerSeverity.Error
    case 'warning':
      return 4; // MarkerSeverity.Warning
    case 'information':
      return 2; // MarkerSeverity.Info
    case 'hint':
      return 1; // MarkerSeverity.Hint
    default:
      return 2; // Default to Info
  }
}

/**
 * Format an LSP diagnostic message for display in Monaco hover.
 *
 * Merges the title, problem, details, and hints into a single message.
 */
function formatLspMessage(diag: LspDiagnostic): string {
  let msg = diag.title;

  if (diag.problem) {
    msg += '\n' + diag.problem.content;
  }

  // Merge details into the message
  if (diag.details.length > 0) {
    msg +=
      '\n\nDetails:\n' + diag.details.map((d) => '  \u2022 ' + d.content.content).join('\n');
  }

  if (diag.hints.length > 0) {
    msg +=
      '\n\nSuggestions:\n' + diag.hints.map((h) => '  \u2192 ' + h.content).join('\n');
  }

  return msg;
}

/**
 * Convert an array of LSP diagnostics to Monaco markers.
 *
 * This function handles the new LSP format with:
 * - Nested Range/Position objects
 * - camelCase field names
 * - 0-based line/character positions (converted to 1-based for Monaco)
 *
 * @param diagnostics - Array of LSP diagnostics (0-based positions)
 * @returns Object with markers for Monaco and unlocated diagnostics for banner display
 */
export function lspDiagnosticsToMarkers(
  diagnostics: LspDiagnostic[]
): DiagnosticsResult<LspDiagnostic> {
  const markers: Monaco.editor.IMarkerData[] = [];
  const unlocatedDiagnostics: LspDiagnostic[] = [];

  for (const diag of diagnostics) {
    // Check if diagnostic has a valid range
    const hasLocation =
      diag.range &&
      diag.range.start &&
      typeof diag.range.start.line === 'number';

    if (hasLocation) {
      // Convert from 0-based (LSP) to 1-based (Monaco)
      markers.push({
        severity: lspSeverityToMonaco(diag.severity),
        message: formatLspMessage(diag),
        startLineNumber: diag.range.start.line + 1,
        startColumn: diag.range.start.character + 1,
        endLineNumber: diag.range.end.line + 1,
        endColumn: diag.range.end.character + 1 || 1000, // End of line if 0
        code: diag.code ?? undefined,
        source: diag.source ?? 'quarto',
      });
    } else {
      // No location - collect for banner display
      unlocatedDiagnostics.push(diag);
    }
  }

  return { markers, unlocatedDiagnostics };
}
