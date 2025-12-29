/**
 * Convert Quarto diagnostics to Monaco editor markers.
 *
 * This module provides utilities for converting structured diagnostic information
 * from the WASM rendering layer to Monaco editor markers (squiggly underlines).
 */

import type * as Monaco from 'monaco-editor';
import type { Diagnostic, DiagnosticKind } from '../types/diagnostic';

/**
 * Result of converting diagnostics to Monaco markers.
 *
 * Diagnostics without source locations cannot become inline markers,
 * so they are returned separately for banner/notification display.
 */
export interface DiagnosticsResult {
  /** Markers that can be shown inline in the editor. */
  markers: Monaco.editor.IMarkerData[];
  /** Diagnostics without locations, for banner display. */
  unlocatedDiagnostics: Diagnostic[];
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
 * Format a diagnostic message for display in Monaco hover.
 *
 * Merges the title, problem, details, and hints into a single message.
 * Per design decision, details are merged into the main message rather than
 * shown as separate markers.
 */
function formatMessage(diag: Diagnostic): string {
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
 * Convert an array of diagnostics to Monaco markers and unlocated diagnostics.
 *
 * @param diagnostics - Array of diagnostics from the WASM layer
 * @returns Object with markers for Monaco and unlocated diagnostics for banner display
 */
export function diagnosticsToMarkers(
  diagnostics: Diagnostic[]
): DiagnosticsResult {
  const markers: Monaco.editor.IMarkerData[] = [];
  const unlocatedDiagnostics: Diagnostic[] = [];

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
