// node --test scripts/red-main/*.test.mjs

import { describe, it } from 'node:test';
import assert from 'node:assert/strict';

import {
  addOccurrence,
  commentBody,
  countOccurrences,
  decide,
  eligible,
  hasOccurrence,
  issueBody,
  keyOf,
} from './file-issues.mjs';

const plan = {
  key: 'abc123def456',
  kind: 'test',
  title: '[red main] vitest: pkg/src/a.test.ts > works',
  workflow: 'TS Test Suite',
  job: 'Workspace TS suites',
  variants: ['macos-latest'],
  runner: 'vitest',
  id: 'pkg/src/a.test.ts > works',
  excerpt: 'FAIL pkg/src/a.test.ts > works\nError: Timeout\n````nested fence````',
};
const run = (id, attempt = 1) => ({
  id, attempt, url: `https://github.com/o/r/actions/runs/${id}`,
  headSha: '0123456789abcdef', createdAt: '2026-11-23T20:30:31Z',
});
const now = new Date('2026-11-24T00:00:00Z');

describe('issue body', () => {
  const body = issueBody(plan, run(1));

  it('carries the key marker and one occurrence', () => {
    assert.equal(keyOf(body), plan.key);
    assert.equal(countOccurrences(body), 1);
    assert.ok(hasOccurrence(body, run(1)));
  });

  it('cannot be broken out of by a fence in the excerpt', () => {
    const fences = body.match(/^````/gm);
    assert.equal(fences.length, 2);
  });

  it('accumulates occurrences; attempts are distinct', () => {
    let b = addOccurrence(body, `| 2026-11-24 | [2 #1](u) | \`x\` | y |`);
    b = addOccurrence(b, `| 2026-11-24 | [2 #2](u) | \`x\` | y |`);
    assert.equal(countOccurrences(b), 3);
    assert.ok(hasOccurrence(b, run(2, 2)));
    assert.ok(!hasOccurrence(b, run(2, 3)));
  });

  it('a recurrence comment says so', () => {
    assert.match(commentBody(plan, run(3), { reopened: true }), /^\*\*Recurred after close\.\*\*/);
  });
});

describe('decide', () => {
  const issue = (over) => ({ number: 10, state: 'OPEN', closedAt: null, body: issueBody(plan, run(1)), ...over });

  it('creates when nothing matches the key', () => {
    assert.deepEqual(decide(plan, run(2), [issue({ body: 'unrelated' })], now), { action: 'create' });
  });

  it('comments on the open issue', () => {
    assert.equal(decide(plan, run(2), [issue()], now).action, 'comment');
  });

  it('skips a run it already recorded (re-running the filer is a no-op)', () => {
    assert.equal(decide(plan, run(1), [issue()], now).action, 'skip');
  });

  it('reopens an issue closed within 30 days', () => {
    const d = decide(plan, run(2), [issue({ state: 'CLOSED', closedAt: '2026-11-10T00:00:00Z' })], now);
    assert.equal(d.action, 'reopen');
  });

  it('opens a fresh issue linking an old closed one', () => {
    const d = decide(plan, run(2), [issue({ state: 'CLOSED', closedAt: '2026-09-01T00:00:00Z' })], now);
    assert.deepEqual(d, { action: 'create', previous: 10 });
  });
});

describe('eligible', () => {
  const base = { conclusion: 'failure', headBranch: 'main', event: 'push', workflow: 'Test Suite' };

  it('accepts a red push to main of a watched workflow', () => {
    assert.equal(eligible(base), null);
  });

  it('rejects a fork PR from a branch named main', () => {
    assert.match(eligible({ ...base, event: 'pull_request' }), /event/);
  });

  it('rejects unwatched workflows and other branches', () => {
    assert.match(eligible({ ...base, workflow: 'Nightly' }), /not watched/);
    assert.match(eligible({ ...base, headBranch: 'kyoto' }), /branch/);
  });
});
