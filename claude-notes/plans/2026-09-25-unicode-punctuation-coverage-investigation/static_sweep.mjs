// Static sweep: which single non-ASCII code points can START a pandoc_str token?
// Reads the compiled pandoc_str PATTERN from grammar.json and tests each
// assigned code point with JS's Unicode regex engine. tree-sitter's own
// Unicode tables may differ slightly by version; confirm with dynamic_sweep.
import fs from 'node:fs';
const g = JSON.parse(fs.readFileSync(new URL('../../../crates/tree-sitter-qmd/tree-sitter-markdown/src/grammar.json', import.meta.url)));
const pats = g.rules.pandoc_str.members.filter(m => m.type === 'PATTERN').map(m => new RegExp('^(?:' + m.value + ')', 'u'));
const cat = (c) => {
  for (const k of ['Lu','Ll','Lt','Lm','Lo','Mn','Mc','Me','Nd','Nl','No','Pc','Pd','Ps','Pe','Pi','Pf','Po','Sm','Sc','Sk','So','Zs','Zl','Zp','Cc','Cf','Cs','Co','Cn'])
    if (new RegExp(`^\\p{${k}}$`, 'u').test(c)) return k;
  return '??';
};
const misses = {};
for (let cp = 0x80; cp <= 0x10FFFF; cp++) {
  if (cp >= 0xD800 && cp <= 0xDFFF) continue;
  const c = String.fromCodePoint(cp);
  const k = cat(c);
  if (['Cn','Co','Cs','Cc'].includes(k)) continue;
  if (pats.some(p => p.test(c))) continue;
  (misses[k] ??= []).push(cp);
}
const hex = cp => 'U+' + cp.toString(16).toUpperCase().padStart(4, '0');
for (const [k, cps] of Object.entries(misses)) {
  console.log(`## ${k}: ${cps.length} code points rejected`);
  console.log(cps.map(cp => `${hex(cp)} ${String.fromCodePoint(cp)}`).join('  '));
  console.log();
}
