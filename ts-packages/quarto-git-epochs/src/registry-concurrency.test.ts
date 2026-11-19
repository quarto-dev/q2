/**
 * Gate 0 item 6, TypeScript side: the flat registry under two actors.
 * The Rust side lives in `crates/quarto-hub/src/index.rs` (tests).
 * See claude-notes/designs/git-epoch-registry-contract.md, "Sidecar doc".
 */
import * as A from '@automerge/automerge';
import { describe, expect, it } from 'vitest';

type Side = { version: number; entries: Record<string, number>; retired: Record<string, number> };
const K = (p: string, b: string, d: string) => `${p}\0${b}\0${d}`;
const base = (): A.Doc<Side> => A.from<Side>({ version: 1, entries: {}, retired: {} });
const two = (b: A.Doc<Side>) => [A.clone(b, { actor: 'aa'.repeat(16) }), A.clone(b, { actor: 'bb'.repeat(16) })] as const;

describe('flat registry concurrency', () => {
  it('keeps both docs of a creation race and different keys under a new path', () => {
    let [x, y] = two(base());
    x = A.change(x, (d) => { d.entries[K('a.qmd', 'b1', 'docX')] = new A.Int(0) as unknown as number; });
    y = A.change(y, (d) => { d.entries[K('a.qmd', 'b1', 'docY')] = new A.Int(0) as unknown as number; });
    x = A.change(x, (d) => { d.entries[K('new/p.qmd', 'b1', 'd1')] = new A.Int(0) as unknown as number; });
    y = A.change(y, (d) => { d.entries[K('new/p.qmd', 'b2', 'd2')] = new A.Int(0) as unknown as number; });
    expect(Object.keys(A.merge(A.clone(x), y).entries)).toHaveLength(4);
  });

  it('merges an identical key from both sides to one key', () => {
    let [x, y] = two(base());
    x = A.change(x, (d) => { d.entries[K('s', 'b', 'd')] = new A.Int(1) as unknown as number; });
    y = A.change(y, (d) => { d.entries[K('s', 'b', 'd')] = new A.Int(1) as unknown as number; });
    expect(Object.keys(A.merge(A.clone(x), y).entries)).toHaveLength(1);
  });

  it('loses one side in the nested form (why the registry is flat)', () => {
    type Nested = { entries: Record<string, Record<string, number>> };
    const b = A.from<Nested>({ entries: {} });
    let x = A.clone(b, { actor: 'aa'.repeat(16) });
    let y = A.clone(b, { actor: 'bb'.repeat(16) });
    x = A.change(x, (d) => { d.entries['new/p.qmd'] = {}; d.entries['new/p.qmd'].k1 = 0; });
    y = A.change(y, (d) => { d.entries['new/p.qmd'] = {}; d.entries['new/p.qmd'].k2 = 0; });
    expect(Object.keys(A.merge(A.clone(x), y).entries['new/p.qmd'])).toHaveLength(1);
  });

  it('leaves the put after a concurrent delete and put of one key, in both merge orders', () => {
    const k = K('p', 'b', 'd');
    const s = A.change(base(), (d) => { d.entries[k] = new A.Int(0) as unknown as number; });
    let [x, y] = two(s);
    x = A.change(x, (d) => { delete d.entries[k]; });
    y = A.change(y, (d) => { d.entries[k] = new A.Int(2) as unknown as number; });
    expect(A.merge(A.clone(x), y).entries[k]).toBe(2);
    expect(A.merge(A.clone(y), x).entries[k]).toBe(2);
  });

  it('keeps a retired key through a concurrent delete, and NUL keys and ints survive save and load', () => {
    const k = K('p', 'b', 'd');
    const s = A.change(base(), (d) => { d.entries[k] = new A.Int(0) as unknown as number; });
    let [x, y] = two(s);
    x = A.change(x, (d) => { d.retired[k] = new A.Int(5) as unknown as number; });
    y = A.change(y, (d) => { delete d.entries[k]; });
    const loaded = A.load<Side>(A.save(A.merge(A.clone(x), y)));
    expect(Object.keys(loaded.retired)).toEqual([k]);
    expect(loaded.retired[k]).toBe(5);
  });
});
