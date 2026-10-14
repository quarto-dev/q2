/**
 * Entry point of the embedded driver. q2 pipes the built `dist/ts-bundle.mjs`
 * to `node -` and passes the job as JSON in `Q2_TS_BUNDLE_JOB`
 * (see `crates/quarto-core/src/extension/build.rs`).
 *
 * Problems go to stderr as `error: ...`; the exit status is 1 on failure.
 */

import { bundle, type BundleJob } from "./bundle.js";

const JOB_ENV = "Q2_TS_BUNDLE_JOB";

function report(e: unknown): void {
  const err = e as { message?: string; frame?: string; loc?: { file?: string; line: number; column: number } };
  const where = err.loc?.file ? ` (${err.loc.file}:${err.loc.line}:${err.loc.column})` : "";
  console.error(`error: ${err.message ?? String(e)}${where}`);
  if (err.frame) console.error(err.frame);
}

const raw = process.env[JOB_ENV];
if (!raw) {
  console.error(`error: ${JOB_ENV} is not set; this script is run by \`q2 call build-ts-extension\``);
  process.exitCode = 2;
} else {
  try {
    await bundle(JSON.parse(raw) as BundleJob);
  } catch (e) {
    report(e);
    process.exitCode = 1;
  }
}
