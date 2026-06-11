/**
 * Regression test for bd-2d8ur7e9: the server must start when invoked
 * through a symlinked path.
 *
 * Node's ESM loader canonicalizes `import.meta.url` through realpath,
 * but `process.argv[1]` is whatever path the invoker used. The
 * am-I-the-entry-module guard in index.ts compared the two without
 * canonicalizing argv[1], so any symlink in the invocation path made
 * the process load modules and exit 0 without ever starting the
 * server. This is not exotic: macOS `/tmp` and `/var` are symlinks
 * into `/private`, and npm/npx `.bin` shims are symlinks — the npx
 * distribution channel would not have worked at all.
 */

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

const pkgRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const realEntry = path.join(pkgRoot, 'dist', 'index.js');

let tmpDir: string;
let symlinkEntry: string;

beforeAll(() => {
  tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'hub-mcp-symlink-test-'));
  symlinkEntry = path.join(tmpDir, 'linked-index.js');
  fs.symlinkSync(realEntry, symlinkEntry);
});

afterAll(() => {
  fs.rmSync(tmpDir, { recursive: true, force: true });
});

describe('symlinked invocation', () => {
  it('still reaches main() (prints --help usage rather than silently exiting)', () => {
    const res = spawnSync('node', [symlinkEntry, '--help'], { encoding: 'utf8' });
    expect(res.status).toBe(0);
    // Before the fix this was '' — the entry guard mismatched and the
    // process exited without running main() at all.
    expect(res.stderr).toContain('--server');
  });
});
