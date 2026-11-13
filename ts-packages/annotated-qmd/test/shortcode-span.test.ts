/**
 * Shortcodes reach the JSON as a `Span` with class `quarto-shortcode__`, built
 * by the writer without an `a` (attribute source) field — the attribute is
 * synthesized, so no source text backs it (`crates/pampa/src/writers/json.rs`,
 * `Inline::Shortcode`). bd-htza1cii: conversion must not crash on it.
 */

import { test } from 'node:test';
import { strict as assert } from 'node:assert';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { parseRustQmdDocument } from '../src/index.js';
import type { AnnotatedParse, RustQmdJson } from '../src/types.js';

function load(): RustQmdJson {
  const json = JSON.parse(
    readFileSync(resolve(process.cwd(), 'examples/shortcode.json'), 'utf-8')
  ) as RustQmdJson;
  const qmd = readFileSync(resolve(process.cwd(), 'examples/shortcode.qmd'), 'utf-8');
  for (const file of json.astContext.files) file.content = qmd;
  return json;
}

function find(ap: AnnotatedParse, pred: (a: AnnotatedParse) => boolean): AnnotatedParse[] {
  const out = pred(ap) ? [ap] : [];
  for (const c of ap.components) out.push(...find(c, pred));
  return out;
}

test('a shortcode Span without an attribute source converts, with no attr components', () => {
  const doc = parseRustQmdDocument(load());
  const shortcodes = find(
    doc,
    (a) =>
      a.kind === 'Span' &&
      Array.isArray(a.result) &&
      (a.result as unknown as [[string, string[]]])[0][1].includes('quarto-shortcode__')
  );
  assert.equal(shortcodes.length, 2);
  const inline = shortcodes[0];
  assert.equal(inline.source.value.substring(inline.start, inline.end), '{{< meta title >}}');
  assert.equal(inline.components.filter((c) => c.kind.startsWith('attr-')).length, 0);
});

test('a regular Span next to it keeps its attribute components', () => {
  const doc = parseRustQmdDocument(load());
  const classes = find(doc, (a) => a.kind === 'attr-class').map((a) => a.result);
  assert.ok(classes.includes('note'), `attr-class components: ${JSON.stringify(classes)}`);
});
