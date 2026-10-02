/**
 * The P7 docx golden fixtures as the pandoc.wasm parity net uses them (pandoc-host H3).
 * Node-only: reads `crates/quarto-core/tests/fixtures/pandoc-goldens` from disk.
 *
 * The fixture list is `fixtures.json`, the committed export of
 * `quarto_output_extract::FIXTURES` (a Rust test fails when they drift). The reference for
 * each fixture is the extraction snapshot the Rust golden test asserts against; the one
 * accepted-divergent fixture (mermaid) compares against the native q2 extraction instead
 * (`parity/`, see DIVERGENCES.md).
 */
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { REPO } from './pandocRecordings';

export interface GoldenFixture {
  qmd: string;
  resources: string[];
}

export const FIXTURES_ROOT = path.join(REPO, 'crates/quarto-core/tests/fixtures/pandoc-goldens');
const SNAPSHOTS = path.join(REPO, 'crates/quarto-core/tests/integration/snapshots');

export function goldenFixtures(): GoldenFixture[] {
  return JSON.parse(readFileSync(path.join(FIXTURES_ROOT, 'fixtures.json'), 'utf8'));
}

/** `golden_snapshot_name` (quarto-output-extract): the TS twin; a missing snapshot fails the test. */
export function snapshotName(qmd: string, format: string): string {
  return `${qmd.replace(/\.qmd$/, '').replace(/[/-]/g, '_')}__${format}`;
}

/** The fixture's files as (project-relative path, bytes): the qmd first, then its resources. */
export function fixtureFiles(f: GoldenFixture): { path: string; bytes: Uint8Array }[] {
  return [f.qmd, ...f.resources].map((rel) => {
    const b = readFileSync(path.join(FIXTURES_ROOT, rel));
    return { path: rel, bytes: new Uint8Array(b.buffer, b.byteOffset, b.byteLength).slice() };
  });
}

/** The extraction text the wasm output must equal, and where it came from. */
export function referenceFor(f: GoldenFixture, format = 'docx'): { text: string; source: string } {
  const name = snapshotName(f.qmd, format);
  const native = path.join(FIXTURES_ROOT, 'parity', `${name}.txt`);
  if (existsSync(native)) return { text: readFileSync(native, 'utf8').trimEnd(), source: native };
  const snap = path.join(SNAPSHOTS, `${name}.snap`);
  // insta: `---\n<header>\n---\n<body>`
  const raw = readFileSync(snap, 'utf8').replace(/\r\n/g, '\n');
  const body = raw.split('\n---\n').slice(1).join('\n---\n');
  return { text: body.trimEnd(), source: snap };
}

/** The `quarto-output-extract` binary: `QUARTO_OUTPUT_EXTRACT`, else a release or debug build. */
export function findExtractor(): string | null {
  const exe = process.platform === 'win32' ? '.exe' : '';
  const candidates = [
    process.env.QUARTO_OUTPUT_EXTRACT,
    path.join(REPO, 'target/release/quarto-output-extract' + exe),
    path.join(REPO, 'target/debug/quarto-output-extract' + exe),
  ];
  return candidates.find((c): c is string => !!c && existsSync(c)) ?? null;
}

/** The extractor's text for a file (`.typ` is verbatim; the CLI reads it the same way). */
export function extractText(extractor: string, file: string): string {
  return execFileSync(extractor, ['extract', file], { maxBuffer: 1 << 28 }).toString('utf8');
}
