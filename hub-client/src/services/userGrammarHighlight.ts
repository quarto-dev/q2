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
 * - We walk `Query.captures()` directly and emit one span per capture.
 *   Capture resolution on overlaps is whatever web-tree-sitter's native
 *   pattern evaluator yields, not the longest-match-wins of
 *   tree-sitter-highlight.
 * - Injection queries (`injections.scm`) are ignored.
 * - Locals queries (`locals.scm`) are ignored. The native user-grammar
 *   path also passes empty locals/injections (see
 *   `quarto-highlight/src/user_grammar.rs:151`), so for user grammars
 *   the divergence is small; for built-ins it would matter more but
 *   built-ins don't flow through this code path at all.
 *
 * ## Known divergence from native output: nested-capture end bytes
 *
 * When the query has two captures that open at the same start byte
 * (e.g. `(bare_key) @type` on the inner key node + `(pair (bare_key))
 * @property` on the enclosing pair), `tree-sitter-highlight`'s
 * `HighlightEvent` stream emits both `HighlightStart`s back-to-back
 * with no intervening `Source`. The Rust `collect_spans` records
 * both spans with the outer capture's end byte (the cursor only
 * advances on `Source` events), so the inner span's reported range
 * stretches to match the outer.
 *
 * `Query.captures()` gives node-exact ranges, so this implementation
 * reports the inner capture with its actual node end. Rendered HTML
 * consequently differs for same-start nested captures:
 *
 * - Native: `<span class="hl-property"><span class="hl-type">
 *   name = "value"</span></span>` — outer-capture class wraps the
 *   whole pair and the inner-capture class wraps it too.
 * - Browser: `<span class="hl-property"><span class="hl-type">
 *   name</span> = "value"</span>` — the inner-capture class wraps
 *   only the key.
 *
 * The parity test at `userGrammarParity.wasm.test.ts` pins this down:
 * every native capture identity (start+name) appears in the JS
 * output and vice versa, and native end-byte is always >= JS
 * end-byte for the same identity. Anything tighter would require
 * porting tree-sitter-highlight's event-stream semantics to JS.
 *
 * Output is sorted canonically `(startIndex asc, endIndex desc)` so
 * identical (grammar, source) inputs produce identical strings — useful
 * for caching and for the parity test. The HTML writer treats span
 * order as immaterial (see `crates/pampa/src/writers/html.rs`'s
 * `write_highlighted_body`), so canonical ordering here doesn't change
 * what users see.
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
      const opts: Parameters<typeof Parser.init>[0] | undefined =
        typeof window === 'undefined'
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
// `typeof window !== 'undefined'` branch, node never executes it.
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
 * Walk `Query.captures()` over `root` and return the captures as
 * `[start, end, capture]` triples, sorted `(start asc, end desc)` so
 * outer ranges come before the inner ranges they enclose — the order
 * the HTML writer expects for nested opens.
 */
function collectSpans(query: Query, root: TsNode): SpanTriple[] {
  const captures = query.captures(root);
  const spans: SpanTriple[] = new Array(captures.length);
  for (let i = 0; i < captures.length; i++) {
    const cap = captures[i];
    spans[i] = [cap.node.startIndex, cap.node.endIndex, cap.name];
  }
  spans.sort((a, b) => a[0] - b[0] || b[1] - a[1]);
  return spans;
}
