/**
 * Validates every shared fixture against the row schemas in
 * claude-notes/designs/git-epoch-registry-contract.md (section "Row schemas").
 * The Rust crate (GE-2) and this package (GE-5) read the same files.
 */
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as A from '@automerge/automerge';
import { describe, expect, it } from 'vitest';

const FIXTURES = join(dirname(fileURLToPath(import.meta.url)), '..', 'fixtures');
const readJson = (rel: string): any => JSON.parse(readFileSync(join(FIXTURES, rel), 'utf8'));

// The contract's vocabulary tables, copied here as the checked source.
const EVENTS = [
  'flipped',
  'switched',
  'detached',
  'reattached',
  'held',
  'reset',
  'retired',
  'moved-forward',
  'joined',
  'seeded',
];
const SWAP_EVENTS = ['pending', 'done', 'unavailable'];

function expectUniqueIds(rows: any[]) {
  const ids = rows.map((r) => r.id);
  for (const id of ids) expect(typeof id, 'row without a string id').toBe('string');
  expect(new Set(ids).size, 'duplicate ids').toBe(ids.length);
}

describe('decision tables', () => {
  it('classify rows have the contract fields', () => {
    const rows = readJson('decision-tables/classify.json');
    expect(rows.length).toBeGreaterThan(0);
    expectUniqueIds(rows);
    for (const r of rows) {
      expect(['hub', 'extension'], r.id).toContain(r.peer);
      expect(r.reflog === null || Array.isArray(r.reflog), r.id).toBe(true);
      expect(Array.isArray(r.changed), r.id).toBe(true);
      expect(['commit', 'checkout'], r.id).toContain(r.expected);
    }
  });

  it('arrival rows have a peer, a policy for the hub, and an expected step', () => {
    const rows = readJson('decision-tables/arrival.json');
    expect(rows.length).toBeGreaterThan(0);
    expectUniqueIds(rows);
    for (const r of rows) {
      expect(['hub', 'extension'], r.id).toContain(r.peer);
      if (r.peer === 'hub') expect(['ReadOnly', 'WriteBack'], r.id).toContain(r.policy);
      expect(['text', 'binary'], r.id).toContain(r.kind);
      expect(['none', 'canonical', 'other'], r.id).toContain(r.attached);
      expect(['1', '2a', '2b', '3', '4', '5'], r.id).toContain(r.expected?.step);
      expect(typeof r.expected?.action, r.id).toBe('string');
    }
  });

  it('startup rows cover the three cases', () => {
    const rows = readJson('decision-tables/startup.json');
    expectUniqueIds(rows);
    expect(new Set(rows.map((r: any) => r.expected))).toEqual(
      new Set(['reconcile', 'arrive-differing', 'arrive-all']),
    );
  });

  it('registry rows have an op and an expected value', () => {
    const rows = readJson('decision-tables/registry.json');
    expect(rows.length).toBeGreaterThan(0);
    expectUniqueIds(rows);
    for (const r of rows) {
      expect(typeof r.op, r.id).toBe('string');
      expect(Array.isArray(r.entries), r.id).toBe(true);
      expect(r.expected, r.id).toBeDefined();
    }
  });
});

describe('scenarios', () => {
  const dir = join(FIXTURES, 'scenarios');
  const files = existsSync(dir) ? readdirSync(dir).filter((f) => f.endsWith('.json')) : [];
  it('has scenarios', () => expect(files.length).toBeGreaterThan(0));
  for (const f of files) {
    it(`${f} is a list of git, write, remove, and writeBinary steps`, () => {
      const s = readJson(`scenarios/${f}`);
      expect(typeof s.id).toBe('string');
      expect(s.steps.length).toBeGreaterThan(0);
      for (const step of s.steps) {
        const kinds = ['git', 'write', 'remove', 'writeBinary'].filter((k) => k in step);
        expect(kinds.length, JSON.stringify(step)).toBe(1);
        if (step.expect) expect(['commit', 'checkout', 'none']).toContain(step.expect.verdict);
      }
    });
  }
});

describe('vocabulary', () => {
  it('matches the contract tables', () => {
    const v = readJson('vocabulary.json');
    expect(v.events).toEqual(EVENTS);
    expect(v.swapEvents).toEqual(SWAP_EVENTS);
  });
});

const plain = (v: any): any => {
  if (A.isImmutableString(v)) return v.toString();
  if (Array.isArray(v)) return v.map(plain);
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, plain(x)]));
  return v;
};

describe('golden documents', () => {
  it('each binary has a manifest entry', () => {
    const manifest = readJson('golden/manifest.json');
    const bins = readdirSync(join(FIXTURES, 'golden')).filter((f) => f.endsWith('.bin'));
    expect(bins.length).toBeGreaterThan(0);
    expect(Object.keys(manifest).sort()).toEqual(bins.sort());
  });

  it('every binary decodes to the entries in the manifest', () => {
    const manifest = readJson('golden/manifest.json');
    for (const [file, expected] of Object.entries<any>(manifest)) {
      const doc: any = A.load(readFileSync(join(FIXTURES, 'golden', file)));
      if (expected.index) {
        expect(plain(doc.epochs), file).toBe(expected.index.epochs);
        expect(plain(doc.owner), file).toBe(expected.index.owner);
        expect(plain(doc.repo), file).toEqual(expected.index.repo);
        expect(doc.version, file).toBe(4);
      } else {
        const dump = (m: string) =>
          Object.keys(doc[m]).sort().map((key) => ({ key, gen: doc[m][key] }));
        expect({ entries: dump('entries'), retired: dump('retired') }, file).toEqual(expected);
      }
    }
  });

  it('the two index forms differ in storage: Text object versus scalar string', () => {
    const t: any = A.load(readFileSync(join(FIXTURES, 'golden', 'index-v4-text-form.bin')));
    const s: any = A.load(readFileSync(join(FIXTURES, 'golden', 'index-v4-scalar-form.bin')));
    expect(A.isImmutableString(t.epochs)).toBe(false); // Text object, read as a string
    expect(A.isImmutableString(s.epochs)).toBe(true); // scalar string
  });
});
