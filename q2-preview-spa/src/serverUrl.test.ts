/**
 * Server URLs for the live preview resolve against the page, so the SPA
 * works behind a path-stripping proxy such as `/p/<token>/` (plan
 * `claude-notes/plans/2026-11-05-q2-preview-proxy-base-path.md`).
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { serverUrl, serverWsUrl } from './serverUrl';

describe('serverUrl', () => {
  it('resolves a route under the page directory', () => {
    expect(serverUrl('/health', 'https://host/p/abc123/?page=doc.qmd')).toBe(
      'https://host/p/abc123/health',
    );
    expect(serverUrl('/api/preview/deps?page=a%20b.qmd', 'https://host/p/abc123/')).toBe(
      'https://host/p/abc123/api/preview/deps?page=a%20b.qmd',
    );
  });

  it('resolves at the server root without a prefix', () => {
    expect(serverUrl('/api/preview/config', 'http://127.0.0.1:4000/?page=doc.qmd')).toBe(
      'http://127.0.0.1:4000/api/preview/config',
    );
  });
});

describe('serverWsUrl', () => {
  it('swaps the scheme and keeps the prefix', () => {
    expect(serverWsUrl('/ws', 'https://host/p/abc123/?page=doc.qmd')).toBe(
      'wss://host/p/abc123/ws',
    );
    expect(serverWsUrl('/ws', 'http://127.0.0.1:4000/')).toBe('ws://127.0.0.1:4000/ws');
  });
});

/** Every non-test source file under `src/`. */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const full = path.join(dir, name);
    if (statSync(full).isDirectory()) return name === 'test-utils' ? [] : sources(full);
    return /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) ? [full] : [];
  });
}

/**
 * Places in `source` that build a server URL from the page origin
 * (`loc.host`, `location.origin`) or hand `fetch` / `WebSocket` /
 * `EventSource` a root-absolute literal or an ALL_CAPS route constant
 * (`fetch(PDF_ROUTE)`) instead of going through `serverUrl`. Matches across
 * line breaks; comment lines are skipped.
 */
function rootAbsoluteUrls(source: string): { line: number; text: string }[] {
  const lines = source.split('\n');
  const code = lines.map((l) => (/^\s*(\/\/|\/?\*)/.test(l) ? '' : l)).join('\n');
  const patterns = [
    /\.(?:host|origin)\b/g,
    /\b(?:fetch|WebSocket|EventSource)\(\s*(?:['"`]\/|[A-Z][A-Z0-9_]*\b)/g,
  ];
  return patterns
    .flatMap((re) => [...code.matchAll(re)])
    .map((m) => {
      const line = code.slice(0, m.index).split('\n').length;
      return { line, text: lines[line - 1].trim() };
    })
    .sort((a, b) => a.line - b.line);
}

describe('rootAbsoluteUrls', () => {
  it('flags every shape the proxy fix replaced', () => {
    const offenders = [
      'const healthUrl = `${loc.protocol}//${loc.host}/health`;',
      'const u = `${window.location.protocol}//${window.location.host}/health`;',
      'const u = new URL("/ws", location.origin);',
      "const resp = await fetch('/api/preview/re-execute', {",
      'const resp = await fetch(\n  `/api/preview/deps?page=${encodeURIComponent(p)}`,\n);',
      'const response = await fetch(PDF_ROUTE, {',
      "const ws = new WebSocket('/ws');",
      "const es = new EventSource(\n  '/__q2-preview/events',\n);",
    ];
    for (const src of offenders) {
      expect(rootAbsoluteUrls(src), src).not.toEqual([]);
    }
  });

  it('accepts URLs resolved through serverUrl', () => {
    const fine = [
      "const resp = await fetch(serverUrl('/health'));",
      'const resp = await fetch(\n  serverUrl(`/api/preview/deps?page=${p}`),\n);',
      'const response = await fetch(serverUrl(PDF_ROUTE), {',
      'const resp = await fetch(healthUrl);',
      "const ws = new WebSocket(serverWsUrl('/ws'));",
      "// fetch('/health') used to live here",
    ];
    for (const src of fine) {
      expect(rootAbsoluteUrls(src), src).toEqual([]);
    }
  });
});

describe('SPA sources', () => {
  it('build no server URL from the origin or as a root-absolute path', () => {
    const offenders = sources(__dirname).flatMap((file) =>
      rootAbsoluteUrls(readFileSync(file, 'utf8')).map(
        ({ line, text }) => `${path.relative(__dirname, file)}:${line}: ${text}`,
      ),
    );
    expect(offenders).toEqual([]);
  });
});
