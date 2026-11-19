// Regenerates fixtures/golden/*.bin and manifest.json:
//   node ts-packages/quarto-git-epochs/scripts/make-golden.mjs
// Each document is a sidecar (contract: "Sidecar doc") built with @automerge/automerge.
import * as A from '@automerge/automerge';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const OUT = join(dirname(fileURLToPath(import.meta.url)), '..', 'fixtures', 'golden');
mkdirSync(OUT, { recursive: true });
const K = (p, b, d) => `${p}\0${b}\0${d}`;
const int = (n) => new A.Int(n);
const actor = (c) => c.repeat(32);
// One writer creates the sidecar, with both maps, in a single change.
const base = () => A.change(A.init({ actor: actor('0') }), { time: 0 }, (d) => {
  d.version = 1; d.entries = {}; d.retired = {};
});
const fork = (d, a) => A.clone(d, { actor: actor(a) });
const put = (d, map, key, gen) => A.change(d, { time: 0 }, (x) => { x[map][key] = int(gen); });
const del = (d, map, key) => A.change(d, { time: 0 }, (x) => { delete x[map][key]; });

const docs = {};
// before and after a move forward
let b = put(base(), 'entries', K('a.qmd', 'b1', 'docA'), 2);
docs['move-forward-before'] = A.clone(b); // clone: later changes must not alter the saved state
let a = put(b, 'entries', K('a.qmd', 'b2', 'docA'), 2);
docs['move-forward-after'] = del(a, 'entries', K('a.qmd', 'b1', 'docA'));
// creation race on one key (two docs for one path and blob), merged
{ const s = base(); let x = fork(s, 'a'), y = fork(s, 'b');
  x = put(x, 'entries', K('a.qmd', 'b1', 'docX'), 0); y = put(y, 'entries', K('a.qmd', 'b1', 'docY'), 0);
  docs['creation-race-one-key'] = A.merge(x, y); }
// creation race under a new path, merged
{ const s = base(); let x = fork(s, 'a'), y = fork(s, 'b');
  x = put(x, 'entries', K('new/p.qmd', 'b1', 'd1'), 0); y = put(y, 'entries', K('new/p.qmd', 'b2', 'd2'), 0);
  docs['creation-race-new-path'] = A.merge(x, y); }
// concurrent put and delete of one key: the put survives
{ const s = put(base(), 'entries', K('p.qmd', 'b1', 'docA'), 0); let x = fork(s, 'a'), y = fork(s, 'b');
  x = del(x, 'entries', K('p.qmd', 'b1', 'docA')); y = put(y, 'entries', K('p.qmd', 'b1', 'docA'), 2);
  docs['concurrent-put-delete'] = A.merge(x, y); }
// retire: the entry stays, the retired set gains it
docs['retire'] = put(put(base(), 'entries', K('a.qmd', 'b1', 'docA'), 0), 'retired', K('a.qmd', 'b1', 'docA'), 0);
// repair: a doc re-registered under its HEAD blob after its entry was lost
{ const s = put(base(), 'entries', K('a.qmd', 'b1', 'docA'), 1);
  docs['repair'] = put(del(s, 'entries', K('a.qmd', 'b1', 'docA')), 'entries', K('a.qmd', 'b1', 'docA'), 1); }

// Version 4 index docs carrying epochs, owner, and repo in both string forms:
// a plain JS string is stored as a Text object (what hub-client writes), an
// ImmutableString as a scalar string (what the Rust hub writes).
const indexDoc = (str) => A.change(A.init({ actor: actor('1') }), { time: 0 }, (d) => {
  d.files = {}; d.version = int(4); d.identities = {}; d.folders = {};
  d.epochs = str('sidecar-doc'); d.owner = str('owner-1');
  d.repo = { roots: [str('abc123'), str('def456')], prefix: str('docs/site') };
});
const INDEX_FIELDS = { epochs: 'sidecar-doc', owner: 'owner-1', repo: { roots: ['abc123', 'def456'], prefix: 'docs/site' } };
const indexDocs = {
  'index-v4-text-form': indexDoc((v) => v),
  'index-v4-scalar-form': indexDoc((v) => new A.ImmutableString(v)),
};

const manifest = {};
for (const [name, doc] of Object.entries(indexDocs)) {
  const file = `${name}.bin`;
  writeFileSync(join(OUT, file), A.save(doc));
  manifest[file] = { index: INDEX_FIELDS };
}
for (const [name, doc] of Object.entries(docs)) {
  const file = `${name}.bin`;
  writeFileSync(join(OUT, file), A.save(doc));
  const dump = (m) => Object.keys(doc[m]).sort().map((key) => ({ key, gen: doc[m][key] }));
  manifest[file] = { entries: dump('entries'), retired: dump('retired') };
}
writeFileSync(join(OUT, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
console.log(`wrote ${Object.keys(docs).length + Object.keys(indexDocs).length} golden documents`);
