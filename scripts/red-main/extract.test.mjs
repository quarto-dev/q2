// node --test scripts/red-main/*.test.mjs
//
// Fixtures are real `gh run view --log-failed` output from red main runs,
// trimmed to the failure windows (raw logs are 5k–17k lines).

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  MASS_FAILURE_THRESHOLD,
  issueKey,
  normalizeJob,
  parseFailedLog,
  planIssues,
  repoRelative,
  shorten,
  splitLogLine,
  stripAnsi,
} from './extract.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const fixture = (name) => readFileSync(join(here, 'fixtures', name), 'utf8');

const ids = (log) =>
  [...parseFailedLog(log).values()].flatMap((j) => j.tests.map((t) => `${t.runner}: ${t.id}`));

describe('line handling', () => {
  it('strips both real and caret-notation escapes', () => {
    assert.equal(stripAnsi('\x1b[31mred\x1b[39m ^[[41m^[[1m FAIL ^[[22m'), 'red  FAIL ');
  });

  it('splits job, drops step and timestamp', () => {
    assert.deepEqual(
      splitLogLine('Run test suite (macos-latest)\tUNKNOWN STEP\t2026-11-11T17:33:53.2245290Z         FAIL [  1.0s] a b'),
      { job: 'Run test suite (macos-latest)', text: '        FAIL [  1.0s] a b' },
    );
  });

  it('keeps continuation lines that carry no timestamp', () => {
    assert.equal(splitLogLine('J\tS\t ❯ src/x.test.ts:1:2').text, ' ❯ src/x.test.ts:1:2');
  });

  it('separates the matrix variant from the job name', () => {
    assert.deepEqual(normalizeJob('Hub-Client E2E Tests (chromium)'), { base: 'Hub-Client E2E Tests', variant: 'chromium' });
    assert.deepEqual(normalizeJob('WASM Tests'), { base: 'WASM Tests', variant: null });
  });

  it('shortens titles from the middle, keeping the file and the test name', () => {
    const s = shorten('ts-packages/quarto-hub-mcp/src/exit-drain.test.ts > exit drains outbound sync (bd-10deu8h4) > create_project then immediate stdin EOF must not lose the created docs');
    assert.equal(s.length, 110);
    assert.match(s, /^ts-packages\/quarto-hub-mcp\//);
    assert.match(s, /must not lose the created docs$/);
  });

  it('makes runner paths repo-relative on both OSes', () => {
    assert.equal(repoRelative('/Users/runner/work/q2/q2/ts-packages/quarto-hub-mcp'), 'ts-packages/quarto-hub-mcp');
    assert.equal(repoRelative('/home/runner/work/q2/q2/hub-client/'), 'hub-client');
  });
});

describe('parseFailedLog on real logs', () => {
  it('vitest: qualifies test files with the package from the RUN header', () => {
    assert.deepEqual(ids(fixture('vitest-hub-mcp-timeouts.log')), [
      'vitest: ts-packages/quarto-hub-mcp/src/exit-drain.test.ts > exit drains outbound sync (bd-10deu8h4) > create_project then immediate stdin EOF must not lose the created docs',
      'vitest: ts-packages/quarto-hub-mcp/src/upload-local-file.test.ts > edit upload > handles a full-size image without a conversation-sized payload',
      'vitest: ts-packages/quarto-hub-mcp/src/upload-local-file.test.ts > edit upload > accepts a file at exactly the size cap and refuses one byte more',
    ]);
  });

  it('vitest: the excerpt carries the error', () => {
    const [first] = parseFailedLog(fixture('vitest-hub-mcp-timeouts.log')).get('Workspace TS suites (macos-latest)').tests;
    assert.match(first.excerpt, /Timeout waiting for response to request 2/);
  });

  it('nextest: one entry although the FAIL line repeats in the summary', () => {
    assert.deepEqual(ids(fixture('nextest-jupyter-goldens.log')), [
      'nextest: quarto-core::integration jupyter_markdown_goldens::jupyter_markdown_matches_goldens',
    ]);
  });

  it('playwright: keys on the project, spec and title, without line numbers', () => {
    assert.deepEqual(ids(fixture('playwright-webkit-pdf-viewer.log')), [
      'playwright: [webkit] e2e/pandoc-pdf-viewer.harness.spec.ts › PDF viewer › the preview pane: it compiles on mount, an edit recompiles in place and keeps the reader where they were',
    ]);
  });

  it('playwright: reports the failed tests and not the flaky ones', () => {
    // The run printed 10 numbered entries: 2 failed + 8 flaky.
    assert.deepEqual(ids(fixture('playwright-failed-and-flaky.log')), [
      'playwright: [chromium] e2e/smoke-all.spec.ts › smoke-all E2E tests › toc-containers/content-visible-span-kept.qmd [html]',
      'playwright: [chromium] e2e/smoke-all.spec.ts › smoke-all E2E tests › toc/toc-expand-depth.qmd [html]',
    ]);
  });

  it('playwright: the excerpt comes from the numbered detail section', () => {
    const [t] = parseFailedLog(fixture('playwright-webkit-pdf-viewer.log')).get('Hub-Client E2E Tests (chromium)').tests;
    assert.match(t.excerpt, /^\s+1\) \[webkit\]/);
    assert.match(t.excerpt, /Expected: 0\n\s+Received: 2/);
    // …and stops at the step boundary instead of running into the next step.
    assert.doesNotMatch(t.excerpt, /##\[group\]/);
  });

  it('build break: no tests', () => {
    assert.deepEqual(ids(fixture('build-break-no-tests.log')), []);
  });

  it('vitest: unhandled errors key on the originating file', () => {
    const log = [
      'J (ubuntu-latest)\tS\t2026-01-01T00:00:00Z  RUN  v4.1.8 /home/runner/work/q2/q2/hub-client',
      'J (ubuntu-latest)\tS\t2026-01-01T00:00:01Z EnvironmentTeardownError: Closing rpc while "onUserConsoleLog" was pending',
      'J (ubuntu-latest)\tS\t2026-01-01T00:00:01Z This error originated in "src/components/Foo.integration.test.tsx" test file. It doesn\'t mean the error was thrown inside the file itself',
    ].join('\n');
    assert.deepEqual(ids(log), ['vitest: hub-client/src/components/Foo.integration.test.tsx (unhandled error)']);
  });
});

describe('planIssues', () => {
  const failedJob = (name, steps = []) => ({
    name,
    conclusion: 'failure',
    steps: steps.map((s) => ({ name: s, conclusion: 'failure' })),
  });

  it('one issue per failing test, titled for humans, keyed stably', () => {
    const plans = planIssues({
      workflow: 'TS Test Suite',
      jobs: [failedJob('Workspace TS suites (macos-latest)', ['Run quarto-hub-mcp tests'])],
      log: fixture('vitest-hub-mcp-timeouts.log'),
    });
    assert.equal(plans.length, 3);
    assert.equal(plans[0].kind, 'test');
    assert.match(plans[0].title, /^\[red main\] vitest: ts-packages\/quarto-hub-mcp\/src\/exit-drain/);
    assert.match(plans[0].title, /must not lose the created docs$/);
    assert.ok(plans[0].title.length <= 140);
    assert.deepEqual(plans[0].variants, ['macos-latest']);
    assert.equal(plans[0].key, issueKey('vitest', plans[0].id));
  });

  it('the key ignores OS and job, so both matrix legs land in one issue', () => {
    const log = fixture('nextest-jupyter-goldens.log');
    const linux = log.replaceAll('Run test suite (macos-latest)', 'Run test suite (ubuntu-latest)');
    const plans = planIssues({
      workflow: 'Test Suite',
      jobs: [failedJob('Run test suite (macos-latest)'), failedJob('Run test suite (ubuntu-latest)')],
      log: `${log}\n${linux}`,
    });
    assert.equal(plans.length, 1);
    assert.deepEqual(plans[0].variants, ['macos-latest', 'ubuntu-latest']);
  });

  it('falls back to workflow › job › failed step when no test is identified', () => {
    const [plan] = planIssues({
      workflow: 'TS Test Suite',
      jobs: [failedJob('Run test suite (ubuntu-latest)', ['Build WASM'])],
      log: fixture('build-break-no-tests.log').replaceAll(/^[^\t]+\t/gm, 'Run test suite (ubuntu-latest)\t'),
    });
    assert.equal(plan.kind, 'job');
    assert.equal(plan.title, '[red main] TS Test Suite › Run test suite › Build WASM');
    assert.match(plan.excerpt, /failed to select a version/);
  });

  it(`collapses more than ${MASS_FAILURE_THRESHOLD} failures in one job into one issue`, () => {
    const job = 'E2E (chromium)';
    const n = MASS_FAILURE_THRESHOLD + 1;
    const log = [
      `${job}\tS\t2026-01-01T00:00:00Z   ${n} failed`,
      ...Array.from({ length: n }, (_, i) => `${job}\tS\t2026-01-01T00:00:00Z     [chromium] › e2e/a.spec.ts:1:1 › t${i}`),
      `${job}\tS\t2026-01-01T00:00:00Z   3 passed (1.0s)`,
    ].join('\n');
    const plans = planIssues({ workflow: 'Hub-Client E2E Tests', jobs: [failedJob(job)], log });
    assert.equal(plans.length, 1);
    assert.equal(plans[0].kind, 'mass');
    assert.equal(plans[0].tests.length, n);
    assert.equal(plans[0].key, issueKey('mass', 'Hub-Client E2E Tests', 'E2E'));
  });

  it('ignores jobs that did not fail', () => {
    const plans = planIssues({
      workflow: 'Test Suite',
      jobs: [{ name: 'Run test suite (macos-latest)', conclusion: 'success', steps: [] }],
      log: fixture('nextest-jupyter-goldens.log'),
    });
    assert.deepEqual(plans, []);
  });
});
