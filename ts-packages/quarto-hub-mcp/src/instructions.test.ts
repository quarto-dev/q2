/**
 * Server `instructions` as the operating guide (ERG-6) with the
 * untrusted-content note (ERG-10), bd-zv8u2sxi;
 * claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md.
 *
 * `instructions` is the one text every host injects into the model's
 * context before any tool is called — it must carry the whole operating
 * model, not just share-URL parsing. Living steering text: it describes
 * only tools that exist (Phase 1 surface), and is revised every phase.
 */

import { describe, it, expect } from 'vitest';

import { startInMemoryMcp } from './in-memory-fixture.js';

describe('server instructions (ERG-6, ERG-10)', () => {
  it('reads as an operating guide: workflow, hashes, sync, watching, auth', async () => {
    const f = await startInMemoryMcp();
    try {
      const instructions = f.client.getInstructions() ?? '';
      // The connect → read → edit loop with the artifacts that make it safe.
      expect(instructions).toContain('connect_project');
      expect(instructions).toContain('read_file');
      expect(instructions).toContain('patch_file');
      expect(instructions).toContain('expected_hash');
      expect(instructions).toContain('synced');
      expect(instructions).toContain('wait_for_change');
      // The share-URL semantics stay.
      expect(instructions).toContain('#/share/');
      // When to authenticate.
      expect(instructions).toContain('authenticate');
      // Collaboration etiquette: don't clobber the human's file.
      expect(instructions).toMatch(/human/i);
    } finally {
      await f.close();
    }
  });

  it('carries the untrusted-content note (ERG-10)', async () => {
    const f = await startInMemoryMcp();
    try {
      const instructions = f.client.getInstructions() ?? '';
      expect(instructions).toMatch(/untrusted/i);
    } finally {
      await f.close();
    }
  });

  it('says so when the server runs read-only', async () => {
    const f = await startInMemoryMcp({ readOnly: true });
    try {
      expect(f.client.getInstructions() ?? '').toMatch(/read-only/i);
    } finally {
      await f.close();
    }
  });

  it('does not claim read-only in the default read-write mode', async () => {
    const f = await startInMemoryMcp();
    try {
      expect(f.client.getInstructions() ?? '').not.toMatch(/read-only/i);
    } finally {
      await f.close();
    }
  });
});
