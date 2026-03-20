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

  it('is a no-op on a V1 doc', () => {
    const doc: IndexDocument = {
      files: { 'index.qmd': 'doc1' },
      version: 1,
      identities: { actor1: { name: 'Alice', color: '#E91E63' } },
    };
    const changed = migrateIndexDocument(doc);

    expect(changed).toBe(false);
    expect(doc.version).toBe(1);
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
