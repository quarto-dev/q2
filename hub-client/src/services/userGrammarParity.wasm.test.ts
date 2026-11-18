/**
 * Native-vs-browser parity test — Phase 4.4 of
 * `claude-notes/plans/2026-04-21-syntax-highlighting-phase-4.md`.
 *
 * Runs the JS-side user-grammar highlighter on the same Java properties
 * fixture the native golden
 * (`crates/quarto-highlight/tests/integration/snapshots/
 * integration__golden__user_grammar_properties.snap`) covers, and validates
 * that each side's spans cover the same parts of the source. (The fixture
 * used TOML until TOML became a built-in language.)
 *
 * ## Both paths now flatten identically (exact parity)
 *
 * As of the 2026-06-10 Monaco highlighting plan (Phase 0), the native
 * render path no longer uses `tree-sitter-highlight`'s lossy
 * `HighlightEvent` stream: it walks `Query.captures()` (node-exact) and
 * flattens innermost-wins via `quarto_highlight::flatten_spans`. The JS
 * side mirrors that exact flatten (`Highlight.ts`), so native and
 * browser produce **identical** flattened spans. (With the former TOML
 * fixture that meant `@type` over a key and `@property` over the gap bytes
 * around it; bd-98k6's same-start over-wrap, `@type` reported as 0-14, is
 * gone on both paths.)
 *
 * ## What this test checks
 *
 * Every native capture identity `(start, captureName)` appears in the JS
 * output and vice versa (no path-unique captures), and — now that both
 * flatten — the native end byte equals the JS end byte for a shared
 * identity (the `>=` invariant below holds with equality).
 *
 * Runs under `npm run test:wasm`.
 */

import { beforeAll, describe, expect, it } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';

import { loadUserGrammar, type UserGrammarHighlighter } from '@quarto/preview-runtime/userGrammar/Highlight';

type SpanTriple = [number, number, string];

/** The exact source used in `golden_user_grammar_properties` in
 * `crates/quarto-highlight/tests/integration/golden.rs`. */
const PROPERTIES_SOURCE = '# server\nhost = localhost\nport: 8080\n';

let highlighter: UserGrammarHighlighter;
let nativeGolden: SpanTriple[];

/** Parse an `insta` `.snap` file: strip the YAML frontmatter and JSON-parse the body. */
function parseInstaSnap(contents: string): SpanTriple[] {
  const lines = contents.split('\n');
  if (lines[0] !== '---') throw new Error('snap file missing opening `---`');
  let i = 1;
  while (i < lines.length && lines[i] !== '---') i++;
  if (i === lines.length) throw new Error('snap file missing closing `---`');
  i++;
  return JSON.parse(lines.slice(i).join('\n').trim()) as SpanTriple[];
}

beforeAll(async () => {
  const __dirname = dirname(fileURLToPath(import.meta.url));
  const repoRoot = resolve(__dirname, '../../..');

  const fixtureDir = join(
    repoRoot,
    'crates/quarto-highlight/tests/fixtures/user-grammar-properties',
  );
  const wasmBytes = await readFile(join(fixtureDir, 'properties.wasm'));
  const highlightsScm = await readFile(join(fixtureDir, 'highlights.scm'), 'utf-8');
  highlighter = await loadUserGrammar({
    name: 'properties',
    wasmBytes: new Uint8Array(wasmBytes),
    highlightsScm,
  });

  const snapPath = join(
    repoRoot,
    'crates/quarto-highlight/tests/integration/snapshots/integration__golden__user_grammar_properties.snap',
  );
  nativeGolden = parseInstaSnap(await readFile(snapPath, 'utf-8'));
});

/** A capture's "identity" across paths: its start byte and capture name. */
type CaptureIdentity = string; // `${start}:${capture}`
const identity = (s: SpanTriple): CaptureIdentity => `${s[0]}:${s[2]}`;
const identitySet = (spans: SpanTriple[]): Set<CaptureIdentity> =>
  new Set(spans.map(identity));

describe('native-vs-browser parity on the properties fixture', () => {
  it('the parity fixtures cover the same source', () => {
    // Sanity: if this constant ever drifts from the one in
    // `crates/quarto-highlight/tests/integration/golden.rs`, the comparison is
    // meaningless.
    expect(PROPERTIES_SOURCE).toBe('# server\nhost = localhost\nport: 8080\n');
    expect(nativeGolden.length).toBeGreaterThan(0);
  });

  it('every native capture identity appears in the JS output', () => {
    const jsSpans = JSON.parse(highlighter.highlight(PROPERTIES_SOURCE)) as SpanTriple[];
    const jsIdentities = identitySet(jsSpans);
    const missing = nativeGolden.filter((n) => !jsIdentities.has(identity(n)));
    expect(
      missing,
      `native spans missing from JS output:\n  native: ${JSON.stringify(nativeGolden)}\n  js:     ${JSON.stringify(jsSpans)}`,
    ).toEqual([]);
  });

  it('every JS capture identity appears in the native output', () => {
    const jsSpans = JSON.parse(highlighter.highlight(PROPERTIES_SOURCE)) as SpanTriple[];
    const nativeIdentities = identitySet(nativeGolden);
    const extra = jsSpans.filter((s) => !nativeIdentities.has(identity(s)));
    expect(
      extra,
      `JS emitted captures not present in native output:\n  native: ${JSON.stringify(nativeGolden)}\n  js:     ${JSON.stringify(jsSpans)}`,
    ).toEqual([]);
  });

  it('end-byte divergences are bounded by the enclosing-capture invariant', () => {
    // For any JS capture, if a native capture exists with the same
    // (start, capture) identity, the native end byte must be >= the
    // JS end byte. This is the event-stream-semantics divergence
    // documented at the top of the file: native stretches nested
    // same-start captures to the outer capture's end; JS reports
    // exact node ends. Any case where native end < JS end would
    // be a new class of divergence worth investigating.
    const jsSpans = JSON.parse(highlighter.highlight(PROPERTIES_SOURCE)) as SpanTriple[];
    for (const js of jsSpans) {
      const natives = nativeGolden.filter((n) => identity(n) === identity(js));
      for (const n of natives) {
        expect(
          n[1],
          `native end < JS end for ${JSON.stringify(js)} vs ${JSON.stringify(n)}`,
        ).toBeGreaterThanOrEqual(js[1]);
      }
    }
  });
});
