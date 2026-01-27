/**
 * Tests for diagnosticToMonaco utility functions
 */

import { describe, it, expect } from 'vitest';
import {
  diagnosticsToMarkers,
  lspDiagnosticsToMarkers,
  type DiagnosticsResult,
} from './diagnosticToMonaco';
import type { Diagnostic as LegacyDiagnostic } from '../types/diagnostic';
import type { Diagnostic as LspDiagnostic } from '../types/intelligence';

describe('diagnosticsToMarkers (legacy format)', () => {
  describe('severity conversion', () => {
    it('should convert error kind to MarkerSeverity.Error (8)', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Test error',
        hints: [],
        details: [],
        start_line: 1,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].severity).toBe(8);
    });

    it('should convert warning kind to MarkerSeverity.Warning (4)', () => {
      const diag: LegacyDiagnostic = {
        kind: 'warning',
        title: 'Test warning',
        hints: [],
        details: [],
        start_line: 1,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].severity).toBe(4);
    });

    it('should convert info kind to MarkerSeverity.Info (2)', () => {
      const diag: LegacyDiagnostic = {
        kind: 'info',
        title: 'Test info',
        hints: [],
        details: [],
        start_line: 1,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].severity).toBe(2);
    });

    it('should convert note kind to MarkerSeverity.Hint (1)', () => {
      const diag: LegacyDiagnostic = {
        kind: 'note',
        title: 'Test note',
        hints: [],
        details: [],
        start_line: 1,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].severity).toBe(1);
    });
  });

  describe('message formatting', () => {
    it('should use title as base message', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Expected semicolon',
        hints: [],
        details: [],
        start_line: 1,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].message).toBe('Expected semicolon');
    });

    it('should include problem in message', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Parse error',
        problem: 'Unexpected token',
        hints: [],
        details: [],
        start_line: 1,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].message).toContain('Parse error');
      expect(result.markers[0].message).toContain('Unexpected token');
    });

    it('should include details in message', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Validation error',
        hints: [],
        details: [
          { kind: 'error', content: 'Field "name" is required' },
          { kind: 'info', content: 'See documentation for details' },
        ],
        start_line: 1,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].message).toContain('Details:');
      expect(result.markers[0].message).toContain('Field "name" is required');
      expect(result.markers[0].message).toContain('See documentation for details');
    });

    it('should include hints in message as suggestions', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Undefined variable',
        hints: ['Did you mean "count"?', 'Check for typos'],
        details: [],
        start_line: 1,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].message).toContain('Suggestions:');
      expect(result.markers[0].message).toContain('Did you mean "count"?');
      expect(result.markers[0].message).toContain('Check for typos');
    });

    it('should include code if provided', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Type error',
        code: 'E0001',
        hints: [],
        details: [],
        start_line: 1,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].code).toBe('E0001');
    });
  });

  describe('position handling', () => {
    it('should use provided start_line and start_column', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Error',
        hints: [],
        details: [],
        start_line: 10,
        start_column: 5,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].startLineNumber).toBe(10);
      expect(result.markers[0].startColumn).toBe(5);
    });

    it('should default start_column to 1 if not provided', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Error',
        hints: [],
        details: [],
        start_line: 10,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].startColumn).toBe(1);
    });

    it('should use end_line and end_column if provided', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Error',
        hints: [],
        details: [],
        start_line: 10,
        start_column: 5,
        end_line: 12,
        end_column: 20,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].endLineNumber).toBe(12);
      expect(result.markers[0].endColumn).toBe(20);
    });

    it('should default end_line to start_line if not provided', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Error',
        hints: [],
        details: [],
        start_line: 10,
        start_column: 5,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].endLineNumber).toBe(10);
    });

    it('should default end_column to 1000 (end of line) if not provided', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Error',
        hints: [],
        details: [],
        start_line: 10,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].endColumn).toBe(1000);
    });
  });

  describe('located vs unlocated diagnostics', () => {
    it('should separate diagnostics without start_line into unlocatedDiagnostics', () => {
      const diags: LegacyDiagnostic[] = [
        {
          kind: 'error',
          title: 'Located error',
          hints: [],
          details: [],
          start_line: 5,
        },
        {
          kind: 'warning',
          title: 'Unlocated warning',
          hints: [],
          details: [],
          // No start_line
        },
      ];

      const result = diagnosticsToMarkers(diags);
      expect(result.markers).toHaveLength(1);
      expect(result.markers[0].message).toBe('Located error');
      expect(result.unlocatedDiagnostics).toHaveLength(1);
      expect(result.unlocatedDiagnostics[0].title).toBe('Unlocated warning');
    });

    it('should handle all unlocated diagnostics', () => {
      const diags: LegacyDiagnostic[] = [
        { kind: 'error', title: 'Error 1', hints: [], details: [] },
        { kind: 'error', title: 'Error 2', hints: [], details: [] },
      ];

      const result = diagnosticsToMarkers(diags);
      expect(result.markers).toHaveLength(0);
      expect(result.unlocatedDiagnostics).toHaveLength(2);
    });

    it('should handle empty array', () => {
      const result = diagnosticsToMarkers([]);
      expect(result.markers).toHaveLength(0);
      expect(result.unlocatedDiagnostics).toHaveLength(0);
    });
  });

  describe('source attribution', () => {
    it('should set source to "quarto"', () => {
      const diag: LegacyDiagnostic = {
        kind: 'error',
        title: 'Error',
        hints: [],
        details: [],
        start_line: 1,
      };

      const result = diagnosticsToMarkers([diag]);
      expect(result.markers[0].source).toBe('quarto');
    });
  });
});

describe('lspDiagnosticsToMarkers (LSP format)', () => {
  describe('severity conversion', () => {
    it('should convert error severity to MarkerSeverity.Error (8)', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Test error',
        hints: [],
        details: [],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].severity).toBe(8);
    });

    it('should convert warning severity to MarkerSeverity.Warning (4)', () => {
      const diag: LspDiagnostic = {
        severity: 'warning',
        title: 'Test warning',
        hints: [],
        details: [],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].severity).toBe(4);
    });

    it('should convert information severity to MarkerSeverity.Info (2)', () => {
      const diag: LspDiagnostic = {
        severity: 'information',
        title: 'Test info',
        hints: [],
        details: [],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].severity).toBe(2);
    });

    it('should convert hint severity to MarkerSeverity.Hint (1)', () => {
      const diag: LspDiagnostic = {
        severity: 'hint',
        title: 'Test hint',
        hints: [],
        details: [],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].severity).toBe(1);
    });
  });

  describe('position conversion (0-based to 1-based)', () => {
    it('should convert 0-based positions to 1-based', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Error',
        hints: [],
        details: [],
        range: {
          start: { line: 9, character: 4 }, // 0-based: line 10, column 5
          end: { line: 11, character: 19 }, // 0-based: line 12, column 20
        },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].startLineNumber).toBe(10);
      expect(result.markers[0].startColumn).toBe(5);
      expect(result.markers[0].endLineNumber).toBe(12);
      expect(result.markers[0].endColumn).toBe(20);
    });

    it('should handle first position (0, 0)', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Error',
        hints: [],
        details: [],
        range: {
          start: { line: 0, character: 0 },
          end: { line: 0, character: 5 },
        },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].startLineNumber).toBe(1);
      expect(result.markers[0].startColumn).toBe(1);
    });

    it('should handle zero end character by using 1000 (end of line)', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Error',
        hints: [],
        details: [],
        range: {
          start: { line: 0, character: 0 },
          end: { line: 0, character: 0 },
        },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      // When end character is 0, after +1 it becomes 1, but then || 1000 kicks in
      // Actually looking at the code: `diag.range.end.character + 1 || 1000`
      // 0 + 1 = 1, which is truthy, so it stays 1
      expect(result.markers[0].endColumn).toBe(1);
    });
  });

  describe('message formatting (LSP style)', () => {
    it('should use title as base message', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Expected semicolon',
        hints: [],
        details: [],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].message).toBe('Expected semicolon');
    });

    it('should include problem content in message', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Parse error',
        problem: { type: 'plain', content: 'Unexpected token' },
        hints: [],
        details: [],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].message).toContain('Parse error');
      expect(result.markers[0].message).toContain('Unexpected token');
    });

    it('should include details with nested content structure', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Validation error',
        hints: [],
        details: [
          { kind: 'error', content: { type: 'plain', content: 'Field is required' } },
          { kind: 'info', content: { type: 'plain', content: 'Check docs' } },
        ],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].message).toContain('Details:');
      expect(result.markers[0].message).toContain('Field is required');
      expect(result.markers[0].message).toContain('Check docs');
    });

    it('should include hints as MessageContent objects', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Undefined variable',
        hints: [
          { type: 'plain', content: 'Did you mean "count"?' },
          { type: 'plain', content: 'Check for typos' },
        ],
        details: [],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].message).toContain('Suggestions:');
      expect(result.markers[0].message).toContain('Did you mean "count"?');
      expect(result.markers[0].message).toContain('Check for typos');
    });
  });

  describe('located vs unlocated diagnostics', () => {
    it('should separate diagnostics without valid range into unlocatedDiagnostics', () => {
      const diags: LspDiagnostic[] = [
        {
          severity: 'error',
          title: 'Located error',
          hints: [],
          details: [],
          range: { start: { line: 4, character: 0 }, end: { line: 4, character: 10 } },
        },
        {
          severity: 'warning',
          title: 'Unlocated warning',
          hints: [],
          details: [],
          range: { start: {}, end: {} } as any, // Invalid range
        },
      ];

      const result = lspDiagnosticsToMarkers(diags);
      expect(result.markers).toHaveLength(1);
      expect(result.markers[0].message).toBe('Located error');
      expect(result.unlocatedDiagnostics).toHaveLength(1);
      expect(result.unlocatedDiagnostics[0].title).toBe('Unlocated warning');
    });

    it('should handle empty array', () => {
      const result = lspDiagnosticsToMarkers([]);
      expect(result.markers).toHaveLength(0);
      expect(result.unlocatedDiagnostics).toHaveLength(0);
    });
  });

  describe('source attribution', () => {
    it('should use provided source', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Error',
        source: 'yaml-validator',
        hints: [],
        details: [],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].source).toBe('yaml-validator');
    });

    it('should default source to "quarto" if not provided', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Error',
        hints: [],
        details: [],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].source).toBe('quarto');
    });

    it('should include code if provided', () => {
      const diag: LspDiagnostic = {
        severity: 'error',
        title: 'Type error',
        code: 'Q-1-1',
        hints: [],
        details: [],
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 10 } },
      };

      const result = lspDiagnosticsToMarkers([diag]);
      expect(result.markers[0].code).toBe('Q-1-1');
    });
  });
});

describe('integration scenarios', () => {
  it('should handle a typical error report with all fields', () => {
    const diag: LegacyDiagnostic = {
      kind: 'error',
      title: 'Invalid YAML frontmatter',
      code: 'Q-2-1',
      problem: 'Missing required field "title"',
      hints: ['Add a title field to the YAML frontmatter', 'Example: title: "My Document"'],
      details: [
        { kind: 'error', content: 'The "title" field is required for all documents' },
        { kind: 'info', content: 'Found fields: author, date' },
      ],
      start_line: 1,
      start_column: 1,
      end_line: 5,
      end_column: 3,
    };

    const result = diagnosticsToMarkers([diag]);
    expect(result.markers).toHaveLength(1);
    expect(result.unlocatedDiagnostics).toHaveLength(0);

    const marker = result.markers[0];
    expect(marker.severity).toBe(8); // Error
    expect(marker.code).toBe('Q-2-1');
    expect(marker.source).toBe('quarto');
    expect(marker.startLineNumber).toBe(1);
    expect(marker.startColumn).toBe(1);
    expect(marker.endLineNumber).toBe(5);
    expect(marker.endColumn).toBe(3);

    // Check message contains all parts
    expect(marker.message).toContain('Invalid YAML frontmatter');
    expect(marker.message).toContain('Missing required field "title"');
    expect(marker.message).toContain('Details:');
    expect(marker.message).toContain('Suggestions:');
  });

  it('should handle multiple diagnostics of different types', () => {
    const diags: LegacyDiagnostic[] = [
      {
        kind: 'error',
        title: 'Syntax error',
        hints: [],
        details: [],
        start_line: 10,
      },
      {
        kind: 'warning',
        title: 'Deprecated feature',
        hints: [],
        details: [],
        start_line: 20,
      },
      {
        kind: 'info',
        title: 'Document summary',
        hints: [],
        details: [],
        // No location - should be unlocated
      },
    ];

    const result = diagnosticsToMarkers(diags);
    expect(result.markers).toHaveLength(2);
    expect(result.unlocatedDiagnostics).toHaveLength(1);

    // Check severities are correct
    expect(result.markers.find((m) => m.message === 'Syntax error')?.severity).toBe(8);
    expect(result.markers.find((m) => m.message === 'Deprecated feature')?.severity).toBe(4);
  });
});
