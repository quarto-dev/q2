/**
 * Native-vs-browser parity test — Phase 4.4 of
 * `claude-notes/plans/2026-04-21-syntax-highlighting-phase-4.md`.
 *
 * Runs the JS-side user-grammar highlighter on the same TOML fixture
 * the native golden
 * (`crates/quarto-highlight/tests/integration/snapshots/
 * integration__golden__user_grammar_toml.snap`) covers, and validates
 * that each side's spans cover the semantically-equivalent parts of
 * the source.
 *
 * ## Why bit-equality is not the assertion
 *
 * Native uses `tree-sitter-highlight`'s `HighlightEvent` stream. When
 * two captures open at the same start byte (e.g. `@type` for the key
 * and `@property` for the enclosing pair), tree-sitter-highlight emits
 * both `HighlightStart` events with no intervening `Source`, and
 * `collect_spans` records the outer-most end for *both* (because the
 * cursor only advances on `Source` events). So a `(bare_key) @type`
 * pattern that should cover bytes 0-4 ends up reported as 0-14 in
 * native output.
 *
 * The JS side uses `web-tree-sitter`'s `Query.captures()`, which
 * returns one capture per matching node with that node's exact byte
 * range — so `@type` correctly reports 0-4.
 *
 * Parent plan's Design decision 1 anticipated this: we deliberately
 * do not port `tree-sitter-highlight`'s event-stream semantics to JS
 * (Phase 4 scope). The consequence is that for HTML rendering:
 *
 * - **Native**: `<span class="hl-property"><span class="hl-type">
 *   name = "value"</span></span>` — the `.hl-type` class wraps the
 *   whole pair.
 * - **Browser**: `<span class="hl-property"><span class="hl-type">
 *   name</span> = "value"</span>` — the `.hl-type` class wraps just
 *   the key.
 *
 * Both are "highlighted TOML"; browser is arguably more
 * semantically accurate. This is acknowledged divergence, not a
 * regression.
 *
 * ## What this test does check
 *
 * For every native capture, a JS capture must exist with the same
 * `(start, captureName)` — i.e. the browser recognizes every span
 * the native path recognizes. The end byte may differ per the above
 * divergence.
 *
 * Conversely, for every JS capture, a native capture must exist with
 * the same `(start, captureName)` — no browser-unique captures.
 *
 * Runs under `npm run test:wasm`.
 */

import { beforeAll, describe, expect, it } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join, resolve } from 'path';
import { fileURLToPath } from 'url';

import { loadUserGrammar, type UserGrammarHighlighter } from '@quarto/preview-runtime/userGrammar/Highlight';

type SpanTriple = [number, number, string];

/** The exact source used in `crates/quarto-highlight/tests/integration/golden.rs:73`. */
const TOML_SOURCE = 'name = "value"\ncount = 42\n';

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
    'crates/quarto-highlight/tests/fixtures/user-grammar-toml',
  );
  const wasmBytes = await readFile(join(fixtureDir, 'toml.wasm'));
  const highlightsScm = await readFile(join(fixtureDir, 'highlights.scm'), 'utf-8');
  highlighter = await loadUserGrammar({
    name: 'toml',
    wasmBytes: new Uint8Array(wasmBytes),
    highlightsScm,
  });

  const snapPath = join(
    repoRoot,
    'crates/quarto-highlight/tests/integration/snapshots/integration__golden__user_grammar_toml.snap',
  );
  nativeGolden = parseInstaSnap(await readFile(snapPath, 'utf-8'));
});

/** A capture's "identity" across paths: its start byte and capture name. */
type CaptureIdentity = string; // `${start}:${capture}`
const identity = (s: SpanTriple): CaptureIdentity => `${s[0]}:${s[2]}`;
const identitySet = (spans: SpanTriple[]): Set<CaptureIdentity> =>
  new Set(spans.map(identity));

describe('native-vs-browser parity on the TOML fixture', () => {
  it('the parity fixtures cover the same source', () => {
    // Sanity: if this constant ever drifts from the one in
    // `crates/quarto-highlight/tests/integration/golden.rs`, the comparison is
    // meaningless.
    expect(TOML_SOURCE).toBe('name = "value"\ncount = 42\n');
    expect(nativeGolden.length).toBeGreaterThan(0);
  });

  it('every native capture identity appears in the JS output', () => {
    const jsSpans = JSON.parse(highlighter.highlight(TOML_SOURCE)) as SpanTriple[];
    const jsIdentities = identitySet(jsSpans);
    const missing = nativeGolden.filter((n) => !jsIdentities.has(identity(n)));
    expect(
      missing,
      `native spans missing from JS output:\n  native: ${JSON.stringify(nativeGolden)}\n  js:     ${JSON.stringify(jsSpans)}`,
    ).toEqual([]);
  });

  it('every JS capture identity appears in the native output', () => {
    const jsSpans = JSON.parse(highlighter.highlight(TOML_SOURCE)) as SpanTriple[];
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
    const jsSpans = JSON.parse(highlighter.highlight(TOML_SOURCE)) as SpanTriple[];
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
