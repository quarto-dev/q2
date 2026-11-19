import { describe, expect, it } from 'vitest';
import * as notices from './notices.js';
import { movedNotice } from './moved.js';
import type { MovedMessage } from './moved.js';
import { NoticeAggregator } from './swap.js';
import type { ReplacedEvent } from './swap.js';

const done = (path: string, stranded?: number): ReplacedEvent => ({
  path,
  phase: 'done',
  oldDocId: 'o',
  newDocId: 'n',
  stranded,
});

const SUFFIX = "because the project owner's checked-out version changed.";

describe('NoticeAggregator', () => {
  it('10a. one file that is the open file', () => {
    const a = new NoticeAggregator();
    a.add(done('a.qmd'));
    expect(a.flush('a.qmd')).toBe(`This file was replaced ${SUFFIX}`);
  });

  it('10a2. one file when no file is open', () => {
    const a = new NoticeAggregator();
    a.add(done('a.qmd'));
    expect(a.flush()).toBe(`This file was replaced ${SUFFIX}`);
  });

  it('10b. one file that is not the open file names the path', () => {
    const a = new NoticeAggregator();
    a.add(done('a.qmd'));
    expect(a.flush('b.qmd')).toBe(`The file a.qmd was replaced ${SUFFIX}`);
  });

  it('10c. many files give a count', () => {
    const a = new NoticeAggregator();
    a.add(done('a.qmd'));
    a.add(done('b.qmd'));
    a.add(done('c.qmd'));
    expect(a.flush('a.qmd')).toBe(`3 files were replaced ${SUFFIX}`);
  });

  it('10d. appends stranded edits from events and from addStranded', () => {
    const a = new NoticeAggregator();
    a.add(done('a.qmd', 2));
    a.addStranded(1);
    expect(a.flush('a.qmd')).toBe(
      `This file was replaced ${SUFFIX} 3 of your recent edits stayed with the previous version.`,
    );
  });

  it('10e. ignores unavailable events', () => {
    const a = new NoticeAggregator();
    a.add({ path: 'a.qmd', phase: 'unavailable', oldDocId: 'o', newDocId: 'n' });
    a.add({ path: 'a.qmd', phase: 'pending', oldDocId: 'o', newDocId: 'n' });
    expect(a.flush()).toBeNull();
  });

  it('10f. two batches produce two notices; the same path counts once per batch', () => {
    const a = new NoticeAggregator();
    a.add(done('a.qmd'));
    a.add(done('a.qmd'));
    expect(a.flush('a.qmd')).toBe(`This file was replaced ${SUFFIX}`);
    a.add(done('b.qmd'));
    expect(a.flush('b.qmd')).toBe(`This file was replaced ${SUFFIX}`);
    expect(a.flush()).toBeNull();
  });
});

describe('movedNotice', () => {
  it('11. uses the name, or a generic phrase without one', () => {
    expect(movedNotice('Ana')).toBe('Ana switched versions');
    expect(movedNotice()).toBe('This person switched versions');
    const m: MovedMessage = { type: 'moved', peerId: 'p1' };
    expect(m.type).toBe('moved');
  });
});

describe('notice constants', () => {
  it('lists every notice text of the epic, one constant each', () => {
    const { formatNotice, ...constants } = notices as Record<string, unknown>;
    expect(typeof formatNotice).toBe('function');
    expect(constants).toEqual({
      PAUSED_BANNER: 'Paused for git: edits to files the project owner rewrites may be set aside',
      BRANCHES_STAY: '{n} branches of this file stay with the previous version',
      GIT_OP_EDITS_DROPPED: 'Edits made during a git operation were not kept.',
      EDITOR_ONLY_NO_FLIP: 'The project owner has not switched versions yet. Keep waiting?',
      FILES_DETACHED:
        '{n} files are detached: your checked-out version differs from the shared one. Your edits to them no longer reach the project.',
      FILES_JOINED_LIVE:
        '{n} files joined live edits from collaborators that are not committed to git.',
      ADOPT_CONFIRM: 'This file has uncommitted changes. Replace it with the shared version?',
      DOC_SUPERSEDED: 'Your doc was superseded',
      REPLACED_THIS_FILE: `This file was replaced ${SUFFIX}`,
      REPLACED_ONE_FILE: `The file {path} was replaced ${SUFFIX}`,
      REPLACED_MANY_FILES: `{n} files were replaced ${SUFFIX}`,
      STRANDED_EDITS: '{n} of your recent edits stayed with the previous version.',
      MOVED_NAMED: '{name} switched versions',
      MOVED_ANONYMOUS: 'This person switched versions',
      FILES_DETACHED_ACTIONS: ['Adopt shared version', 'Stay detached'],
      FILES_JOINED_LIVE_ACTIONS: ['Use my checkout instead'],
      EDITOR_ONLY_NO_FLIP_ACTIONS: ['Keep waiting', 'Dismiss'],
      NOTICE_SWITCHED_VERSIONS: `This file was replaced ${SUFFIX}`,
      NOTICE_STRANDED_EDITS: '{n} of your recent edits stayed with the previous version.',
      NOTICE_UNAVAILABLE: "The project owner's version of this file is not available yet.",
      NOTICE_DETACHED:
        '{n} files are detached: your checked-out version differs from the shared one. Your edits to them no longer reach the project.',
      NOTICE_JOINED_LIVE:
        '{n} files joined live edits from collaborators that are not committed to git.',
      NOTICE_EDITS_NOT_KEPT: 'Edits made during a git operation were not kept.',
    });
  });

  it('formatNotice fills placeholders', () => {
    expect(notices.formatNotice('{n} files', { n: 3 })).toBe('3 files');
  });
});
