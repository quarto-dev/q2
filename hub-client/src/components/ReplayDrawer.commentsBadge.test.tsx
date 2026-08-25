/**
 * Tests for the outstanding-comments count badge on the comments-mode
 * toggle (bd-0rsk07il, GH #445).
 *
 * The count arrives from the render pipeline's `DocumentProfile`
 * comment summary (RenderResponse.comments) via Editor state. One
 * badge for the whole toggle group (resolved decision: single
 * location), hidden at zero.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import ReplayDrawer from './ReplayDrawer';
import type { ReplayState, ReplayControls } from '../hooks/useReplayMode';

afterEach(cleanup);

const inactiveState: ReplayState = {
  isActive: false,
  historyLength: 0,
  currentIndex: 0,
  isPlaying: false,
  playbackSpeed: 1,
  currentContent: '',
  timestamp: null,
  actor: null,
  chunkActors: [],
};

const noopControls: ReplayControls = {
  enter: () => {},
  exit: () => {},
  apply: () => {},
  seekTo: () => {},
  seekToStart: () => {},
  seekToEnd: () => {},
  play: () => {},
  pause: () => {},
  stepForward: () => {},
  stepBackward: () => {},
  cycleSpeed: () => {},
  getTimestampAtIndex: () => null,
};

function renderDrawer(commentsCount?: number) {
  return render(
    <ReplayDrawer
      state={inactiveState}
      controls={noopControls}
      commentsMode="show"
      onCommentsModeChange={() => {}}
      commentsCount={commentsCount}
    />,
  );
}

describe('comments-mode toggle badge', () => {
  it('shows the outstanding-comment count when positive', () => {
    renderDrawer(3);
    const badge = screen.getByLabelText('3 outstanding comments');
    expect(badge.textContent).toBe('3');
  });

  it('renders no badge at zero', () => {
    renderDrawer(0);
    expect(screen.queryByLabelText(/outstanding comment/)).toBeNull();
    // The toggle group itself still renders.
    expect(screen.getByRole('group', { name: 'Comment display mode' })).toBeTruthy();
  });

  it('renders no badge when the count is not provided', () => {
    renderDrawer(undefined);
    expect(screen.queryByLabelText(/outstanding comment/)).toBeNull();
  });

  it('uses singular phrasing for one comment', () => {
    renderDrawer(1);
    const badge = screen.getByLabelText('1 outstanding comment');
    expect(badge.textContent).toBe('1');
  });
});
