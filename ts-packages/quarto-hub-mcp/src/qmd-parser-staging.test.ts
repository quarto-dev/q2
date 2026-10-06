/**
 * Unit tests for scripts/stage-qmd-parser.mjs (CAP-11): the staging rules
 * for the wasm-qmd-parser package that ships inside the bundle's mini
 * node_modules. Offline by construction: fixture pkg trees are fabricated
 * in a temp dir. The fail-closed guarantee matters because a silently
 * parser-less bundle would only fail the CAP-11 tools at runtime.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
// @ts-expect-error — plain .mjs module without type declarations
import { stageQmdParser } from '../scripts/stage-qmd-parser.mjs';

let tmp: string;
let pkgDir: string;
let outDir: string;

function fabricatePkg(files: string[]) {
  fs.mkdirSync(pkgDir, { recursive: true });
  for (const name of files) {
    fs.writeFileSync(path.join(pkgDir, name), name === 'package.json' ? '{}' : 'bytes');
  }
}

beforeEach(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'qmd-parser-staging-test-'));
  pkgDir = path.join(tmp, 'pkg');
  outDir = path.join(tmp, 'bundle');
});

afterEach(() => {
  fs.rmSync(tmp, { recursive: true, force: true });
});

describe('stageQmdParser', () => {
  it('stages a complete pkg into node_modules/wasm-qmd-parser', () => {
    fabricatePkg([
      'package.json',
      'wasm_qmd_parser.js',
      'wasm_qmd_parser.d.ts',
      'wasm_qmd_parser_bg.wasm',
    ]);
    const staged = stageQmdParser({ pkgDir, outDir });
    expect(staged.sort()).toEqual(
      [
        'package.json',
        'wasm_qmd_parser.js',
        'wasm_qmd_parser.d.ts',
        'wasm_qmd_parser_bg.wasm',
      ].sort(),
    );
    const dest = path.join(outDir, 'node_modules', 'wasm-qmd-parser');
    for (const name of staged) {
      expect(fs.existsSync(path.join(dest, name))).toBe(true);
    }
  });

  it('fails closed when the pkg is missing the wasm', () => {
    fabricatePkg(['package.json', 'wasm_qmd_parser.js', 'wasm_qmd_parser.d.ts']);
    expect(() => stageQmdParser({ pkgDir, outDir })).toThrow(/incomplete/);
  });

  it('fails closed when the pkg dir does not exist', () => {
    expect(() => stageQmdParser({ pkgDir, outDir })).toThrow(/incomplete/);
  });

  it('replaces a previously staged copy (no stale files linger)', () => {
    fabricatePkg([
      'package.json',
      'wasm_qmd_parser.js',
      'wasm_qmd_parser.d.ts',
      'wasm_qmd_parser_bg.wasm',
    ]);
    stageQmdParser({ pkgDir, outDir });
    const dest = path.join(outDir, 'node_modules', 'wasm-qmd-parser');
    fs.writeFileSync(path.join(dest, 'stale-file'), 'old');
    stageQmdParser({ pkgDir, outDir });
    expect(fs.existsSync(path.join(dest, 'stale-file'))).toBe(false);
  });
});
