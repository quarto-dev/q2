/**
 * JS-side user-grammar highlighter — Phase 4.2 of
 * `claude-notes/plans/2026-04-21-syntax-highlighting-phase-4.md`.
 *
 * Wraps `web-tree-sitter` to load a tree-sitter grammar compiled to
 * WebAssembly, compile its `highlights.scm` query, and emit
 * `data-hl-spans`-compatible JSON for a given source string.
 *
 * Wire format (matches `quarto_highlight_encoding`):
 *
 *   JSON.stringify([[start_byte, end_byte, capture_name], …])
 *
 * Bit-for-bit parity with the native `tree-sitter-highlight` Rust crate
 * is a **non-goal**. See the parent plan, "Design decision 1": we do
 * *not* port tree-sitter-highlight's capture-precedence / longest-match
 * resolution nor its locals/injections handling. The simplifications:
 *
 * - We walk `Query.captures()` directly (node-exact byte ranges) and
 *   then **flatten innermost-wins** — exactly mirroring the native
 *   resolver `quarto_highlight::flatten_spans`. The native render path
 *   switched off the lossy `collect_spans` event stream onto the same
 *   `Query.captures()` + `flatten_spans` pair (see the 2026-06-10 Monaco
 *   highlighting plan, Phase 0), so the browser and native render paths
 *   now produce **identical** flattened spans for user grammars — true
 *   parity by construction, and bd-98k6's same-start over-wrap is gone
 *   on both. Injection (`injections.scm`) and locals (`locals.scm`)
 *   queries are still ignored (the native user-grammar path also passes
 *   empty locals/injections).
 *
 * ## Flatten: innermost (narrowest) wins each byte
 *
 * Captures from one query over one tree are nested-or-disjoint, so for
 * any byte the covering captures form a strict nesting chain; the
 * narrowest paints over the wider one, splitting the wider span around
 * it. For `(bare_key) @type` nested in `(pair (bare_key)) @property`,
 * the result is `type` over the key and `property` over the gap bytes
 * around it — node-exact, non-overlapping. Equal-extent collisions (two
 * patterns on one node) are tie-broken by "later in the capture stream
 * wins"; no user-grammar fixture exercises that path.
 *
 * Output is non-overlapping and sorted by start byte. The HTML writer
 * treats span order as immaterial (see `crates/pampa/src/writers/html.rs`'s
 * `write_highlighted_body`); fed flat disjoint spans it emits
 * non-nested `<span>`s.
 */

import { Language, Parser, Query, type Node as TsNode } from 'web-tree-sitter';

let parserInitPromise: Promise<void> | null = null;

/**
 * Initialize the web-tree-sitter runtime exactly once per process.
 * `Parser.init()` is idempotent but doing a promise cache ourselves
 * means concurrent `loadUserGrammar` calls share the same in-flight
 * init, not serialized inits.
 *
 * In the browser, web-tree-sitter's emscripten glue tries to fetch
 * `web-tree-sitter.wasm` from the JS file's directory — which doesn't
 * exist after bundling. We resolve it through Vite's `?url` import so
 * the wasm ships as a hashed asset and `locateFile` points at it.
 * In node (vitest), emscripten uses `fs` and `locateFile` isn't needed.
 */
function ensureParserInit(): Promise<void> {
  if (parserInitPromise === null) {
    parserInitPromise = (async () => {
      // A browser page or a browser worker (the render worker) loads the
      // bundled wasm by URL; node reads it from disk.
      const inBrowser = typeof window !== 'undefined' || (globalThis as { WorkerGlobalScope?: unknown }).WorkerGlobalScope !== undefined;
      const opts: Parameters<typeof Parser.init>[0] | undefined =
        !inBrowser
          ? undefined
          : {
              locateFile: (filename: string) =>
                filename === 'web-tree-sitter.wasm' ? webTreeSitterWasmUrl : filename,
            };
      await Parser.init(opts);
    })();
  }
  return parserInitPromise;
}

// Vite asset import: emits the wasm as a hashed file in `dist/assets/`
// and gives us the final URL at build time. In node (vitest, no Vite),
// this import is still valid because Vite handles `?url` in dev/test
// via its own plugin; however, since we only consult it inside the
// browser branch (a page or a worker), node never executes it.
import webTreeSitterWasmUrl from 'web-tree-sitter/web-tree-sitter.wasm?url';

/**
 * Arguments to {@link loadUserGrammar}. The `name` is only used for
 * diagnostic messages; it does not affect parsing or highlighting.
 */
export interface LoadUserGrammarArgs {
  name: string;
  wasmBytes: Uint8Array;
  highlightsScm: string;
}

/**
 * A loaded user-grammar highlighter. `highlight(source)` returns the
 * JSON triple-array ready to drop into a code node's `data-hl-spans`
 * attribute. Call `dispose()` when the highlighter is no longer
 * needed; after disposal, `highlight()` is not safe to call.
 */
export interface UserGrammarHighlighter {
  readonly name: string;
  highlight(source: string): string;
  dispose(): void;
}

export async function loadUserGrammar(
  args: LoadUserGrammarArgs,
): Promise<UserGrammarHighlighter> {
  await ensureParserInit();

  const language = await Language.load(args.wasmBytes);
  const query = new Query(language, args.highlightsScm);
  const parser = new Parser();
  parser.setLanguage(language);

  let disposed = false;

  return {
    name: args.name,
    highlight(source: string): string {
      if (disposed) {
        throw new Error(`highlighter for ${args.name} is disposed`);
      }
      // Empty source has no captures; short-circuit to avoid a
      // spurious parse call.
      if (source.length === 0) {
        return '[]';
      }
      const tree = parser.parse(source);
      if (!tree) {
        // Parser returned null — typically means the parser was
        // reset mid-parse or the callback returned true. For a plain
        // string input neither applies, but we're defensive.
        return '[]';
      }
      try {
        const spans = collectSpans(query, tree.rootNode);
        return JSON.stringify(spans);
      } finally {
        tree.delete();
      }
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      query.delete();
      parser.delete();
      // `Language` objects are reference-counted; letting them drop
      // out of scope is fine. web-tree-sitter does not expose an
      // explicit free for Language instances loaded via `load()`.
    },
  };
}

type SpanTriple = [number, number, string];

/**
 * Walk `Query.captures()` over `root` (node-exact ranges) and flatten
 * innermost-wins into a non-overlapping, start-sorted run.
 */
function collectSpans(query: Query, root: TsNode): SpanTriple[] {
  const captures = query.captures(root);
  const raw: SpanTriple[] = new Array(captures.length);
  for (let i = 0; i < captures.length; i++) {
    const cap = captures[i];
    raw[i] = [cap.node.startIndex, cap.node.endIndex, cap.name];
  }
  return flattenSpans(raw);
}

/**
 * Innermost (narrowest) span wins each byte — the TS mirror of
 * `quarto_highlight::flatten_spans`. Drops zero-width spans; on a genuine
 * equal-extent collision the later span in the input order wins (the stable
 * sort keeps capture-stream order and the paint buffer lets the last paint win).
 */
function flattenSpans(input: SpanTriple[]): SpanTriple[] {
  const spans = input.filter((s) => s[1] > s[0]);
  if (spans.length === 0) return [];

  let min = Infinity;
  let max = -Infinity;
  for (const s of spans) {
    if (s[0] < min) min = s[0];
    if (s[1] > max) max = s[1];
  }

  // Stable sort: start ascending, then width descending so the wider span is
  // painted first and the narrower overwrites it. (Array.prototype.sort is
  // stable, so equal-extent spans keep capture-stream order.)
  spans.sort((a, b) => a[0] - b[0] || b[1] - b[0] - (a[1] - a[0]));

  const len = max - min;
  const owner = new Int32Array(len).fill(-1);
  for (let idx = 0; idx < spans.length; idx++) {
    const s = spans[idx];
    owner.fill(idx, s[0] - min, s[1] - min);
  }

  const out: SpanTriple[] = [];
  let runOwner = -1;
  let runStart = 0;
  for (let off = 0; off < len; off++) {
    const o = owner[off];
    if (o !== runOwner) {
      if (runOwner !== -1) {
        out.push([runStart + min, off + min, spans[runOwner][2]]);
      }
      runOwner = o;
      runStart = off;
    }
  }
  if (runOwner !== -1) {
    out.push([runStart + min, len + min, spans[runOwner][2]]);
  }
  return out;
}
