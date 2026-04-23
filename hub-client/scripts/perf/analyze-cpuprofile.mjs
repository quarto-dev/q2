#!/usr/bin/env node
// Self-time aggregator for V8 `.cpuprofile` files.
//
// TypeScript counterpart to `crates/perf-harness/scripts/analyze_profile.py`
// — same intent (read a profile, emit a top-N self-time table) but for
// Node's `--cpu-prof` output rather than samply's JSON.
//
// Usage:
//   node analyze-cpuprofile.mjs <path-to-.cpuprofile> [--top N] [--include GLOB ...]
//
// --include adds a custom bucket: any frame whose callFrame.url contains
// the literal substring GLOB is tallied into a bucket named after it. Pass
// multiple times to compare, e.g.:
//   --include /src/services/attribution --include /src/components/Editor
//
// Profile format reference:
//   https://v8.dev/docs/profile (nodes / samples / timeDeltas)

import fs from 'node:fs';
import process from 'node:process';

function parseArgs(argv) {
  const args = { file: null, top: 30, includes: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--top') args.top = Number(argv[++i]);
    else if (a === '--include') args.includes.push(argv[++i]);
    else if (!args.file) args.file = a;
    else throw new Error(`unexpected arg: ${a}`);
  }
  if (!args.file) {
    console.error('usage: analyze-cpuprofile.mjs <profile> [--top N] [--include SUBSTR ...]');
    process.exit(1);
  }
  return args;
}

function labelFor(node) {
  const cf = node.callFrame;
  const name = cf.functionName || '(anonymous)';
  const url = cf.url || '';
  const short = url
    .replace(/^file:\/\//, '')
    .replace(/^.*\/(src|node_modules|dist|scripts)\//, '$1/');
  const loc = url ? `${short}:${(cf.lineNumber ?? 0) + 1}` : '';
  return loc ? `${name}  ${loc}` : name;
}

function defaultBucket(node) {
  const url = node.callFrame.url || '';
  const name = node.callFrame.functionName || '';
  if (!url && (name === '(garbage collector)' || name === '(program)' || name === '(idle)')) {
    return 'runtime';
  }
  if (!url || url.startsWith('node:')) return 'runtime';
  if (url.includes('/node_modules/')) return 'deps';
  return 'app';
}

function bucketFor(node, includes) {
  const url = node.callFrame.url || '';
  for (const sub of includes) if (url.includes(sub)) return sub;
  return defaultBucket(node);
}

const args = parseArgs(process.argv.slice(2));
const profile = JSON.parse(fs.readFileSync(args.file, 'utf8'));
const { nodes, samples, timeDeltas } = profile;
const nodeById = new Map(nodes.map(n => [n.id, n]));

const self = new Map();
let total = 0;
for (let i = 0; i < samples.length; i++) {
  const id = samples[i];
  const dt = timeDeltas[i] || 0;
  self.set(id, (self.get(id) || 0) + dt);
  total += dt;
}

const ranked = [...self.entries()]
  .filter(([id]) => nodeById.has(id))
  .map(([id, t]) => ({ id, t, node: nodeById.get(id) }))
  .sort((a, b) => b.t - a.t);

const pct = t => (100 * t / total).toFixed(1).padStart(5);
const ms = t => (t / 1000).toFixed(1).padStart(7);

console.log(`${args.file}`);
console.log(`samples=${samples.length}  total=${(total / 1000).toFixed(1)} ms`);

console.log(`\nTop ${args.top} self-time frames:`);
console.log('  pct       ms  frame');
for (const e of ranked.slice(0, args.top)) {
  console.log(`${pct(e.t)}% ${ms(e.t)}  ${labelFor(e.node)}`);
}

const buckets = new Map();
for (const e of ranked) {
  const k = bucketFor(e.node, args.includes);
  buckets.set(k, (buckets.get(k) || 0) + e.t);
}
console.log('\nBy origin:');
for (const [k, t] of [...buckets.entries()].sort((a, b) => b[1] - a[1])) {
  console.log(`${pct(t)}% ${ms(t)}  ${k}`);
}

for (const sub of args.includes) {
  const matches = ranked.filter(e => (e.node.callFrame.url || '').includes(sub));
  if (!matches.length) continue;
  console.log(`\nFrames matching "${sub}":`);
  for (const e of matches) console.log(`${pct(e.t)}% ${ms(e.t)}  ${labelFor(e.node)}`);
}
