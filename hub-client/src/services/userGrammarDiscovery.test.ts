/**
 * Vitest coverage for `userGrammarDiscovery` — Phase 4.5 of
 * `claude-notes/plans/2026-04-21-syntax-highlighting-phase-4.md`.
 *
 * Discovery walks a project's file list and returns, for each valid
 * `_quarto/grammars/<name>/` subdirectory, a `{ class, wasmPath,
 * highlightsPath }` triple. Valid = exactly one `.wasm` file + a
 * `highlights.scm`. Mirrors the native rule at
 * `crates/quarto-highlight/src/user_grammar.rs:load_all_from_parent`.
 *
 * No I/O here — the tests feed synthetic path lists directly to the
 * discovery function.
 */

import { describe, expect, it } from 'vitest';

import { discoverUserGrammars } from './userGrammarDiscovery';

describe('discoverUserGrammars', () => {
  it('returns [] for a project with no grammars directory', () => {
    expect(
      discoverUserGrammars(['index.qmd', 'README.md', 'images/logo.png']),
    ).toEqual([]);
  });

  it('discovers a single valid grammar', () => {
    const paths = [
      'index.qmd',
      '_quarto/grammars/toml/toml.wasm',
      '_quarto/grammars/toml/highlights.scm',
    ];
    expect(discoverUserGrammars(paths)).toEqual([
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/toml/toml.wasm',
        highlightsPath: '_quarto/grammars/toml/highlights.scm',
      },
    ]);
  });

  it('discovers multiple valid grammars and sorts them by class name', () => {
    const paths = [
      '_quarto/grammars/zig/zig.wasm',
      '_quarto/grammars/zig/highlights.scm',
      '_quarto/grammars/abap/abap.wasm',
      '_quarto/grammars/abap/highlights.scm',
    ];
    expect(discoverUserGrammars(paths).map((g) => g.class)).toEqual([
      'abap',
      'zig',
    ]);
  });

  it('ignores subdirectories missing highlights.scm', () => {
    const paths = [
      '_quarto/grammars/toml/toml.wasm',
      // no highlights.scm
    ];
    expect(discoverUserGrammars(paths)).toEqual([]);
  });

  it('ignores subdirectories missing the .wasm file', () => {
    const paths = [
      '_quarto/grammars/toml/highlights.scm',
      // no .wasm
    ];
    expect(discoverUserGrammars(paths)).toEqual([]);
  });

  it('ignores subdirectories with multiple .wasm files (ambiguous)', () => {
    const paths = [
      '_quarto/grammars/toml/toml.wasm',
      '_quarto/grammars/toml/other.wasm',
      '_quarto/grammars/toml/highlights.scm',
    ];
    expect(discoverUserGrammars(paths)).toEqual([]);
  });

  it('tolerates incidental files (PROVENANCE.md, injections.scm, locals.scm) in a valid grammar dir', () => {
    const paths = [
      '_quarto/grammars/toml/toml.wasm',
      '_quarto/grammars/toml/highlights.scm',
      '_quarto/grammars/toml/PROVENANCE.md',
      '_quarto/grammars/toml/injections.scm',
      '_quarto/grammars/toml/locals.scm',
    ];
    expect(discoverUserGrammars(paths).map((g) => g.class)).toEqual(['toml']);
  });

  it('only looks inside _quarto/grammars/<name>/, not deeper', () => {
    // A .wasm + .scm buried two levels deep isn't a grammar.
    const paths = [
      '_quarto/grammars/foo/bar/baz.wasm',
      '_quarto/grammars/foo/bar/highlights.scm',
    ];
    expect(discoverUserGrammars(paths)).toEqual([]);
  });

  it('ignores top-level files under _quarto/grammars/ that are not in a subdirectory', () => {
    // `_quarto/grammars/README.md` is a top-level file, not a grammar.
    const paths = [
      '_quarto/grammars/README.md',
      '_quarto/grammars/toml/toml.wasm',
      '_quarto/grammars/toml/highlights.scm',
    ];
    expect(discoverUserGrammars(paths).map((g) => g.class)).toEqual(['toml']);
  });

  it('mixes valid grammars and unrelated project files cleanly', () => {
    const paths = [
      'index.qmd',
      'data/series.csv',
      'images/logo.png',
      '_quarto/grammars/toml/toml.wasm',
      '_quarto/grammars/toml/highlights.scm',
      '_quarto/grammars/broken-grammar/broken.wasm',
      // broken-grammar has no highlights.scm → skipped
      '_quarto/_site/index.html',
    ];
    expect(discoverUserGrammars(paths).map((g) => g.class)).toEqual(['toml']);
  });

  it('rejects paths with leading slash (not our convention)', () => {
    // VFS paths have no leading slash per the uploader plan. A leading
    // slash suggests the caller is passing WASM-side `/project/` paths
    // by mistake; we detect this explicitly instead of silently
    // coercing.
    const paths = [
      '/_quarto/grammars/toml/toml.wasm',
      '/_quarto/grammars/toml/highlights.scm',
    ];
    expect(discoverUserGrammars(paths)).toEqual([]);
  });

  it("class name is the .wasm stem, not the directory name", () => {
    // Native uses the file stem (see user_grammar.rs:find_wasm_in_dir).
    // Following the same rule in JS: a dir named `my-grammar` that
    // contains `toml.wasm` registers the class `toml`. (If this feels
    // surprising, we can change both paths later to prefer the dir
    // name — but match native for now.)
    const paths = [
      '_quarto/grammars/my-grammar/toml.wasm',
      '_quarto/grammars/my-grammar/highlights.scm',
    ];
    expect(discoverUserGrammars(paths)).toEqual([
      {
        class: 'toml',
        wasmPath: '_quarto/grammars/my-grammar/toml.wasm',
        highlightsPath: '_quarto/grammars/my-grammar/highlights.scm',
      },
    ]);
  });
});
