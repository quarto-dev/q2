/**
 * Public index-document getters (quarto-hub-mcp CAP-2/CAP-3 substrate):
 * `getIdentitiesFromIndex` / `getCapturesFromIndex` were closure-local
 * helpers inside createSyncClient; they are now module-level exports so
 * the MCP server can read the identity/capture sidecars out of an index
 * snapshot (`getIndexHandle().doc()`). Pure functions of the document —
 * the tests pin the absent-map and shallow-copy contracts, plus the
 * project-set re-exports the package index must surface.
 */

import { describe, it, expect } from 'vitest';

import type { IndexDocument } from '@quarto/quarto-automerge-schema';

import {
  getCapturesFromIndex,
  getIdentitiesFromIndex,
  projectSetKey,
  readProjectSetDoc,
} from './index.js';

describe('getIdentitiesFromIndex', () => {
  it('returns {} when the identities map is absent (V1 documents)', () => {
    expect(getIdentitiesFromIndex({} as IndexDocument)).toEqual({});
  });

  it('returns a shallow copy of the identities map', () => {
    const identities = { actor1: { name: 'Charlie', color: '#E91E63' } };
    const doc = { identities } as unknown as IndexDocument;
    const out = getIdentitiesFromIndex(doc);
    expect(out).toEqual(identities);
    // A copy, not the live map: mutating the result must not touch the doc.
    out.actor2 = { name: 'Mallory', color: '#000000' };
    expect(Object.keys(identities)).toEqual(['actor1']);
  });
});

describe('getCapturesFromIndex', () => {
  it('returns {} when the captures sidecar is absent (V1 documents)', () => {
    expect(getCapturesFromIndex({} as IndexDocument)).toEqual({});
  });

  it('returns a shallow copy of the captures map', () => {
    const captures = {
      'index.qmd': { captureDocId: 'abc', state: 'error' as const, lastError: 'boom' },
    };
    const doc = { captures } as unknown as IndexDocument;
    const out = getCapturesFromIndex(doc);
    expect(out).toEqual(captures);
    delete out['index.qmd'];
    expect(Object.keys(captures)).toEqual(['index.qmd']);
  });
});

describe('project-set re-exports (CAP-3)', () => {
  it('projectSetKey strips the automerge: prefix', () => {
    expect(projectSetKey('automerge:abc123')).toBe('abc123');
    expect(projectSetKey('abc123')).toBe('abc123');
  });

  it('readProjectSetDoc is exported from the package index', () => {
    expect(typeof readProjectSetDoc).toBe('function');
  });
});
