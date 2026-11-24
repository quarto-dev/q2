// Failure extraction for the red-main issue filer (bd-red-main-issues-fs4r6cp0).
//
// Pure functions only: text in, failures out. All GitHub I/O lives in
// file-issues.mjs, so everything here is unit-tested against real (trimmed)
// CI logs in ./fixtures.
//
// Input is the output of `gh run view <id> --log-failed`: one line per log
// line, `<job>\t<step>\t<timestamp> <text>`. Two quirks of that format:
//   - ESC is printed as the two characters `^[`, so colour codes arrive as
//     literal `^[[31m` text (and as real `\x1b[31m` from other sources).
//   - The step column is `UNKNOWN STEP` for most runs, so failed step names
//     come from the jobs JSON instead (see `planIssues`).
//
// See claude-notes/plans/2026-11-23-red-main-issue-filer.md.

import { createHash } from 'node:crypto';

/** More distinct failing tests than this in one job → one "mass failure"
 * issue for the job, not one issue per test. */
export const MASS_FAILURE_THRESHOLD = 5;

/** Lines of log kept after a failure's first mention, for the issue excerpt. */
const EXCERPT_LINES = 30;

const ANSI = /(?:\x1b|\^\[)\[[0-9;?]*[A-Za-z]/g;

export function stripAnsi(s) {
  return s.replace(ANSI, '');
}

/** Split one `--log-failed` line into { job, text }, without the timestamp. */
export function splitLogLine(line) {
  const first = line.indexOf('\t');
  const second = first === -1 ? -1 : line.indexOf('\t', first + 1);
  if (second === -1) return { job: null, text: stripAnsi(line) };
  const job = line.slice(0, first);
  const text = stripAnsi(line.slice(second + 1)).replace(
    /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z ?/,
    '',
  );
  return { job, text };
}

/** `Run test suite (macos-latest)` → { base: 'Run test suite', variant: 'macos-latest' }. */
export function normalizeJob(name) {
  const m = /^(.*?)\s*\(([^()]*)\)\s*$/.exec(name);
  return m ? { base: m[1], variant: m[2] } : { base: name, variant: null };
}

/** A repo-relative path from an absolute runner path (`/home/runner/work/q2/q2/x` → `x`). */
export function repoRelative(p) {
  return p.replace(/^.*?\/work\/[^/]+\/[^/]+\/?/, '').replace(/\/$/, '');
}

const NEXTEST_FAIL =
  /^\s+(?:FAIL|SIGSEGV|SIGABRT|SIGBUS|SIGKILL|TIMEOUT|LEAK-FAIL) \[\s*[\d.]+s\]\s+(?:\(\s*\d+\/\d+\)\s+)?(\S+)\s+(\S+)\s*$/;
const VITEST_RUN = /^\s*RUN\s+v\d\S*\s+(\/\S+)\s*$/;
const VITEST_FAIL = /^\s*FAIL\s+(?:\|[^|]+\|\s+)?(\S+\.(?:test|spec)\.[cm]?[jt]sx?)\s*(?:>\s*(.*?)|\[\s*\S+\s*\])?\s*$/;
const VITEST_UNHANDLED = /This error originated in "(\S+?)" test file/;
const PW_COUNT = /^\s{1,4}(\d+) (failed|flaky|skipped|passed|interrupted|did not run)\b/;
const PW_ENTRY = /^\s+\[([\w-]+)\] › (\S+?):\d+:\d+ › (.*?)[\s─]*$/;

/**
 * Parse a whole `--log-failed` dump.
 *
 * @returns {Map<string, {tests: Array<{runner: string, id: string, excerpt: string}>}>}
 *   keyed by the raw job name. Only jobs with at least one log line appear.
 */
export function parseFailedLog(log) {
  const jobs = new Map();
  const lines = log.split(/\r?\n/).map(splitLogLine);

  const ensure = (job) => {
    if (!jobs.has(job)) jobs.set(job, { tests: [], seen: new Set(), vitestRoot: null, pwBlock: null });
    return jobs.get(job);
  };

  const excerptFrom = (i, job) => {
    const out = [];
    for (let k = i; k < lines.length && out.length < EXCERPT_LINES; k++) {
      if (lines[k].job !== job) continue;
      // Stop at the step boundary rather than run into the next step.
      if (k > i && /^##\[(group|endgroup|error)\]/.test(lines[k].text)) break;
      out.push(lines[k].text);
    }
    return out.join('\n').trimEnd();
  };

  const add = (st, job, runner, id, i) => {
    const k = `${runner}\0${id}`;
    if (st.seen.has(k)) return;
    st.seen.add(k);
    st.tests.push({ runner, id, excerpt: excerptFrom(i, job) });
  };

  lines.forEach(({ job, text }, i) => {
    if (job === null) return;
    const st = ensure(job);
    let m;

    // Playwright prints its numbered details for failed *and* flaky tests,
    // so only the end-of-run summary block under "N failed" is trusted.
    if ((m = PW_COUNT.exec(text))) {
      st.pwBlock = m[2] === 'failed' ? 'failed' : null;
      return;
    }
    if (st.pwBlock === 'failed') {
      if ((m = PW_ENTRY.exec(text))) {
        const [, project, spec, title] = m;
        const id = `[${project}] ${spec} › ${title.trim()}`;
        // Excerpt from the numbered detail section when present.
        const detail = lines.findIndex(
          (l, k) => k < i && l.job === job && l.text.includes(`] › ${spec}:`) && l.text.includes(title.trim()),
        );
        add(st, job, 'playwright', id, detail === -1 ? i : detail);
        return;
      }
      st.pwBlock = null;
    }

    if ((m = NEXTEST_FAIL.exec(text))) {
      add(st, job, 'nextest', `${m[1]} ${m[2]}`, i);
      return;
    }
    if ((m = VITEST_RUN.exec(text))) {
      st.vitestRoot = repoRelative(m[1]);
      return;
    }
    if ((m = VITEST_FAIL.exec(text))) {
      const file = st.vitestRoot ? `${st.vitestRoot}/${m[1]}` : m[1];
      const id = m[2] ? `${file} > ${m[2]}` : file;
      add(st, job, 'vitest', id, i);
      return;
    }
    if ((m = VITEST_UNHANDLED.exec(text))) {
      const file = st.vitestRoot ? `${st.vitestRoot}/${m[1]}` : m[1];
      add(st, job, 'vitest', `${file} (unhandled error)`, i);
    }
  });

  for (const st of jobs.values()) {
    delete st.seen;
    delete st.vitestRoot;
    delete st.pwBlock;
  }
  return jobs;
}

export function issueKey(...parts) {
  return createHash('sha1').update(parts.join('\0')).digest('hex').slice(0, 12);
}

/** Cut from the middle: the start names the file, the end names the test,
 * and both are what a reader scans an issue list for. */
export function shorten(s, n = 110) {
  if (s.length <= n) return s;
  const head = Math.floor((n - 1) * 0.4);
  return `${s.slice(0, head)}…${s.slice(s.length - (n - 1 - head))}`;
}

/**
 * Turn one failed run into the issues it should open or update.
 *
 * @param {object} run
 * @param {string} run.workflow       workflow name
 * @param {Array<{name: string, conclusion: string, steps?: Array<{name: string, conclusion: string}>}>} run.jobs
 * @param {string} run.log            `gh run view --log-failed` output
 * @returns {Array<{key, kind, title, workflow, job, variant, runner?, id?, tests?, steps?, excerpt}>}
 */
export function planIssues({ workflow, jobs, log }) {
  const parsed = parseFailedLog(log);
  const plans = [];

  for (const job of jobs) {
    if (job.conclusion !== 'failure') continue;
    const { base, variant } = normalizeJob(job.name);
    const tests = parsed.get(job.name)?.tests ?? [];
    const failedSteps = (job.steps ?? []).filter((s) => s.conclusion === 'failure').map((s) => s.name);

    if (tests.length > MASS_FAILURE_THRESHOLD) {
      plans.push({
        key: issueKey('mass', workflow, base),
        kind: 'mass',
        title: `[red main] ${workflow} › ${base}: mass failure`,
        workflow, job: base, variant,
        tests: tests.map((t) => `${t.runner}: ${t.id}`),
        excerpt: tests[0].excerpt,
      });
    } else if (tests.length > 0) {
      for (const t of tests) {
        plans.push({
          key: issueKey(t.runner, t.id),
          kind: 'test',
          title: `[red main] ${t.runner}: ${shorten(t.id)}`,
          workflow, job: base, variant,
          runner: t.runner, id: t.id,
          excerpt: t.excerpt,
        });
      }
    } else {
      // No test identified: a build break, infra failure, or a runner we
      // don't parse yet. Key on where it failed instead of what.
      const step = failedSteps[0] ?? '(unknown step)';
      plans.push({
        key: issueKey('job', workflow, base, step),
        kind: 'job',
        title: `[red main] ${base === workflow ? workflow : `${workflow} › ${base}`} › ${shorten(step, 60)}`,
        workflow, job: base, variant,
        steps: failedSteps,
        excerpt: errorExcerpt(log, job.name),
      });
    }
  }

  // The same test failing in two matrix legs of one run is one issue.
  const byKey = new Map();
  for (const p of plans) {
    const prev = byKey.get(p.key);
    if (prev) prev.variants.add(p.variant);
    else byKey.set(p.key, { ...p, variants: new Set([p.variant]) });
  }
  return [...byKey.values()].map(({ variant, variants, ...p }) => ({
    ...p,
    variants: [...variants].filter(Boolean).sort(),
  }));
}

/** The last few `error`-looking lines of a job, for issues with no test id. */
export function errorExcerpt(log, jobName) {
  const lines = log
    .split(/\r?\n/)
    .map(splitLogLine)
    .filter((l) => l.job === jobName)
    .map((l) => l.text);
  const hits = [];
  lines.forEach((t, i) => {
    if (/(^|\s)(error(\[E\d+\])?:|##\[error\]|Error:|panicked at)/.test(t)) hits.push(i);
  });
  if (hits.length === 0) return lines.slice(-EXCERPT_LINES).join('\n').trimEnd();
  const start = Math.max(0, hits[0] - 3);
  return lines.slice(start, start + EXCERPT_LINES).join('\n').trimEnd();
}
