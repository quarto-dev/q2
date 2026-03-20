/**
 * Tests for ReplayDrawer component
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import ReplayDrawer from './ReplayDrawer';
import type { ReplayState, ReplayControls } from '../hooks/useReplayMode';

function makeState(overrides: Partial<ReplayState> = {}): ReplayState {
  return {
    isActive: false,
    historyLength: 0,
    currentIndex: 0,
    isPlaying: false,
    playbackSpeed: 1,
    currentContent: '',
    timestamp: null,
    actor: null,
    chunkActors: [],
    ...overrides,
  };
}

function makeControls(overrides: Partial<ReplayControls> = {}): ReplayControls {
  return {
    enter: vi.fn(),
    exit: vi.fn(),
    apply: vi.fn(),
    seekTo: vi.fn(),
    seekToStart: vi.fn(),
    seekToEnd: vi.fn(),
    play: vi.fn(),
    pause: vi.fn(),
    stepForward: vi.fn(),
    stepBackward: vi.fn(),
    cycleSpeed: vi.fn(),
    getTimestampAtIndex: vi.fn().mockReturnValue(null),
    ...overrides,
  };
}

describe('ReplayDrawer', () => {
  let controls: ReplayControls;

  beforeEach(() => {
    controls = makeControls();
  });

  afterEach(() => {
    cleanup();
  });

  describe('collapsed state', () => {
    it('renders chevron and "History" label', () => {
      render(<ReplayDrawer state={makeState()} controls={controls} />);
      expect(screen.getByText('Replay')).toBeDefined();
    });

    it('clicking the bar calls controls.enter()', () => {
      render(<ReplayDrawer state={makeState()} controls={controls} />);
      fireEvent.click(screen.getByText('Replay'));
      expect(controls.enter).toHaveBeenCalled();
    });
  });

  describe('expanded state', () => {
    const activeState = makeState({
      isActive: true,
      historyLength: 100,
      currentIndex: 42,
      currentContent: 'hello',
      timestamp: 1710000000,
      actor: 'abcdef0123456789abcdef0123456789',
      chunkActors: Array.from({ length: 10 }, () => [{ actor: 'abcdef0123456789abcdef0123456789', fraction: 1 }]),
    });

    it('renders transport controls when active', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      expect(screen.getByLabelText('Skip to start')).toBeDefined();
      expect(screen.getByLabelText('Step backward')).toBeDefined();
      expect(screen.getByLabelText('Play')).toBeDefined();
      expect(screen.getByLabelText('Step forward')).toBeDefined();
      expect(screen.getByLabelText('Skip to end')).toBeDefined();
    });

    it('Skip to start button calls controls.seekToStart()', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.click(screen.getByLabelText('Skip to start'));
      expect(controls.seekToStart).toHaveBeenCalled();
    });

    it('Skip to end button calls controls.seekToEnd()', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.click(screen.getByLabelText('Skip to end'));
      expect(controls.seekToEnd).toHaveBeenCalled();
    });

    it('renders Apply button and handle', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      expect(screen.getByText('Restore')).toBeDefined();
      expect(screen.getByLabelText('Close replay')).toBeDefined();
    });

    it('renders position indicator', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      expect(screen.getByText(/43\/100/)).toBeDefined();
    });

    it('renders actor short hash when no identity available', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      expect(screen.getByText('abcdef01')).toBeDefined();
    });

    it('renders screen name when identity is available', () => {
      const identities = { 'abcdef0123456789abcdef0123456789': { name: 'Alice', color: '#E91E63' } };
      render(<ReplayDrawer state={activeState} controls={controls} identities={identities} />);
      expect(screen.getByText('Alice')).toBeDefined();
    });

    it('renders truncated hex when identity is not in map', () => {
      const identities = { 'otheractor': { name: 'Bob', color: '#4CAF50' } };
      render(<ReplayDrawer state={activeState} controls={controls} identities={identities} />);
      expect(screen.getByText('abcdef01')).toBeDefined();
    });

    it('applies --me CSS class when currentActorId matches', () => {
      const identities = { 'abcdef0123456789abcdef0123456789': { name: 'Alice', color: '#E91E63' } };
      render(<ReplayDrawer state={activeState} controls={controls} currentActorId="abcdef0123456789abcdef0123456789" identities={identities} />);
      const actorEl = screen.getByText('Alice');
      expect(actorEl.className).toContain('replay-drawer__actor--me');
    });

    it('does not apply --me CSS class when actor is not current user', () => {
      const identities = { 'abcdef0123456789abcdef0123456789': { name: 'Alice', color: '#E91E63' } };
      render(<ReplayDrawer state={activeState} controls={controls} currentActorId="different0123456789abcdef01234567" identities={identities} />);
      const actorEl = screen.getByText('Alice');
      expect(actorEl.className).not.toContain('replay-drawer__actor--me');
    });

    it('applies --me CSS class with truncated hex when no identity', () => {
      render(<ReplayDrawer state={activeState} controls={controls} currentActorId="abcdef0123456789abcdef0123456789" />);
      const actorEl = screen.getByText('abcdef01');
      expect(actorEl.className).toContain('replay-drawer__actor--me');
    });

    it('renders short hash when currentActorId does not match and no identities', () => {
      render(<ReplayDrawer state={activeState} controls={controls} currentActorId="different0123456789abcdef01234567" />);
      expect(screen.getByText('abcdef01')).toBeDefined();
    });

    it('renders short hash when currentActorId is null', () => {
      render(<ReplayDrawer state={activeState} controls={controls} currentActorId={null} />);
      expect(screen.getByText('abcdef01')).toBeDefined();
    });

    it('Apply button calls controls.apply()', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.click(screen.getByText('Restore'));
      expect(controls.apply).toHaveBeenCalled();
    });

    it('handle calls controls.exit()', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.click(screen.getByLabelText('Close replay'));
      expect(controls.exit).toHaveBeenCalled();
    });

    it('header row does not exit on click', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      // Clicking the position text in the header should not trigger exit
      fireEvent.click(screen.getByText(/43\/100/));
      expect(controls.exit).not.toHaveBeenCalled();
    });

    it('shows Pause button when playing', () => {
      const playingState = makeState({
        ...activeState,
        isPlaying: true,
      });
      render(<ReplayDrawer state={playingState} controls={controls} />);
      expect(screen.getByLabelText('Pause')).toBeDefined();
    });

    it('scrubber onChange calls controls.seekTo()', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      const scrubber = screen.getByRole('slider');
      fireEvent.change(scrubber, { target: { value: '10' } });
      expect(controls.seekTo).toHaveBeenCalledWith(10);
    });

    it('speed button shows current speed and calls cycleSpeed', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      const speedBtn = screen.getByLabelText('Playback speed');
      expect(speedBtn.textContent).toBe('1x');
      fireEvent.click(speedBtn);
      expect(controls.cycleSpeed).toHaveBeenCalled();
    });

    it('speed button reflects 4x speed', () => {
      const fastState = makeState({ ...activeState, playbackSpeed: 4 });
      render(<ReplayDrawer state={fastState} controls={controls} />);
      expect(screen.getByLabelText('Playback speed').textContent).toBe('4x');
    });
  });

  describe('keyboard shortcuts', () => {
    const activeState = makeState({
      isActive: true,
      historyLength: 100,
      currentIndex: 50,
      currentContent: 'test',
      chunkActors: Array.from({ length: 10 }, () => [{ actor: 'actor1', fraction: 1 }]),
    });

    it('Space toggles play/pause', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.keyDown(container.firstChild!, { key: ' ' });
      expect(controls.play).toHaveBeenCalled();
    });

    it('ArrowLeft calls stepBackward', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.keyDown(container.firstChild!, { key: 'ArrowLeft' });
      expect(controls.stepBackward).toHaveBeenCalled();
    });

    it('ArrowRight calls stepForward', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.keyDown(container.firstChild!, { key: 'ArrowRight' });
      expect(controls.stepForward).toHaveBeenCalled();
    });

    it('Home calls seekToStart', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.keyDown(container.firstChild!, { key: 'Home' });
      expect(controls.seekToStart).toHaveBeenCalled();
    });

    it('End calls seekToEnd', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.keyDown(container.firstChild!, { key: 'End' });
      expect(controls.seekToEnd).toHaveBeenCalled();
    });

    it('Escape calls exit', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.keyDown(container.firstChild!, { key: 'Escape' });
      expect(controls.exit).toHaveBeenCalled();
    });
  });
});
