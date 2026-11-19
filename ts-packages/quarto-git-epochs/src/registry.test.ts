import * as A from '@automerge/automerge';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { FIXTURES, readJson } from './fixture-io.js';
import {
  type Entry,
  type Sidecar,
  asString,
  caseCollisions,
  canonical,
  createSidecar,
  entriesFor,
  liveElsewhere,
  moveForward,
  nextGeneration,
  parseKey,
  readEntries,
  readIndexFields,
  readRetired,
  register,
  registryKey,
  repair,
  retire,
} from './registry.js';
import { EVENTS, SWAP_EVENTS } from './vocabulary.js';

const rows: any[] = readJson('decision-tables/registry.json');
const manifest = readJson('golden/manifest.json');
const actor = (c: string) => c.repeat(32);

function build(entries: Entry[], retired: Entry[]): A.Doc<Sidecar> {
  let d = createSidecar();
  for (const e of entries) d = register(d, e);
  for (const e of retired) d = A.change(d, (x) => { x.retired[registryKey(e.path, e.blob, e.doc)] = new A.Int(e.gen) as unknown as number; });
  return d;
}
const view = (d: A.Doc<Sidecar>) => ({ entries: readEntries(d), retired: readRetired(d) });
const sortE = (es: Entry[]) => [...es].sort((a, b) => registryKey(a.path, a.blob, a.doc).localeCompare(registryKey(b.path, b.blob, b.doc)));

describe('registry: shared fixture rows', () => {
  for (const r of rows) {
    it(`${r.id} (${r.op})`, () => {
      const base = build(r.entries, r.retired);
      switch (r.op) {
        case 'canonical': {
          expect(canonical(view(base), r.args.path, r.args.blob)?.doc ?? null).toBe(r.expected.doc);
          break;
        }
        case 'create': {
          expect(nextGeneration(view(base), r.args.path)).toBe(r.expected.gen);
          break;
        }
        case 'moveForward': {
          const out = moveForward(base, r.args);
          expect(sortE(readEntries(out))).toEqual(sortE(r.expected.entries));
          break;
        }
        case 'union': {
          let [x, y] = [A.clone(base, { actor: actor('a') }), A.clone(base, { actor: actor('b') })];
          x = r.args.sides[0].reduce((d: A.Doc<Sidecar>, e: Entry) => register(d, e), x);
          y = r.args.sides[1].reduce((d: A.Doc<Sidecar>, e: Entry) => register(d, e), y);
          const keys = readEntries(A.merge(A.clone(x), y)).map((e) => [e.path, e.blob, e.doc]);
          expect(keys.sort()).toEqual([...r.expected.keys].sort());
          break;
        }
        case 'putDelete': {
          const [path, blob, doc] = r.args.key;
          const k = registryKey(path, blob, doc);
          let x = A.clone(base, { actor: actor('a') });
          let y = A.clone(base, { actor: actor('b') });
          x = A.change(x, (d) => { delete d.entries[k]; });
          y = A.change(y, (d) => { d.entries[k] = new A.Int(r.args.putGen) as unknown as number; });
          expect(sortE(readEntries(A.merge(A.clone(x), y)))).toEqual(sortE(r.expected.entries));
          expect(sortE(readEntries(A.merge(A.clone(y), x)))).toEqual(sortE(r.expected.entries));
          break;
        }
        case 'retire': {
          const out = retire(base, r.args);
          expect(sortE(readEntries(out))).toEqual(sortE(r.expected.entries));
          expect(sortE(readRetired(out))).toEqual(sortE(r.expected.retired));
          break;
        }
        case 'repair': {
          const out = repair(base, r.args.attached, r.args.headBlob);
          expect(sortE(readEntries(out))).toEqual(sortE(r.expected.entries));
          break;
        }
        default:
          throw new Error(`registry row ${r.id}: unknown op ${r.op}`);
      }
    });
  }
});

describe('registry: operations', () => {
  it('keys are path NUL blob NUL doc and round-trip', () => {
    const k = registryKey('a/b.qmd', 'b1', 'doc');
    expect(k).toBe('a/b.qmd\u0000b1\u0000doc');
    expect(parseKey(k)).toEqual({ path: 'a/b.qmd', blob: 'b1', doc: 'doc' });
  });
  it('a path with backslashes is rejected: keys always use /', () => {
    expect(() => registryKey('a\\b.qmd', 'b1', 'd')).toThrow(/forward slashes/);
  });
  it('register is put-if-absent and returns the same doc when nothing changes', () => {
    const d = register(createSidecar(), { path: 'a', blob: 'b', doc: 'd', gen: 0 });
    expect(register(d, { path: 'a', blob: 'b', doc: 'd', gen: 7 })).toBe(d);
    expect(readEntries(d)[0].gen).toBe(0);
  });
  it('moveForward is a single Automerge change', () => {
    const d = register(createSidecar(), { path: 'a', blob: 'b1', doc: 'd', gen: 2 });
    const out = moveForward(d, { path: 'a', oldBlob: 'b1', newBlob: 'b2', doc: 'd' });
    expect(A.getChangesSince(out, A.getHeads(d)).length).toBe(1);
  });
  it('retire never removes anything, and canonical then skips the doc', () => {
    const d = retire(register(createSidecar(), { path: 'a', blob: 'b', doc: 'd', gen: 0 }), { path: 'a', blob: 'b', doc: 'd' });
    expect(readEntries(d)).toHaveLength(1);
    expect(canonical(view(d), 'a', 'b')).toBeNull();
  });
  it('entriesFor returns all blobs of a path', () => {
    const d = build([{ path: 'a', blob: '1', doc: 'x', gen: 0 }, { path: 'a', blob: '2', doc: 'y', gen: 0 }, { path: 'b', blob: '1', doc: 'z', gen: 0 }], []);
    expect(entriesFor(view(d), 'a')).toHaveLength(2);
  });
  it('caseCollisions finds paths equal when lowercased, and only those', () => {
    const d = build([{ path: 'A.qmd', blob: '1', doc: 'x', gen: 0 }, { path: 'a.qmd', blob: '1', doc: 'y', gen: 0 }, { path: 'c.qmd', blob: '1', doc: 'z', gen: 0 }], []);
    expect([...caseCollisions(view(d))].sort()).toEqual(['A.qmd', 'a.qmd']);
  });
  it('liveElsewhere reports a doc registered under another path', () => {
    const d = build([{ path: 'a', blob: '1', doc: 'x', gen: 0 }], []);
    expect(liveElsewhere(view(d), 'x', 'b')).toBe(true);
    expect(liveElsewhere(view(d), 'x', 'a')).toBe(false);
  });
  it('the sidecar is created with both maps in its first change', () => {
    const d = createSidecar();
    expect(d.version).toBe(1);
    expect(A.getAllChanges(d)).toHaveLength(1);
  });
});

describe('registry: golden documents', () => {
  const sidecars = Object.entries<any>(manifest).filter(([, v]) => v.entries);
  it('reads the logical entries of every sidecar golden document', () => {
    expect(sidecars.length).toBeGreaterThan(0);
    for (const [file, expected] of sidecars) {
      const d = A.load<Sidecar>(readFileSync(join(FIXTURES, 'golden', file)));
      const dump = (es: Entry[]) => es.map((e) => ({ key: registryKey(e.path, e.blob, e.doc), gen: e.gen })).sort((a, b) => a.key.localeCompare(b.key));
      expect({ entries: dump(readEntries(d)), retired: dump(readRetired(d)) }, file).toEqual({
        entries: [...expected.entries].sort((a: any, b: any) => a.key.localeCompare(b.key)),
        retired: [...expected.retired].sort((a: any, b: any) => a.key.localeCompare(b.key)),
      });
    }
  });
  const load = (f: string) => A.load<Sidecar>(readFileSync(join(FIXTURES, 'golden', f)));
  const asEntries = (f: string) => readEntries(load(f));
  it('move forward applied to the before doc equals the after doc', () => {
    const out = moveForward(load('move-forward-before.bin'), { path: 'a.qmd', oldBlob: 'b1', newBlob: 'b2', doc: 'docA' });
    expect(sortE(readEntries(out))).toEqual(sortE(asEntries('move-forward-after.bin')));
  });
  it('retire applied equals the retire doc', () => {
    const out = retire(build([{ path: 'a.qmd', blob: 'b1', doc: 'docA', gen: 0 }], []), { path: 'a.qmd', blob: 'b1', doc: 'docA' });
    expect(sortE(readRetired(out))).toEqual(sortE(readRetired(load('retire.bin'))));
  });
  it('repair applied to an empty registry equals the repair doc', () => {
    const out = repair(createSidecar(), { path: 'a.qmd', doc: 'docA', gen: 1 }, 'b1');
    expect(sortE(readEntries(out))).toEqual(sortE(asEntries('repair.bin')));
  });
  it('reads the epochs pointer in both the Text and the scalar form', () => {
    for (const f of ['index-v4-text-form.bin', 'index-v4-scalar-form.bin']) {
      const d: any = A.load(readFileSync(join(FIXTURES, 'golden', f)));
      expect(readIndexFields(d), f).toEqual(manifest[f].index);
    }
    expect(asString(new A.ImmutableString('x'))).toBe('x');
    expect(asString('y')).toBe('y');
  });
  it('the doc this package writes is committed as a golden fixture and decodes to what the operations produce', () => {
    const file = 'ge05-registry-ops.bin';
    let d = createSidecar();
    d = register(d, { path: 'a.qmd', blob: 'b1', doc: 'docA', gen: 0 });
    d = register(d, { path: 'b.qmd', blob: 'c1', doc: 'docB', gen: 0 });
    d = moveForward(d, { path: 'a.qmd', oldBlob: 'b1', newBlob: 'b2', doc: 'docA' });
    d = retire(d, { path: 'b.qmd', blob: 'c1', doc: 'docB' });
    d = register(d, { path: 'b.qmd', blob: 'c1', doc: 'docC', gen: nextGeneration(view(d), 'b.qmd') });
    const p = join(FIXTURES, 'golden', file);
    if (process.env.UPDATE_GOLDEN === '1') {
      writeFileSync(p, A.save(d));
      const dump = (es: Entry[]) => es.map((e) => ({ key: registryKey(e.path, e.blob, e.doc), gen: e.gen })).sort((a, b) => a.key.localeCompare(b.key));
      const m = readJson('golden/manifest.json');
      m[file] = { entries: dump(readEntries(d)), retired: dump(readRetired(d)) };
      writeFileSync(join(FIXTURES, 'golden', 'manifest.json'), JSON.stringify(m, null, 2) + '\n');
    }
    expect(existsSync(p)).toBe(true);
    expect(sortE(readEntries(load(file)))).toEqual(sortE(readEntries(d)));
    expect(sortE(readRetired(load(file)))).toEqual(sortE(readRetired(d)));
  });
});

describe('vocabulary', () => {
  it('the exported event kinds equal fixtures/vocabulary.json', () => {
    const v = readJson('vocabulary.json');
    expect([...EVENTS]).toEqual(v.events);
    expect([...SWAP_EVENTS]).toEqual(v.swapEvents);
  });
});
