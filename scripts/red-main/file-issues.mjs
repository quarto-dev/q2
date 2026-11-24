#!/usr/bin/env node
// File (or update) one GitHub issue per test that failed on main
// (bd-red-main-issues-fs4r6cp0).
//
//   node scripts/red-main/file-issues.mjs [--dry-run] [--repo owner/name] --run <id> [--run <id>...]
//   node scripts/red-main/file-issues.mjs [--dry-run] [--repo owner/name] --since 2026-10-24
//
// We never merge red, so every red run on main is a signal. Each failing
// test (see extract.mjs for how tests are identified) maps to one issue,
// found again through a `<!-- red-main-key: … -->` marker in its body:
//
//   no issue            → open one
//   open issue          → comment, add a row to its occurrence table
//   closed ≤ 30 days    → reopen + comment ("recurred after close")
//   closed > 30 days    → open a new issue that links the old one
//
// --dry-run reads but never writes; it prints (and, under Actions, writes to
// the job summary) what it would do. With --since it replays every failed
// main run since the date, oldest first, carrying state between runs, so a
// dry run shows the issue list a backfill would produce.
//
// Needs `gh` on PATH, authenticated with issues:write (actions:read for logs).
// See claude-notes/plans/2026-11-23-red-main-issue-filer.md.

import { execFileSync } from 'node:child_process';
import { appendFileSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { planIssues } from './extract.mjs';

/** Workflows whose red main runs are filed. Test-running ones only. */
export const WATCHED_WORKFLOWS = ['Test Suite', 'TS Test Suite', 'Hub-Client E2E Tests'];
export const LABELS = ['red-main', 'testing'];
const REOPEN_WINDOW_DAYS = 30;
/** Recent red-main issues scanned for keys (newest first). */
const ISSUE_SCAN_LIMIT = 300;
const EXCERPT_MAX_CHARS = 4000;
const MASS_LIST_MAX = 50;

const KEY_MARKER = (key) => `<!-- red-main-key: ${key} -->`;
const OCC_START = '<!-- red-main-occurrences -->';
const OCC_END = '<!-- /red-main-occurrences -->';

// ---------------------------------------------------------------------------
// Rendering (pure; exported for tests)
// ---------------------------------------------------------------------------

export function keyOf(body) {
  return /<!-- red-main-key: ([0-9a-f]+) -->/.exec(body ?? '')?.[1] ?? null;
}

/** `| date | run | commit | where |` row; the run cell doubles as an idempotency key. */
export function occurrenceRow(run, plan) {
  const where = plan.variants.length ? plan.variants.join(', ') : plan.job;
  return `| ${run.createdAt.slice(0, 10)} | [${run.id} #${run.attempt}](${run.url}) | \`${run.headSha.slice(0, 9)}\` | ${where} |`;
}

export function hasOccurrence(body, run) {
  return (body ?? '').includes(`[${run.id} #${run.attempt}](`);
}

/** Insert a row at the end of the occurrence table. */
export function addOccurrence(body, row) {
  const end = body.indexOf(OCC_END);
  if (end === -1) return `${body}\n\n${row}`;
  return `${body.slice(0, end)}${row}\n${body.slice(end)}`;
}

export function countOccurrences(body) {
  const start = body.indexOf(OCC_START);
  const end = body.indexOf(OCC_END);
  if (start === -1 || end === -1) return 0;
  return body.slice(start, end).split('\n').filter((l) => /^\| \d{4}-/.test(l)).length;
}

function fence(text) {
  const t = text.length > EXCERPT_MAX_CHARS ? `${text.slice(0, EXCERPT_MAX_CHARS)}\n… (truncated)` : text;
  return `\`\`\`\`text\n${t.replaceAll('````', '``​``')}\n\`\`\`\``;
}

function subject(plan) {
  switch (plan.kind) {
    case 'test':
      return `| Runner | ${plan.runner} |\n| Test | \`${plan.id}\` |`;
    case 'mass':
      return `| Failing tests | ${plan.tests.length} in one job (collapsed into this issue) |`;
    default:
      return `| Failed step | ${plan.steps.length ? plan.steps.map((s) => `\`${s}\``).join(', ') : '(unknown)'} |\n| Note | No failing test was identified: likely a build or infrastructure failure. |`;
  }
}

export function issueBody(plan, run, { previous } = {}) {
  const parts = [
    KEY_MARKER(plan.key),
    `This failed on \`main\`. We never merge red, so a failure on main means a flake, environment drift, a cross-PR interaction or a regression. Each new failure is added below.`,
    '',
    '| | |',
    '|---|---|',
    `| Workflow | ${plan.workflow} › ${plan.job} |`,
    subject(plan),
  ];
  if (previous) parts.push(`| Earlier issue | #${previous} |`);
  if (plan.kind === 'mass') {
    const shown = plan.tests.slice(0, MASS_LIST_MAX).map((t) => `- \`${t}\``);
    if (plan.tests.length > MASS_LIST_MAX) shown.push(`- … and ${plan.tests.length - MASS_LIST_MAX} more`);
    parts.push('', '### Failing tests (first occurrence)', '', ...shown);
  }
  parts.push(
    '',
    '### Occurrences',
    '',
    OCC_START,
    '| Date | Run (attempt) | Commit | Where |',
    '|---|---|---|---|',
    occurrenceRow(run, plan),
    OCC_END,
    '',
    '<details><summary>First failure</summary>',
    '',
    fence(plan.excerpt),
    '',
    '</details>',
    '',
    '---',
    '_Filed by `.github/workflows/red-main.yml` (`scripts/red-main/`). Triage with `/triage <issue>` and note the braid strand here. Closing this issue reopens it if the test fails again within 30 days._',
  );
  return parts.join('\n');
}

export function commentBody(plan, run, { reopened } = {}) {
  const where = plan.variants.length ? ` (${plan.variants.join(', ')})` : '';
  const lead = reopened
    ? `**Recurred after close.** Failed again in [run ${run.id}, attempt ${run.attempt}](${run.url}) on \`${run.headSha.slice(0, 9)}\`${where}.`
    : `Failed again in [run ${run.id}, attempt ${run.attempt}](${run.url}) on \`${run.headSha.slice(0, 9)}\`${where}.`;
  return `${lead}\n\n<details><summary>Failure</summary>\n\n${fence(plan.excerpt)}\n\n</details>`;
}

/**
 * Decide what to do for one plan given the known issues (pure).
 * @returns {{action: 'create'|'comment'|'reopen'|'skip', issue?: object, previous?: number}}
 */
export function decide(plan, run, issues, now = new Date()) {
  const matches = issues.filter((i) => keyOf(i.body) === plan.key).sort((a, b) => b.number - a.number);
  const open = matches.find((i) => i.state === 'OPEN');
  if (open) return hasOccurrence(open.body, run) ? { action: 'skip', issue: open } : { action: 'comment', issue: open };
  const latest = matches[0];
  if (!latest) return { action: 'create' };
  if (hasOccurrence(latest.body, run)) return { action: 'skip', issue: latest };
  const ageDays = (now - new Date(latest.closedAt)) / 86_400_000;
  if (latest.closedAt && ageDays <= REOPEN_WINDOW_DAYS) return { action: 'reopen', issue: latest };
  return { action: 'create', previous: latest.number };
}

// ---------------------------------------------------------------------------
// GitHub I/O
// ---------------------------------------------------------------------------

function gh(args, { input } = {}) {
  return execFileSync('gh', args, { encoding: 'utf8', maxBuffer: 512 * 1024 * 1024, input, stdio: ['pipe', 'pipe', 'pipe'] });
}

function ghJson(args) {
  return JSON.parse(gh(args));
}

function withBodyFile(text, fn) {
  const dir = mkdtempSync(join(tmpdir(), 'red-main-'));
  const file = join(dir, 'body.md');
  writeFileSync(file, text);
  try {
    return fn(file);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

function loadRun(repo, id) {
  const r = ghJson(['run', 'view', String(id), '--repo', repo, '--json',
    'databaseId,attempt,headSha,headBranch,event,workflowName,url,createdAt,conclusion,jobs']);
  return {
    id: r.databaseId, attempt: r.attempt, headSha: r.headSha, headBranch: r.headBranch,
    event: r.event, workflow: r.workflowName, url: r.url, createdAt: r.createdAt,
    conclusion: r.conclusion, jobs: r.jobs,
  };
}

/** The failed jobs' logs, or null when GitHub has none (expired, or a job
 * that died before logging). Without a log every failed job still gets a
 * workflow › job › step issue. */
function loadLog(repo, run) {
  try {
    return gh(['run', 'view', String(run.id), '--repo', repo, '--attempt', String(run.attempt), '--log-failed']);
  } catch (e) {
    console.error(`run ${run.id}: no log (${(e.stderr?.toString() || e.message).trim()})`);
    return null;
  }
}

function loadIssues(repo) {
  return ghJson(['issue', 'list', '--repo', repo, '--label', LABELS[0], '--state', 'all',
    '--limit', String(ISSUE_SCAN_LIMIT), '--json', 'number,state,closedAt,title,body,url']);
}

function failedMainRunsSince(repo, since) {
  const runs = [];
  for (const wf of WATCHED_WORKFLOWS) {
    const list = ghJson(['run', 'list', '--repo', repo, '--workflow', wf, '--branch', 'main',
      '--status', 'failure', '--created', `>=${since}`, '--limit', '200',
      '--json', 'databaseId,createdAt']);
    runs.push(...list);
  }
  return runs.sort((a, b) => a.createdAt.localeCompare(b.createdAt)).map((r) => r.databaseId);
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const opts = { dryRun: false, repo: process.env.GITHUB_REPOSITORY ?? 'quarto-dev/q2', runs: [], since: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--dry-run') opts.dryRun = true;
    else if (a === '--repo') opts.repo = argv[++i];
    else if (a === '--run') opts.runs.push(argv[++i]);
    else if (a === '--since') opts.since = argv[++i];
    else throw new Error(`unknown argument: ${a}`);
  }
  if (process.env.DRY_RUN === '1' || process.env.DRY_RUN === 'true') opts.dryRun = true;
  if (!opts.runs.length && !opts.since) {
    throw new Error('Usage: file-issues.mjs [--dry-run] [--repo owner/name] (--run <id>... | --since YYYY-MM-DD)');
  }
  return opts;
}

/** A run is eligible only if it is a red main run of a watched workflow from this repo. */
export function eligible(run) {
  if (run.conclusion !== 'failure') return 'not a failure';
  if (run.headBranch !== 'main') return `branch ${run.headBranch}`;
  if (!['push', 'schedule', 'workflow_dispatch'].includes(run.event)) return `event ${run.event}`;
  if (!WATCHED_WORKFLOWS.includes(run.workflow)) return `workflow ${run.workflow} not watched`;
  return null;
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  const runIds = opts.since ? failedMainRunsSince(opts.repo, opts.since) : opts.runs;
  if (!opts.dryRun) {
    // Idempotent: --force updates the label if it already exists.
    gh(['label', 'create', LABELS[0], '--repo', opts.repo, '--force', '--color', 'B60205',
      '--description', 'A test failed on main (filed by .github/workflows/red-main.yml)']);
  }
  const issues = loadIssues(opts.repo);
  const report = [];
  let fakeNumber = 0;

  for (const id of runIds) {
    const run = loadRun(opts.repo, id);
    const why = eligible(run);
    if (why) {
      report.push(`- run ${id}: skipped (${why})`);
      continue;
    }
    const log = loadLog(opts.repo, run);
    if (log === null) report.push(`- run ${id}: no log available; filing by job and step`);
    const plans = planIssues({ workflow: run.workflow, jobs: run.jobs, log: log ?? '' });

    for (const plan of plans) {
      const d = decide(plan, run, issues);
      const ref = d.issue ? `#${d.issue.number}` : '(new)';
      report.push(`- run ${run.id}: **${d.action}** ${ref} ${plan.title}`);
      if (d.action === 'skip') continue;

      if (d.action === 'create') {
        const body = issueBody(plan, run, { previous: d.previous });
        let number = `dry-${++fakeNumber}`;
        if (!opts.dryRun) {
          const url = withBodyFile(body, (f) => gh(['issue', 'create', '--repo', opts.repo, '--title', plan.title,
            '--body-file', f, ...LABELS.flatMap((l) => ['--label', l])]).trim());
          number = Number(url.split('/').pop());
        }
        issues.unshift({ number, state: 'OPEN', closedAt: null, title: plan.title, body });
        continue;
      }

      const issue = d.issue;
      const newBody = addOccurrence(issue.body, occurrenceRow(run, plan));
      if (!opts.dryRun) {
        if (d.action === 'reopen') gh(['issue', 'reopen', '--repo', opts.repo, String(issue.number)]);
        withBodyFile(newBody, (f) => gh(['issue', 'edit', '--repo', opts.repo, String(issue.number), '--body-file', f]));
        withBodyFile(commentBody(plan, run, { reopened: d.action === 'reopen' }), (f) =>
          gh(['issue', 'comment', '--repo', opts.repo, String(issue.number), '--body-file', f]));
      }
      issue.body = newBody;
      issue.state = 'OPEN';
    }
  }

  const touched = issues.filter((i) => String(i.number).startsWith('dry-') || report.some((r) => r.includes(`#${i.number} `)));
  const summary = [
    `## red-main filer${opts.dryRun ? ' (dry run: nothing written)' : ''}`,
    '',
    ...report,
    '',
    `### Issues touched: ${touched.length}`,
    '',
    '| Occurrences | Issue | Title |',
    '|---:|---|---|',
    ...touched
      .map((i) => ({ n: countOccurrences(i.body), i }))
      .sort((a, b) => b.n - a.n)
      .map(({ n, i }) => `| ${n} | ${String(i.number).startsWith('dry-') ? 'new' : `#${i.number}`} | ${i.title} |`),
  ].join('\n');
  console.log(summary);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${summary}\n`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  try {
    main();
  } catch (e) {
    console.error(e.stderr?.toString() || e.message);
    process.exit(1);
  }
}
