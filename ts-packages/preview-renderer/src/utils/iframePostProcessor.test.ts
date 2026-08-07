/**
 * Unit tests for `reverseMapArtifactHref` (bd-lnd3).
 *
 * The integration behavior of `postProcessIframe` (which calls
 * this helper plus mutates a Document) is exercised in a separate
 * integration test under jsdom. This file is the pure-function
 * counterpart and runs in plain node.
 */

import { describe, it, expect } from 'vitest';
import { reverseMapArtifactHref } from './iframePostProcessor';

const FILES: readonly string[] = [
  'index.qmd',
  'about.qmd',
  'posts/first.qmd',
  'posts/second.qmd',
  'logo.png',
];

describe('reverseMapArtifactHref', () => {
  // ─── bd-6d2wj4zp Phase 5: .md is a renderable source ────────────
  it('reverse-maps to a source .md when no .qmd sibling exists', () => {
    const files = ['index.qmd', 'admin/index.md', 'notes.md'];
    expect(
      reverseMapArtifactHref('/.quarto/project-artifacts/notes.html', files),
    ).toEqual({ path: 'notes.md', anchor: null });
    expect(
      reverseMapArtifactHref(
        '/.quarto/project-artifacts/admin/index.html#setup',
        files,
      ),
    ).toEqual({ path: 'admin/index.md', anchor: 'setup' });
  });

  it('prefers .qmd over .md when both stems exist', () => {
    // Both rendering to the same .html is a render-time collision
    // anyway; the reverse map just needs a deterministic order.
    const files = ['about.qmd', 'about.md'];
    expect(
      reverseMapArtifactHref('/.quarto/project-artifacts/about.html', files),
    ).toEqual({ path: 'about.qmd', anchor: null });
  });

  it('maps bare artifact root to index.md when only index.md exists', () => {
    const files = ['index.md', 'about.qmd'];
    expect(
      reverseMapArtifactHref('/.quarto/project-artifacts/', files),
    ).toEqual({ path: 'index.md', anchor: null });
  });

  it('reverse-maps a top-level artifact-rooted .html URL to its source .qmd', () => {
    expect(
      reverseMapArtifactHref('/.quarto/project-artifacts/about.html', FILES),
    ).toEqual({ path: 'about.qmd', anchor: null });
  });

  it('preserves anchors through the reverse mapping', () => {
    expect(
      reverseMapArtifactHref(
        '/.quarto/project-artifacts/about.html#intro',
        FILES,
      ),
    ).toEqual({ path: 'about.qmd', anchor: 'intro' });
  });

  it('handles subdirectory paths', () => {
    expect(
      reverseMapArtifactHref(
        '/.quarto/project-artifacts/posts/first.html',
        FILES,
      ),
    ).toEqual({ path: 'posts/first.qmd', anchor: null });
  });

  it('returns null when the source file is not in the project', () => {
    expect(
      reverseMapArtifactHref(
        '/.quarto/project-artifacts/notes.html',
        FILES,
      ),
    ).toBeNull();
  });

  it('returns null for non-html artifact paths', () => {
    expect(
      reverseMapArtifactHref(
        '/.quarto/project-artifacts/styles.css',
        FILES,
      ),
    ).toBeNull();
  });

  it('returns null for non-artifact-rooted hrefs', () => {
    expect(reverseMapArtifactHref('./about.qmd', FILES)).toBeNull();
    expect(reverseMapArtifactHref('about.html', FILES)).toBeNull();
    expect(
      reverseMapArtifactHref('https://example.com/about.html', FILES),
    ).toBeNull();
  });

  it('returns null for an empty or anchor-only href', () => {
    expect(reverseMapArtifactHref('', FILES)).toBeNull();
    expect(reverseMapArtifactHref('#section', FILES)).toBeNull();
  });

  it('returns null when projectFilePaths is empty', () => {
    expect(
      reverseMapArtifactHref('/.quarto/project-artifacts/about.html', []),
    ).toBeNull();
  });

  it('does not consume non-renderable extensions disguised under .html stripping', () => {
    // `logo.png` is in the project but the URL form would be
    // `logo.png.html` for the helper to even consider it — and
    // `logo.png` doesn't end with one of the renderable extensions.
    expect(
      reverseMapArtifactHref('/.quarto/project-artifacts/logo.html', FILES),
    ).toBeNull();
  });

  it('respects path separators (no fuzzy matching)', () => {
    // `posts-first.html` should not match `posts/first.qmd`.
    expect(
      reverseMapArtifactHref(
        '/.quarto/project-artifacts/posts-first.html',
        FILES,
      ),
    ).toBeNull();
  });

  // ─── bd-ql55q: bare artifact-root (directory URL = project home) ──
  //
  // `page_url_for_site_root_dir()` in VFS-root mode returns
  // `/.quarto/project-artifacts/` — a trailing-slash directory URL.
  // The navbar brand falls back to this when no `logo-href` is set,
  // and other site-root navigation surfaces may follow. Mirror the
  // browser's static-server "directory URL = index.html" convention
  // by reverse-mapping the bare root to `index.qmd` when present.
  // Strict-list policy of this surface is preserved: return null if
  // there is no `index.qmd` to map to.

  it('reverse-maps bare artifact root to index.qmd when present', () => {
    // bd-ql55q P-A
    expect(
      reverseMapArtifactHref('/.quarto/project-artifacts/', FILES),
    ).toEqual({ path: 'index.qmd', anchor: null });
  });

  it('preserves anchor on bare artifact root', () => {
    // bd-ql55q P-B
    expect(
      reverseMapArtifactHref('/.quarto/project-artifacts/#intro', FILES),
    ).toEqual({ path: 'index.qmd', anchor: 'intro' });
  });

  it('returns null for bare artifact root when no index.qmd is in the project', () => {
    // bd-ql55q P-C: strict policy — don't intercept if there's
    // nothing to map to. Lets the click pass through with whatever
    // the calling surface's default is.
    const filesWithoutIndex = ['about.qmd', 'posts/first.qmd'] as const;
    expect(
      reverseMapArtifactHref('/.quarto/project-artifacts/', filesWithoutIndex),
    ).toBeNull();
    expect(
      reverseMapArtifactHref(
        '/.quarto/project-artifacts/#intro',
        filesWithoutIndex,
      ),
    ).toBeNull();
  });
});
