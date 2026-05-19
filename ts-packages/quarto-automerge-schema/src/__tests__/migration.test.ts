import { describe, it, expect } from 'vitest';
import {
  CURRENT_SCHEMA_VERSION,
  migrateIndexDocument,
  setIdentity,
} from '../index.js';
import type { IndexDocument } from '../index.js';

describe('migrateIndexDocument', () => {
  it('migrates a V0 doc (no version, no identities) to V1', () => {
    const doc: IndexDocument = { files: { 'index.qmd': 'doc1' } };
    const changed = migrateIndexDocument(doc);

    expect(changed).toBe(true);
    expect(doc.version).toBe(CURRENT_SCHEMA_VERSION);
    expect(doc.identities).toEqual({});
    // files are untouched
    expect(doc.files).toEqual({ 'index.qmd': 'doc1' });
  });

  it('migrates a V1 doc forward to the current version', () => {
    // V1 was the schema before the capture sidecar was introduced.
    // Migration must bump it to current without dropping anything.
    const doc: IndexDocument = {
      files: { 'index.qmd': 'doc1' },
      version: 1,
      identities: { actor1: { name: 'Alice', color: '#E91E63' } },
    };
    const changed = migrateIndexDocument(doc);

    expect(changed).toBe(true);
    expect(doc.version).toBe(CURRENT_SCHEMA_VERSION);
    expect(doc.identities).toEqual({ actor1: { name: 'Alice', color: '#E91E63' } });
  });

  it('initializes identities if version is missing but identities somehow exist', () => {
    // Edge case: identities present but no version
    const doc: IndexDocument = {
      files: {},
      identities: { actor1: { name: 'Bob', color: '#4CAF50' } },
    };
    const changed = migrateIndexDocument(doc);

    expect(changed).toBe(true);
    expect(doc.version).toBe(CURRENT_SCHEMA_VERSION);
    // identities already existed, not overwritten
    expect(doc.identities).toEqual({ actor1: { name: 'Bob', color: '#4CAF50' } });
  });
});

describe('migrateIndexDocument — v2 (capture sidecar)', () => {
  it('bumps a V0 doc straight to V2', () => {
    const doc: IndexDocument = { files: { 'index.qmd': 'doc1' } };
    const changed = migrateIndexDocument(doc);

    expect(changed).toBe(true);
    expect(doc.version).toBe(2);
    expect(doc.version).toBe(CURRENT_SCHEMA_VERSION);
    // captures is optional and absent until a capture is recorded
    expect(doc.captures).toBeUndefined();
  });

  it('migrates a V1 doc to V2 without touching files or identities', () => {
    const doc: IndexDocument = {
      files: { 'index.qmd': 'doc1' },
      version: 1,
      identities: { actor1: { name: 'Alice', color: '#E91E63' } },
    };
    const changed = migrateIndexDocument(doc);

    expect(changed).toBe(true);
    expect(doc.version).toBe(2);
    expect(doc.files).toEqual({ 'index.qmd': 'doc1' });
    expect(doc.identities).toEqual({ actor1: { name: 'Alice', color: '#E91E63' } });
    expect(doc.captures).toBeUndefined();
  });

  it('is a no-op on a V2 doc', () => {
    const doc: IndexDocument = {
      files: { 'index.qmd': 'doc1' },
      version: 2,
      identities: {},
      captures: {
        'index.qmd': {
          captureDocId: 'capture-doc-1',
          state: 'idle',
        },
      },
    };
    const changed = migrateIndexDocument(doc);

    expect(changed).toBe(false);
    expect(doc.version).toBe(2);
    expect(doc.captures).toEqual({
      'index.qmd': { captureDocId: 'capture-doc-1', state: 'idle' },
    });
  });

  it('preserves an existing captures sidecar through migration from V1', () => {
    // V1 docs cannot legally have captures, but if a future-V2-written doc
    // is mis-tagged as V1, migration must not drop the sidecar.
    const doc: IndexDocument = {
      files: { 'index.qmd': 'doc1' },
      version: 1,
      captures: { 'index.qmd': { captureDocId: 'cap-1' } },
    };
    const changed = migrateIndexDocument(doc);

    expect(changed).toBe(true);
    expect(doc.version).toBe(2);
    expect(doc.captures).toEqual({ 'index.qmd': { captureDocId: 'cap-1' } });
  });

  it('accepts a CaptureRef with all optional fields populated', () => {
    // Type-level test: the shape compiles and roundtrips.
    const doc: IndexDocument = {
      files: { 'posts/p.qmd': 'doc-p' },
      version: 2,
      captures: {
        'posts/p.qmd': {
          captureDocId: 'cap-p',
          staleness: true,
          state: 'error',
          lastError: 'engine timed out',
        },
      },
    };
    expect(doc.captures!['posts/p.qmd'].lastError).toBe('engine timed out');
    expect(doc.captures!['posts/p.qmd'].state).toBe('error');
  });
});

describe('setIdentity', () => {
  it('adds a new identity', () => {
    const doc: IndexDocument = { files: {}, version: 1, identities: {} };
    const changed = setIdentity(doc, 'actor1', 'Alice', '#E91E63');

    expect(changed).toBe(true);
    expect(doc.identities!['actor1']).toEqual({ name: 'Alice', color: '#E91E63' });
  });

  it('overwrites a changed screen name', () => {
    const doc: IndexDocument = {
      files: {},
      version: 1,
      identities: { actor1: { name: 'Alice', color: '#E91E63' } },
    };
    const changed = setIdentity(doc, 'actor1', 'Alicia', '#E91E63');

    expect(changed).toBe(true);
    expect(doc.identities!['actor1']).toEqual({ name: 'Alicia', color: '#E91E63' });
  });

  it('returns false when identity is unchanged', () => {
    const doc: IndexDocument = {
      files: {},
      version: 1,
      identities: { actor1: { name: 'Alice', color: '#E91E63' } },
    };
    const changed = setIdentity(doc, 'actor1', 'Alice', '#E91E63');

    expect(changed).toBe(false);
  });

  it('initializes identities map if missing', () => {
    const doc: IndexDocument = { files: {} };
    const changed = setIdentity(doc, 'actor1', 'Alice', '#E91E63');

    expect(changed).toBe(true);
    expect(doc.identities).toEqual({ actor1: { name: 'Alice', color: '#E91E63' } });
  });

  it('leaves other identities untouched', () => {
    const doc: IndexDocument = {
      files: {},
      version: 1,
      identities: { actor1: { name: 'Alice', color: '#E91E63' }, actor2: { name: 'Bob', color: '#4CAF50' } },
    };
    setIdentity(doc, 'actor1', 'Alicia', '#E91E63');

    expect(doc.identities!['actor1']).toEqual({ name: 'Alicia', color: '#E91E63' });
    expect(doc.identities!['actor2']).toEqual({ name: 'Bob', color: '#4CAF50' });
  });
});
