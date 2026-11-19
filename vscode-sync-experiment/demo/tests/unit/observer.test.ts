import { describe, expect, it } from 'vitest';
import { deriveEvents, narrate, type StatusBody } from '../../src/observer';

const body = (o: Partial<StatusBody> = {}): StatusBody => ({ tracking: true, paused: false, pending: false, markers: [], decisions: [], ...o });
const dec = (kind: string, files: string[] = [], extra: Partial<StatusBody['decisions'][0]> = {}) => ({ kind, time_ms: 1, files, step: null, discarded: null, ...extra });

describe('observer (events and narration)', () => {
  it('a first body with a decision yields that decision as an event; the same body again yields nothing', () => {
    const b = body({ decisions: [dec('seeded')] });
    expect(deriveEvents(null, b).map((e) => e.kind)).toEqual(['decision:seeded']);
    expect(deriveEvents(b, b)).toEqual([]);
  });

  it('a flip of N files is narrated with N and the swap notice', () => {
    const prev = body();
    const next = body({ decisions: [dec('flipped', ['a.qmd', 'b.qmd', 'c.png'])] });
    const [e] = deriveEvents(prev, next);
    expect(e!.kind).toBe('decision:flipped');
    expect(narrate(e!)).toBe('The hub switched branches: 3 files followed (a.qmd, b.qmd, c.png).');
  });

  it('a pause and its end are two events', () => {
    const a = body();
    const b = body({ paused: true, markers: ['rebase-merge'] });
    expect(deriveEvents(a, b).map((e) => narrate(e))).toEqual(['Sync paused: a rebase-merge is in progress.']);
    expect(deriveEvents(b, a).map((e) => narrate(e))).toEqual(['Sync resumed.']);
  });

  it('a retire reports the discarded edits', () => {
    const [e] = deriveEvents(body(), body({ decisions: [dec('retired', ['p.qmd'], { discarded: 2 })] }));
    expect(narrate(e!)).toBe('The hub retired p.qmd and discarded 2 unsaved edits.');
  });

  it('only decisions after the previous body are new', () => {
    const d1 = dec('flipped', ['a.qmd'], { time_ms: 1 });
    const d2 = dec('moved-forward', ['a.qmd'], { time_ms: 2 });
    const events = deriveEvents(body({ decisions: [d1] }), body({ decisions: [d1, d2] }));
    expect(events.map((e) => e.kind)).toEqual(['decision:moved-forward']);
  });

  it('a removal and a re-listing are narrated from the file lists', () => {
    const e = deriveEvents(body(), body(), { prevFiles: ['a.qmd', 'old.qmd'], nextFiles: ['a.qmd', 'new.qmd'] });
    expect(e.map((x) => narrate(x))).toEqual(['old.qmd left the project.', 'new.qmd joined the project.']);
  });
});
