import { describe, expect, it } from 'vitest';
import { previewHits, formatLoopTable, type LoopRun } from '../../src/style-loop';

const line = (o: object) => JSON.stringify(o);

describe('previewHits: unstyled previews in one story\'s events.jsonl', () => {
  it('lists each unstyled event with who saw it and whether it lasted', () => {
    const text = [
      line({ source: 'preview', kind: 'facts', details: { who: 'host' } }),
      line({ source: 'preview', kind: 'unstyled', details: { who: 'guest', reasons: ['no-stylesheet-rules', 'default-font'] } }),
      line({ source: 'preview', kind: 'unstyled-persisted', details: { who: 'guest' } }),
      line({ source: 'invariant', kind: 'x', details: {} }),
      'not json',
    ].join('\n');
    expect(previewHits(text)).toEqual([{ who: 'guest', reasons: ['no-stylesheet-rules', 'default-font'], persisted: true }]);
  });

  it('is empty when every preview was styled', () => {
    expect(previewHits(line({ source: 'preview', kind: 'facts', details: {} }))).toEqual([]);
  });
});

describe('formatLoopTable: the tally of a loop', () => {
  it('counts runs with a hit, per story and per side', () => {
    const runs: LoopRun[] = [
      { run: 1, preset: 'hub-browser', passed: true, hits: [] },
      { run: 2, preset: 'hub-browser', passed: true, hits: [{ who: 'guest', reasons: ['no-stylesheet-rules'], persisted: false }] },
      { run: 2, preset: 'vscode-share', passed: false, hits: [] },
    ];
    const t = formatLoopTable(runs);
    expect(t).toContain('hub-browser');
    expect(t).toMatch(/hub-browser\s+2\s+1\s+0\s+0/); // columns: story, runs, unstyled, persisted, failed
    expect(t).toContain('guest: 1');
  });
});
