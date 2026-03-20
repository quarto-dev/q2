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
      identities: { actor1: 'Alice' },
    };
    const changed = migrateIndexDocument(doc);

    expect(changed).toBe(false);
    expect(doc.version).toBe(1);
    expect(doc.identities).toEqual({ actor1: 'Alice' });
  });

  it('initializes identities if version is missing but identities somehow exist', () => {
    // Edge case: identities present but no version
    const doc: IndexDocument = {
      files: {},
      identities: { actor1: 'Bob' },
    };
    const changed = migrateIndexDocument(doc);

    expect(changed).toBe(true);
    expect(doc.version).toBe(CURRENT_SCHEMA_VERSION);
    // identities already existed, not overwritten
    expect(doc.identities).toEqual({ actor1: 'Bob' });
  });
});

describe('setIdentity', () => {
  it('adds a new identity', () => {
    const doc: IndexDocument = {
      files: {},
      version: 1,
      identities: {},
    };
    const changed = setIdentity(doc, 'actor1', 'Alice');

    expect(changed).toBe(true);
    expect(doc.identities!['actor1']).toBe('Alice');
  });

  it('overwrites a changed screen name', () => {
    const doc: IndexDocument = {
      files: {},
      version: 1,
      identities: { actor1: 'Alice' },
    };
    const changed = setIdentity(doc, 'actor1', 'Alicia');

    expect(changed).toBe(true);
    expect(doc.identities!['actor1']).toBe('Alicia');
  });

  it('returns false when screen name is unchanged', () => {
    const doc: IndexDocument = {
      files: {},
      version: 1,
      identities: { actor1: 'Alice' },
    };
    const changed = setIdentity(doc, 'actor1', 'Alice');

    expect(changed).toBe(false);
    expect(doc.identities!['actor1']).toBe('Alice');
  });

  it('initializes identities map if missing', () => {
    const doc: IndexDocument = { files: {} };
    const changed = setIdentity(doc, 'actor1', 'Alice');

    expect(changed).toBe(true);
    expect(doc.identities).toEqual({ actor1: 'Alice' });
  });

  it('leaves other identities untouched', () => {
    const doc: IndexDocument = {
      files: {},
      version: 1,
      identities: { actor1: 'Alice', actor2: 'Bob' },
    };
    setIdentity(doc, 'actor1', 'Alicia');

    expect(doc.identities!['actor1']).toBe('Alicia');
    expect(doc.identities!['actor2']).toBe('Bob');
  });
});
