import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as A from '@automerge/automerge';
import { describe, expect, it } from 'vitest';
import {
  CURRENT_SCHEMA_VERSION,
  createIndexDocumentContent,
  migrateIndexDocument,
} from '../index.js';
import type { IndexDocument } from '../index.js';

const GOLDEN = join(
  dirname(fileURLToPath(import.meta.url)),
  '..', '..', '..', 'quarto-git-epochs', 'fixtures', 'golden', 'index-v4.json',
);

describe('index schema version 4', () => {
  it('is version 4', () => {
    expect(CURRENT_SCHEMA_VERSION).toBe(4);
  });

  it('migrates a version 3 doc to 4 with no other change', () => {
    const doc: IndexDocument = {
      files: { 'a.qmd': 'doc1' },
      version: 3,
      identities: {},
      folders: { sub: true },
    };
    expect(migrateIndexDocument(doc)).toBe(true);
    expect(doc).toEqual({
      files: { 'a.qmd': 'doc1' },
      version: 4,
      identities: {},
      folders: { sub: true },
    });
  });

  it('leaves a version 4 doc untouched', () => {
    const doc: IndexDocument = { files: {}, version: 4, identities: {}, folders: {} };
    expect(migrateIndexDocument(doc)).toBe(false);
    expect(doc).toEqual({ files: {}, version: 4, identities: {}, folders: {} });
  });

  it('leaves a version 5 doc untouched (an older client leaves a newer doc alone)', () => {
    const doc = { files: {}, version: 5, identities: {}, folders: {}, future: 1 } as IndexDocument;
    expect(migrateIndexDocument(doc)).toBe(false);
    expect(doc.version).toBe(5);
    expect((doc as unknown as Record<string, unknown>).future).toBe(1);
  });

  it('createIndexDocumentContent writes exactly the four keys and equals the golden file', () => {
    const content = createIndexDocumentContent();
    expect(Object.keys(content).sort()).toEqual(['files', 'folders', 'identities', 'version']);
    expect(content.version).toBe(4);
    expect(content).toEqual(JSON.parse(readFileSync(GOLDEN, 'utf8')));
  });

  it('round-trips the epochs, owner, and repo keys through an Automerge change', () => {
    let doc = A.from(createIndexDocumentContent() as unknown as Record<string, unknown>) as A.Doc<IndexDocument>;
    doc = A.change(doc, (d) => {
      d.epochs = 'automerge:sidecar';
      d.owner = 'owner-1';
      d.repo = { roots: ['abc123', 'def456'], prefix: 'docs/site' };
    });
    const loaded = A.load(A.save(doc)) as A.Doc<IndexDocument>;
    expect(loaded.epochs).toBe('automerge:sidecar');
    expect(loaded.owner).toBe('owner-1');
    expect(loaded.repo).toEqual({ roots: ['abc123', 'def456'], prefix: 'docs/site' });
  });
});
