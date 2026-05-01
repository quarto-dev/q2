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
});
