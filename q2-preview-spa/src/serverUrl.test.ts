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

describe('SPA sources', () => {
  it('build no server URL from the origin or as a root-absolute path', () => {
    const offenders: string[] = [];
    for (const file of sources(__dirname)) {
      readFileSync(file, 'utf8')
        .split('\n')
        .forEach((line, i) => {
          if (/^\s*(\/\/|\*)/.test(line)) return;
          if (/location\.host\b|fetch\(\s*['"`]\//.test(line)) {
            offenders.push(`${path.relative(__dirname, file)}:${i + 1}: ${line.trim()}`);
          }
        });
    }
    expect(offenders).toEqual([]);
  });
});
