/**
 * Playwright global setup for q2-preview-spa.
 *
 * Asserts the `target/debug/q2` binary exists (we don't want each
 * E2E run paying the cargo-compile cost — `cargo xtask verify --e2e`
 * builds it first; for local iteration the dev has typically run
 * `cargo build -p quarto --bin q2` already). If the binary is
 * missing we fail loudly with a copy-pasteable command rather than
 * silently building.
 *
 * Per-test server lifecycle is handled inside the test file (each
 * test gets its own fresh fixture project + server) so we don't need
 * teardown of a shared instance here.
 */

import { access } from 'node:fs/promises';
import path from 'node:path';

const REPO_ROOT = path.resolve(import.meta.dirname, '..', '..', '..');
const Q2_BINARY = path.join(REPO_ROOT, 'target', 'debug', 'q2');

export default async function globalSetup() {
  try {
    await access(Q2_BINARY);
  } catch {
    throw new Error(
      `q2 binary not found at ${Q2_BINARY}\n` +
        `Run: cargo build -p quarto --bin q2\n` +
        `(or use 'cargo xtask verify --e2e' which builds it first)`,
    );
  }
}
