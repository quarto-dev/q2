/**
 * Integration tests for `makeSelfContainedHtml` (bd-vhdknrvl, issue #315).
 *
 * Runs in jsdom because the function parses/serializes HTML with
 * `DOMParser`. VFS reads are injected as plain callbacks (no module
 * mocking) so the tests are decoupled from the WASM singleton.
 *
 * The function turns a standalone-but-not-self-contained render (the
 * kind the WASM HTML pipeline emits, with `/.quarto/…` `<link>`/
 * `<script>` refs and relative `<img>` srcs) into a single file that
 * renders correctly when opened as a bare top-level document — every
 * asset inlined as `<style>` / inline `<script>` / `data:` URI.
 */

import { describe, it, expect } from 'vitest';
import {
  makeSelfContainedHtml,
  type SelfContainedReaders,
} from './makeSelfContainedHtml';

/** A tiny 1×1 PNG, base64 (matches the shared test fixture bytes). */
const PNG_B64 =
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
const WOFF2_B64 = 'd29mZjJieXRlcw=='; // "woff2bytes"

function readers(
  text: Record<string, string> = {},
  bin: Record<string, string> = {},
): SelfContainedReaders {
  return {
    readText: (p) => (p in text ? text[p] : null),
    readBinaryBase64: (p) => (p in bin ? bin[p] : null),
  };
}

function parse(html: string): Document {
  return new DOMParser().parseFromString(html, 'text/html');
}

const DOC_PATH = '/project/sub/doc.qmd';

describe('makeSelfContainedHtml', () => {
  it('inlines a /.quarto stylesheet <link> into a <style> and drops the link', () => {
    const html = `<!DOCTYPE html><html><head>
      <link rel="stylesheet" href="/.quarto/project-artifacts/styles.css">
      </head><body><p>hi</p></body></html>`;
    const out = makeSelfContainedHtml(html, DOC_PATH, {
      ...readers({ '/.quarto/project-artifacts/styles.css': 'p{color:red}' }),
    });
    const doc = parse(out);
    // no external /.quarto link survives
    expect(doc.querySelector('link[href^="/.quarto/"]')).toBeNull();
    // a <style> with the CSS text is present
    const style = doc.querySelector('style');
    expect(style?.textContent).toContain('p{color:red}');
  });

  it('preserves the media attribute when converting <link> to <style>', () => {
    const html = `<html><head>
      <link rel="stylesheet" media="print" href="/.quarto/project-artifacts/print.css">
      </head><body></body></html>`;
    const out = makeSelfContainedHtml(
      html,
      DOC_PATH,
      readers({ '/.quarto/project-artifacts/print.css': '@page{margin:1in}' }),
    );
    const style = parse(out).querySelector('style');
    expect(style?.getAttribute('media')).toBe('print');
    expect(style?.textContent).toContain('@page');
  });

  it('inlines a /.quarto <script src> into an executing inline <script>', () => {
    const html = `<html><head></head><body>
      <script src="/.quarto/project-artifacts/reveal.js"></script>
      </body></html>`;
    const out = makeSelfContainedHtml(
      html,
      DOC_PATH,
      readers({ '/.quarto/project-artifacts/reveal.js': 'window.RUN=1;' }),
    );
    const doc = parse(out);
    expect(doc.querySelector('script[src]')).toBeNull();
    const script = doc.querySelector('script');
    expect(script?.textContent).toContain('window.RUN=1;');
  });

  it('inlines a relative <img> src resolved against the document directory', () => {
    const html = `<html><body><img src="figures/plot.png" alt="p"></body></html>`;
    const out = makeSelfContainedHtml(
      html,
      DOC_PATH,
      // /project/sub/doc.qmd + figures/plot.png → key project/sub/figures/plot.png
      readers({}, { 'project/sub/figures/plot.png': PNG_B64 }),
    );
    const img = parse(out).querySelector('img');
    expect(img?.getAttribute('src')).toBe(`data:image/png;base64,${PNG_B64}`);
  });

  it('inlines a /.quarto <img> src as a data URI', () => {
    const html = `<html><body><img src="/.quarto/project-artifacts/logo.png"></body></html>`;
    const out = makeSelfContainedHtml(
      html,
      DOC_PATH,
      readers({}, { '/.quarto/project-artifacts/logo.png': PNG_B64 }),
    );
    const img = parse(out).querySelector('img');
    expect(img?.getAttribute('src')).toBe(`data:image/png;base64,${PNG_B64}`);
  });

  it('rewrites font url() references inside inlined CSS to data URIs', () => {
    const css = '@font-face{font-family:x;src:url(fonts/f.woff2) format("woff2")}';
    const html = `<html><head>
      <link rel="stylesheet" href="/.quarto/project-artifacts/styles.css">
      </head><body></body></html>`;
    const out = makeSelfContainedHtml(html, DOC_PATH, {
      readText: (p) =>
        p === '/.quarto/project-artifacts/styles.css' ? css : null,
      // url() is relative to the CSS file's dir → /.quarto/project-artifacts/fonts/f.woff2
      readBinaryBase64: (p) =>
        p === '/.quarto/project-artifacts/fonts/f.woff2' ? WOFF2_B64 : null,
    });
    const style = parse(out).querySelector('style');
    expect(style?.textContent).toContain(
      `data:font/woff2;base64,${WOFF2_B64}`,
    );
    expect(style?.textContent).not.toContain('url(fonts/f.woff2)');
  });

  it('strips a leading BOM from inlined CSS so the first rule is not dropped', () => {
    // A UTF-8 BOM (U+FEFF) prefixing CSS bytes is stripped by the browser
    // when loaded via <link>, but injected verbatim into a <style> it
    // invalidates the first selector — dropping e.g. Bootstrap's
    // `:root,[data-bs-theme=light]{…}` variable block so the theme
    // silently fails to apply (issue #315 field bug).
    const css = '﻿:root{--x:1}\nbody{color:red}';
    const html = `<html><head>
      <link rel="stylesheet" href="/.quarto/project-artifacts/styles.css">
      </head><body></body></html>`;
    const out = makeSelfContainedHtml(html, DOC_PATH, {
      ...readers({ '/.quarto/project-artifacts/styles.css': css }),
    });
    const style = parse(out).querySelector('style');
    expect(style?.textContent?.charCodeAt(0)).not.toBe(0xfeff);
    expect(style?.textContent?.startsWith(':root')).toBe(true);
  });

  it('strips a leading BOM from inlined scripts', () => {
    const html = `<html><body>
      <script src="/.quarto/project-artifacts/app.js"></script></body></html>`;
    const out = makeSelfContainedHtml(html, DOC_PATH, {
      ...readers({ '/.quarto/project-artifacts/app.js': '﻿window.OK=1;' }),
    });
    const script = parse(out).querySelector('script');
    expect(script?.textContent?.charCodeAt(0)).not.toBe(0xfeff);
    expect(script?.textContent).toBe('window.OK=1;');
  });

  it('leaves external and data: references untouched', () => {
    const html = `<html><head>
      <link rel="stylesheet" href="https://cdn.example/x.css">
      </head><body>
      <img src="https://img.example/a.png">
      <img src="data:image/png;base64,AAAA">
      <script src="https://cdn.example/x.js"></script>
      </body></html>`;
    const out = makeSelfContainedHtml(html, DOC_PATH, readers());
    const doc = parse(out);
    expect(doc.querySelector('link[href="https://cdn.example/x.css"]')).not.toBeNull();
    expect(doc.querySelector('img[src="https://img.example/a.png"]')).not.toBeNull();
    expect(doc.querySelector('img[src="data:image/png;base64,AAAA"]')).not.toBeNull();
    expect(doc.querySelector('script[src="https://cdn.example/x.js"]')).not.toBeNull();
  });

  it('leaves a reference untouched when the VFS has no bytes for it', () => {
    const html = `<html><body><img src="figures/missing.png"></body></html>`;
    const out = makeSelfContainedHtml(html, DOC_PATH, readers()); // empty VFS
    const img = parse(out).querySelector('img');
    expect(img?.getAttribute('src')).toBe('figures/missing.png');
  });

  it('emits a full document beginning with a DOCTYPE', () => {
    const out = makeSelfContainedHtml(
      `<html><head></head><body><p>x</p></body></html>`,
      DOC_PATH,
      readers(),
    );
    expect(out.startsWith('<!DOCTYPE html>')).toBe(true);
    expect(out).toContain('<p>x</p>');
  });
});
