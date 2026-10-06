/**
 * Build the `wasm-qmd-parser` nodejs package (the CAP-11 parser) and keep
 * it fresh. Shared by the vitest global setup (build-if-stale before tests)
 * and bundle.mjs (build-if-stale before staging into dist-bundle).
 *
 * Freshness is a content stamp over the git state of every crate that can
 * change the wasm bytes — the parser crate, the shared shim crates, and
 * pampa's whole tree — not a mere existence check, so a pampa edit can
 * never leave a silently stale parser in tests or in the bundle (the
 * stale-embed trap claude-notes/instructions/preview-spa-rebuild.md
 * documents for the other wasm artifacts).
 *
 * Build requirements mirror crates/wasm-qmd-parser/AGENTS.md: wasm-pack on
 * PATH, a wasm32-capable clang (Homebrew LLVM on macOS), and the
 * wasm-sysroot CFLAGS. The crate's committed .cargo/config.toml now pins
 * the panic strategy, so no RUSTFLAGS override is needed here.
 */

import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { platform } from 'node:os';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(here, '..', '..', '..');
const crateDir = join(repoRoot, 'crates', 'wasm-qmd-parser');
const pkgDir = join(crateDir, 'pkg-nodejs');
const stampFile = join(pkgDir, '.build-stamp');

/** Crates whose source changes alter the parser wasm. */
const STAMP_PATHS = [
  'crates/wasm-qmd-parser',
  'crates/wasm-c-shim',
  'crates/wasm-printf-fmt',
  'crates/pampa',
  'crates/tree-sitter-qmd',
  'crates/tree-sitter-doctemplate',
  'crates/tree-sitter-language-wasm-shim',
];

const STAMP_VERSION = 'nodejs-release-v1';

function findLlvmBin() {
  if (platform() !== 'darwin') return null;
  for (const loc of ['/opt/homebrew/opt/llvm/bin', '/usr/local/opt/llvm/bin']) {
    if (existsSync(join(loc, 'clang'))) return loc;
  }
  throw new Error(
    'Homebrew LLVM not found (Apple clang cannot target wasm32). Install with: brew install llvm',
  );
}

function computeStamp() {
  try {
    const head = execFileSync('git', ['rev-parse', 'HEAD'], {
      cwd: repoRoot,
      encoding: 'utf8',
    }).trim();
    const dirty = execFileSync('git', ['status', '--porcelain', '--', ...STAMP_PATHS], {
      cwd: repoRoot,
      encoding: 'utf8',
    });
    return createHash('sha1').update(STAMP_VERSION).update(head).update(dirty).digest('hex');
  } catch {
    // Not a git checkout: freshness cannot be proven — treat as always-stale
    // when the pkg is missing, always-fresh when it exists (npm/sdist case).
    return null;
  }
}

function pkgComplete() {
  return (
    existsSync(join(pkgDir, 'wasm_qmd_parser_bg.wasm')) &&
    existsSync(join(pkgDir, 'wasm_qmd_parser.js')) &&
    existsSync(join(pkgDir, 'package.json'))
  );
}

function build() {
  const llvmBin = findLlvmBin();
  const env = {
    ...process.env,
    ...(llvmBin ? { PATH: `${llvmBin}:${process.env.PATH}` } : {}),
    CFLAGS_wasm32_unknown_unknown:
      `-I${join(crateDir, 'wasm-sysroot')} -Wbad-function-cast ` +
      '-Wcast-function-type -fno-builtin -DHAVE_ENDIAN_H',
  };
  const res = spawnSync(
    'wasm-pack',
    ['build', '--target', 'nodejs', '--release', '--out-dir', 'pkg-nodejs'],
    { cwd: crateDir, env, stdio: 'inherit' },
  );
  if (res.error || res.status !== 0) {
    throw new Error(
      'wasm-pack build failed for wasm-qmd-parser (see output above). ' +
        'Requires: wasm-pack (cargo install wasm-pack), a wasm32-capable ' +
        'clang (brew install llvm on macOS), nightly rustup toolchain.',
    );
  }
}

/**
 * Ensure the nodejs parser pkg exists and is fresh. Returns the pkg dir.
 * @param {{force?: boolean, quiet?: boolean}} opts
 */
export function ensureQmdParserPkg(opts = {}) {
  const stamp = computeStamp();
  const fresh =
    !opts.force &&
    pkgComplete() &&
    stamp !== null &&
    existsSync(stampFile) &&
    readFileSync(stampFile, 'utf8').trim() === stamp;
  if (fresh) {
    if (!opts.quiet) console.log(`qmd-parser pkg fresh (${pkgDir})`);
    return pkgDir;
  }
  if (!pkgComplete() || stamp === null) {
    if (!opts.quiet) console.log('building qmd-parser pkg (missing or provenance unknown)...');
  } else if (!opts.quiet) {
    console.log('rebuilding qmd-parser pkg (crate sources changed)...');
  }
  build();
  if (stamp !== null) writeFileSync(stampFile, stamp + '\n');
  return pkgDir;
}
