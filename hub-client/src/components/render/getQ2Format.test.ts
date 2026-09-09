/**
 * The hub-client renderer-routing rule (bd-kltzdhle, D2 in
 * `claude-notes/plans/2026-09-09-hub-client-default-q2-preview.md`).
 *
 * `getQ2Format` reads the format `MetadataMergeStage` wrote into
 * `meta.format` and decides which preview branch `PreviewRouter`
 * mounts: a non-null string mounts `ReactPreview` with that format; `null`
 * mounts the full-DOM `Preview` (MorphIframe). The rule mirrors
 * `map_format_for_preview` in `crates/wasm-quarto-hub-client/src/lib.rs`
 * for the `html` arm, so hub-client's default is `q2 preview`'s default.
 */

import { describe, it, expect } from 'vitest';
import { getQ2Format } from './getQ2Format';

function astWithFormat(format: string | undefined): string {
  const meta =
    format === undefined ? {} : { format: { t: 'MetaString', c: format } };
  return JSON.stringify({ 'pandoc-api-version': [1, 23, 1], meta, blocks: [] });
}

describe('getQ2Format (hub-client renderer routing)', () => {
  it('routes html — the default when no format: key is set — to q2-preview', () => {
    // `detect_format_from_content` reports `html` for a missing key, a
    // bare `format: html`, and a `format: html: {…}` map, and the merge
    // stage writes that normalised string back; all three arrive here
    // as the single string `html`.
    expect(getQ2Format(astWithFormat('html'))).toBe('q2-preview');
  });

  it('routes q2-html-render to the full-DOM renderer (null)', () => {
    expect(getQ2Format(astWithFormat('q2-html-render'))).toBeNull();
  });

  it('passes the React-side formats through unchanged', () => {
    for (const f of ['q2-preview', 'q2-debug', 'q2-slides', 'q2-sandboxed-preview', 'revealjs']) {
      expect(getQ2Format(astWithFormat(f)), f).toBe(f);
    }
  });

  it('routes non-html formats to the full-DOM renderer (null)', () => {
    for (const f of ['pdf', 'docx', 'typst', 'gfm', 'acm-html']) {
      expect(getQ2Format(astWithFormat(f)), f).toBeNull();
    }
  });

  it('returns null when meta carries no format at all', () => {
    // Only reachable for ASTs that did not pass through the merge stage
    // (it always writes `format`); keep the defensive branch pinned.
    expect(getQ2Format(astWithFormat(undefined))).toBeNull();
  });

  it('returns null for unparseable AST JSON', () => {
    expect(getQ2Format('not json')).toBeNull();
  });
});
