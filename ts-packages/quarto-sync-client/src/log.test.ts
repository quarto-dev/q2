/**
 * Tests for the injectable diagnostic logger (bd-sl4o01y0).
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { setSyncLogger, syncLog } from './log.js';

const srcDir = path.dirname(fileURLToPath(import.meta.url));

describe('syncLog', () => {
  afterEach(() => {
    // restore the default sink so test order can't leak a custom logger
    setSyncLogger((...args) => console.log(...args));
  });

  it('defaults to console.log (browser behavior)', () => {
    const spy = vi.spyOn(console, 'log').mockImplementation(() => {});
    syncLog('hello', 42);
    expect(spy).toHaveBeenCalledWith('hello', 42);
    spy.mockRestore();
  });

  it('routes through an injected sink instead of console.log', () => {
    const consoleSpy = vi.spyOn(console, 'log').mockImplementation(() => {});
    const lines: unknown[][] = [];
    setSyncLogger((...args) => lines.push(args));
    syncLog('peer connected', { peerId: 'x' });
    expect(lines).toEqual([['peer connected', { peerId: 'x' }]]);
    expect(consoleSpy).not.toHaveBeenCalled();
    consoleSpy.mockRestore();
  });
});

describe('stdout-purity invariant', () => {
  it('no library source calls console.log directly (use syncLog)', () => {
    // Library code must route diagnostics through the injectable
    // logger: a direct console.log corrupts stdio MCP hosts
    // (bd-sl4o01y0). log.ts holds the single sanctioned default.
    const offenders: string[] = [];
    for (const entry of fs.readdirSync(srcDir)) {
      if (!entry.endsWith('.ts') || entry.endsWith('.test.ts')) continue;
      if (entry === 'log.ts') continue;
      const content = fs.readFileSync(path.join(srcDir, entry), 'utf8');
      content.split('\n').forEach((line, i) => {
        if (line.includes('console.log(')) {
          offenders.push(`${entry}:${i + 1}: ${line.trim()}`);
        }
      });
    }
    expect(offenders).toEqual([]);
  });
});
