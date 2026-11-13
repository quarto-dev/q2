/**
 * Tests for ReplayDrawer component
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react';
import ReplayDrawer from './ReplayDrawer';
import type { ReplayState, ReplayControls } from '../hooks/useReplayMode';

// Mock getAuthorId from automergeSync (now in @quarto/preview-runtime)
let mockAuthorId: string | null = null;
vi.mock('@quarto/preview-runtime', () => ({
  getAuthorId: () => mockAuthorId,
}));

function makeState(overrides: Partial<ReplayState> = {}): ReplayState {
  return {
    isActive: false,
    historyLength: 0,
    currentIndex: 0,
    isPlaying: false,
    playbackSpeed: 1,
    currentContent: '',
    currentHeads: null,
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
    mockAuthorId = null;
  });

  afterEach(() => {
    cleanup();
  });

  describe('collapsed state', () => {
    it('renders chevron and "History" label', () => {
      render(<ReplayDrawer state={makeState()} controls={controls} />);
      expect(screen.getByText('Version history')).toBeDefined();
    });

    it('renders a history icon and a chevron in the toggle', () => {
      render(<ReplayDrawer state={makeState()} controls={controls} />);
      const toggle = screen.getByText('Version history').closest('button')!;
      expect(toggle.querySelectorAll('svg').length).toBe(2);
    });

    it('clicking the bar calls controls.enter()', () => {
      render(<ReplayDrawer state={makeState()} controls={controls} />);
      fireEvent.click(screen.getByText('Version history'));
      expect(controls.enter).toHaveBeenCalled();
    });
  });

  describe('expanded state', () => {
    const activeState = makeState({
      isActive: true,
      historyLength: 100,
      currentIndex: 42,
      currentContent: 'hello',
      currentHeads: null,
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

    it('renders Restore button and close button (no drag handle)', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      expect(screen.getByText('Restore this version')).toBeDefined();
      expect(screen.getByLabelText('Close replay')).toBeDefined();
      expect(container.querySelector('.replay-drawer__handle')).toBeNull();
    });

    it('restore renders as the primary action with an icon', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      const btn = screen.getByText('Restore this version').closest('button')!;
      expect(btn.className).toContain('replay-drawer__restore');
      expect(btn.querySelector('svg')).not.toBeNull();
    });

    it('renders position indicator', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      const current = screen.getByText('43');
      expect(current.closest('.replay-drawer__position')?.textContent).toBe('43 / 100');
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

    it('truncates long actor names and exposes the full name in a tooltip', async () => {
      const longName = 'A'.repeat(100);
      const identities = {
        'abcdef0123456789abcdef0123456789': { name: longName, color: '#E91E63' },
      };
      render(<ReplayDrawer state={activeState} controls={controls} identities={identities} />);

      const actorName = screen.getByText(longName);
      expect(actorName.className).toContain('qh-truncate');

      const { readFileSync } = await import('node:fs');
      const styles = readFileSync('src/components/ReplayDrawer.css', 'utf8');
      expect(styles).toMatch(/\.replay-drawer__actor\s*\{[^}]*min-width:\s*0;/s);

      fireEvent.focus(actorName.closest('.replay-drawer__actor')!);
      expect(screen.getByRole('tooltip').textContent).toBe(longName);
    });

    it('renders truncated hex when identity is not in map', () => {
      const identities = { 'otheractor': { name: 'Bob', color: '#4CAF50' } };
      render(<ReplayDrawer state={activeState} controls={controls} identities={identities} />);
      expect(screen.getByText('abcdef01')).toBeDefined();
    });

    it('applies --me CSS class when currentActorId matches', () => {
      mockAuthorId = 'abcdef0123456789abcdef0123456789';
      const identities = { 'abcdef0123456789abcdef0123456789': { name: 'Alice', color: '#E91E63' } };
      render(<ReplayDrawer state={activeState} controls={controls} identities={identities} />);
      const actorEl = screen.getByText('Alice').closest('.replay-drawer__actor')!;
      expect(actorEl.className).toContain('replay-drawer__actor--me');
    });

    it('does not apply --me CSS class when actor is not current user', () => {
      mockAuthorId = 'different0123456789abcdef01234567';
      const identities = { 'abcdef0123456789abcdef0123456789': { name: 'Alice', color: '#E91E63' } };
      render(<ReplayDrawer state={activeState} controls={controls} identities={identities} />);
      const actorEl = screen.getByText('Alice').closest('.replay-drawer__actor')!;
      expect(actorEl.className).not.toContain('replay-drawer__actor--me');
    });

    it('applies --me CSS class with truncated hex when no identity', () => {
      mockAuthorId = 'abcdef0123456789abcdef0123456789';
      render(<ReplayDrawer state={activeState} controls={controls} />);
      const actorEl = screen.getByText('abcdef01').closest('.replay-drawer__actor')!;
      expect(actorEl.className).toContain('replay-drawer__actor--me');
    });

    it('renders short hash when currentActorId does not match and no identities', () => {
      mockAuthorId = 'different0123456789abcdef01234567';
      render(<ReplayDrawer state={activeState} controls={controls} />);
      expect(screen.getByText('abcdef01')).toBeDefined();
    });

    it('renders short hash when currentActorId is null', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      expect(screen.getByText('abcdef01')).toBeDefined();
    });

    // ── Mixed-history attribution (author-ID transition) ───────────
    //
    // A document can hold pre-transition changes (no author anywhere —
    // the key falls back to the bare stable actor) and post-transition
    // changes (key resolved from the author footer / actor→author index)
    // by the SAME user. By D5 the author ID equals the legacy stable
    // actor ID, so every step from both eras arrives here with the same
    // attribution key — one identity entry, one palette color, and the
    // "me" highlight on both sides of the boundary.
    describe('mixed-history attribution (author-ID transition)', () => {
      const KEY = 'abcdef0123456789abcdef0123456789';
      const identities = { [KEY]: { name: 'Alice', color: '#E91E63' } };

      it('renders steps from both eras as one continuous identity', () => {
        mockAuthorId = KEY;
        // Chunks as replay.ts's getMetadataAt resolves them: legacy steps
        // (actor fallback) and post-transition steps (author) — the same
        // string by D5.
        const mixed = makeState({
          isActive: true,
          historyLength: 4,
          currentIndex: 3,
          currentContent: 'hello',
          currentHeads: null,
          timestamp: 1710000000,
          actor: KEY,
          chunkActors: [
            [{ actor: KEY, fraction: 1 }], // legacy step (actor fallback)
            [{ actor: KEY, fraction: 1 }], // legacy step
            [{ actor: KEY, fraction: 1 }], // post-transition (author footer)
            [{ actor: KEY, fraction: 1 }], // post-transition (actor→author index)
          ],
        });
        const { container } = render(
          <ReplayDrawer state={mixed} controls={controls} identities={identities} />,
        );

        // One continuous color band: every actor-colored waveform rect in
        // the (played) past group uses the single identity entry's color.
        const rects = container.querySelectorAll<SVGRectElement>(
          '.replay-waveform__past rect',
        );
        expect(rects.length).toBe(4);
        for (const rect of rects) {
          expect(rect.getAttribute('fill')).toBe('#E91E63');
        }

        // The current step highlights as "me" with the shared identity name.
        const actorEl = screen.getByText('Alice').closest('.replay-drawer__actor')!;
        expect(actorEl.className).toContain('replay-drawer__actor--me');
      });
    });

    it('Restore button calls controls.apply()', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.click(screen.getByText('Restore this version'));
      expect(controls.apply).toHaveBeenCalled();
    });

    it('close button calls controls.exit()', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.click(screen.getByLabelText('Close replay'));
      expect(controls.exit).toHaveBeenCalled();
    });

    it('clicking the toggle calls controls.exit()', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      fireEvent.click(screen.getByText('Version history'));
      expect(controls.exit).toHaveBeenCalled();
    });

    it('header row does not exit on click', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      // Clicking the position text in the header should not trigger exit
      fireEvent.click(screen.getByText('43'));
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

    it('play/pause render as uniform ghost buttons (no filled-play modifier)', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      expect(screen.getByLabelText('Play').className).toBe('replay-drawer__btn');
      const playingState = makeState({ ...activeState, isPlaying: true });
      render(<ReplayDrawer state={playingState} controls={controls} />);
      expect(screen.getByLabelText('Pause').className).toBe('replay-drawer__btn');
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
      expect(speedBtn.textContent).toBe('1×');
      fireEvent.click(speedBtn);
      expect(controls.cycleSpeed).toHaveBeenCalled();
    });

    it('speed button reflects 4x speed', () => {
      const fastState = makeState({ ...activeState, playbackSpeed: 4 });
      render(<ReplayDrawer state={fastState} controls={controls} />);
      expect(screen.getByLabelText('Playback speed').textContent).toBe('4×');
    });

    it('adds a (you) suffix to the current user actor chip', () => {
      mockAuthorId = 'abcdef0123456789abcdef0123456789';
      const identities = { 'abcdef0123456789abcdef0123456789': { name: 'Alice', color: '#E91E63' } };
      const { container } = render(
        <ReplayDrawer state={activeState} controls={controls} identities={identities} />,
      );
      expect(container.querySelector('.replay-drawer__actor-you')?.textContent).toBe('(you)');
    });

    it('omits the (you) suffix for other actors', () => {
      mockAuthorId = 'different0123456789abcdef01234567';
      const identities = { 'abcdef0123456789abcdef0123456789': { name: 'Alice', color: '#E91E63' } };
      const { container } = render(
        <ReplayDrawer state={activeState} controls={controls} identities={identities} />,
      );
      expect(container.querySelector('.replay-drawer__actor-you')).toBeNull();
    });
  });

  describe('unknown metadata', () => {
    // A step whose change carries no timestamp / attribution key must not
    // leak a bare `0` into the header (React renders falsy numbers inside
    // `{cond && …}`) — it shows an explicit 'unknown' label instead.
    const unknownState = makeState({
      isActive: true,
      historyLength: 5,
      currentIndex: 2,
      currentContent: 'hello',
      currentHeads: null,
      timestamp: null,
      actor: null,
    });

    it('labels a missing timestamp "unknown" and missing attribution "anonymous"', () => {
      render(<ReplayDrawer state={unknownState} controls={controls} />);
      expect(screen.getByText('unknown')).toBeDefined();
      expect(screen.getByText('anonymous')).toBeDefined();
    });

    it('treats a 0 timestamp (unset change time) as unknown', () => {
      const zeroTs = makeState({
        ...unknownState,
        timestamp: 0,
        actor: 'abcdef0123456789abcdef0123456789',
      });
      const { container } = render(<ReplayDrawer state={zeroTs} controls={controls} />);
      const info = container.querySelector('.replay-drawer__info')!;
      expect(screen.getByText('unknown')).toBeDefined();
      expect(info.textContent!.startsWith('0')).toBe(false);
    });
  });

  describe('waveform', () => {
    const activeState = makeState({
      isActive: true,
      historyLength: 100,
      currentIndex: 42,
      currentContent: 'hello',
      currentHeads: null,
      timestamp: 1710000000,
      actor: 'abcdef0123456789abcdef0123456789',
      chunkActors: Array.from({ length: 10 }, () => [{ actor: 'abcdef0123456789abcdef0123456789', fraction: 1 }]),
    });

    it('track rect carries a CSS class, not a hard-coded fill', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      const track = container.querySelector('.replay-waveform__track');
      expect(track).not.toBeNull();
      expect(track!.getAttribute('fill')).toBeNull();
    });

    it('renders past and future clip groups mirroring the actor chunks', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      const past = container.querySelectorAll('.replay-waveform__past rect');
      const future = container.querySelectorAll('.replay-waveform__future rect');
      expect(past.length).toBe(10);
      expect(future.length).toBe(10);
    });

    it('clips the waveform at the playhead position', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      // currentIndex 42 of 100 → 42/99 × 100 = 42.4242…
      const pastClip = container.querySelector('.replay-waveform__clip-past rect');
      const futureClip = container.querySelector('.replay-waveform__clip-future rect');
      expect(parseFloat(pastClip!.getAttribute('width')!)).toBeCloseTo(42.4242, 3);
      expect(parseFloat(futureClip!.getAttribute('x')!)).toBeCloseTo(42.4242, 3);
      expect(parseFloat(futureClip!.getAttribute('width')!)).toBeCloseTo(57.5758, 3);
    });

    it('renders contiguous columns when chunks are few', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      const rects = container.querySelectorAll<SVGRectElement>('.replay-waveform__past rect');
      // 10 chunks → columns tile the full width (no gaps).
      expect(parseFloat(rects[0].getAttribute('x')!)).toBeCloseTo(0, 5);
      expect(parseFloat(rects[0].getAttribute('width')!)).toBeCloseTo(10, 5);
      expect(parseFloat(rects[1].getAttribute('x')!)).toBeCloseTo(10, 5);
    });

    it('renders contiguous columns even when chunks are dense', () => {
      const denseState = makeState({
        ...activeState,
        chunkActors: Array.from({ length: 40 }, () => [{ actor: 'abcdef0123456789abcdef0123456789', fraction: 1 }]),
      });
      const { container } = render(<ReplayDrawer state={denseState} controls={controls} />);
      const rects = container.querySelectorAll<SVGRectElement>('.replay-waveform__past rect');
      // 40 chunks → column width 2.5, still no gaps (review feedback:
      // inter-column gaps read as missing data, not separators).
      expect(rects.length).toBe(40);
      expect(parseFloat(rects[0].getAttribute('x')!)).toBeCloseTo(0, 5);
      expect(parseFloat(rects[0].getAttribute('width')!)).toBeCloseTo(2.5, 5);
      expect(parseFloat(rects[1].getAttribute('x')!)).toBeCloseTo(2.5, 5);
    });

    it('renders a playhead with caret at the progress position', () => {
      const { container } = render(<ReplayDrawer state={activeState} controls={controls} />);
      const playhead = container.querySelector<HTMLElement>('.replay-waveform__playhead');
      expect(playhead).not.toBeNull();
      expect(playhead!.style.left.startsWith('42.42')).toBe(true);
      expect(playhead!.querySelector('.replay-waveform__playhead-caret')).not.toBeNull();
    });

    it('scrubber hover tooltip shows the timestamp and change number', () => {
      const tsControls = makeControls({
        getTimestampAtIndex: vi.fn().mockReturnValue(1710000000),
      });
      const { container } = render(<ReplayDrawer state={activeState} controls={tsControls} />);
      const input = screen.getByRole('slider');
      vi.spyOn(input, 'getBoundingClientRect').mockReturnValue({
        left: 0, width: 200, top: 0, right: 200, bottom: 30, height: 30, x: 0, y: 0,
        toJSON: () => ({}),
      } as DOMRect);
      fireEvent.mouseMove(input, { clientX: 100 });
      const tip = container.querySelector('.replay-drawer__tooltip');
      expect(tip).not.toBeNull();
      expect(tip!.textContent).toContain('Change 51');
      expect(tip!.textContent).toContain('2024');
    });
  });

  describe('tooltip shortcut hints', () => {
    const activeState = makeState({
      isActive: true,
      historyLength: 100,
      currentIndex: 42,
      currentContent: 'hello',
      currentHeads: null,
      chunkActors: [],
    });

    it('play/pause tooltip carries the Space hint', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      const play = screen.getByLabelText('Play');
      fireEvent.focus(play);
      expect(screen.getByRole('tooltip').textContent).toBe('Play (Space)');
      fireEvent.blur(play);
    });

    it('step backward tooltip carries the arrow hint', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      const step = screen.getByLabelText('Step backward');
      fireEvent.focus(step);
      expect(screen.getByRole('tooltip').textContent).toBe('Step backward (←)');
      fireEvent.blur(step);
    });

    it('close button tooltip carries the Esc hint', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      const close = screen.getByLabelText('Close replay');
      fireEvent.focus(close);
      expect(screen.getByRole('tooltip').textContent).toBe('Close replay (Esc)');
      fireEvent.blur(close);
    });

    it('restore tooltip states that history is preserved', () => {
      render(<ReplayDrawer state={activeState} controls={controls} />);
      const restore = screen.getByText('Restore this version').closest('button')!;
      fireEvent.focus(restore);
      expect(screen.getByRole('tooltip').textContent).toBe(
        'Replace the document with this version (history is preserved)',
      );
      fireEvent.blur(restore);
    });
  });

  describe('keyboard shortcuts', () => {
    const activeState = makeState({
      isActive: true,
      historyLength: 100,
      currentIndex: 50,
      currentContent: 'test',
      currentHeads: null,
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

  describe('Attribution toggle', () => {
    it('renders in collapsed state when attributionOn + onAttributionChange are passed', () => {
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={false}
          onAttributionChange={vi.fn()}
        />,
      );
      expect(screen.getByLabelText(/Authors/)).toBeDefined();
    });

    it('renders in expanded state', () => {
      const activeState = makeState({
        isActive: true,
        historyLength: 100,
        currentIndex: 42,
        currentContent: 'hello',
        currentHeads: null,
        timestamp: 1710000000,
        actor: 'abcdef0123456789abcdef0123456789',
        chunkActors: [],
      });
      render(
        <ReplayDrawer
          state={activeState}
          controls={controls}
          attributionOn={false}
          onAttributionChange={vi.fn()}
        />,
      );
      expect(screen.getByLabelText(/Authors/)).toBeDefined();
    });

    it('reflects attributionOn state via aria-pressed', () => {
      const { rerender } = render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={false}
          onAttributionChange={vi.fn()}
        />,
      );
      expect(screen.getByLabelText(/Authors/).getAttribute('aria-pressed')).toBe('false');

      rerender(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={true}
          onAttributionChange={vi.fn()}
        />,
      );
      expect(screen.getByLabelText(/Authors/).getAttribute('aria-pressed')).toBe('true');
    });

    it('clicking toggles via onAttributionChange', () => {
      const onChange = vi.fn();
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={false}
          onAttributionChange={onChange}
        />,
      );
      fireEvent.click(screen.getByLabelText(/Authors/));
      expect(onChange).toHaveBeenCalledWith(true);
    });

    it('clicking does not trigger replay enter', () => {
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={false}
          onAttributionChange={vi.fn()}
        />,
      );
      fireEvent.click(screen.getByLabelText(/Authors/));
      expect(controls.enter).not.toHaveBeenCalled();
    });

    it('is omitted when onAttributionChange is not provided', () => {
      render(<ReplayDrawer state={makeState()} controls={controls} />);
      expect(screen.queryByLabelText(/Authors/)).toBeNull();
    });

    it('applies the generating modifier class and aria-busy when attributionGenerating is true', () => {
      const { rerender } = render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={true}
          onAttributionChange={vi.fn()}
          attributionGenerating={false}
        />,
      );
      const pill = screen.getByLabelText(/Authors/);
      expect(pill.className).not.toContain('replay-drawer__attribution--generating');
      expect(pill.getAttribute('aria-busy')).toBeNull();

      rerender(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={true}
          onAttributionChange={vi.fn()}
          attributionGenerating={true}
        />,
      );
      expect(pill.className).toContain('replay-drawer__attribution--generating');
      expect(pill.getAttribute('aria-busy')).toBe('true');
    });

    it('tooltip text follows the on/off state when enabled', () => {
      const { rerender } = render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={false}
          onAttributionChange={vi.fn()}
        />,
      );
      const pill = screen.getByLabelText(/Authors/);
      fireEvent.focus(pill);
      expect(screen.getByRole('tooltip').textContent).toBe('Show authors overlay');
      fireEvent.blur(pill);

      rerender(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={true}
          onAttributionChange={vi.fn()}
        />,
      );
      fireEvent.focus(screen.getByLabelText(/Authors/));
      expect(screen.getByRole('tooltip').textContent).toBe('Hide authors overlay');
    });

    it('renders disabled with an explanatory tooltip when attributionDisabled is true', () => {
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={false}
          onAttributionChange={vi.fn()}
          attributionDisabled={true}
        />,
      );
      const pill = screen.getByLabelText(/unavailable/);
      expect((pill as HTMLButtonElement).disabled).toBe(true);
      // Disabled buttons swallow pointer events, so the tooltip's hover
      // handlers live on its wrapper span. Hover shows after a 400ms delay.
      vi.useFakeTimers();
      fireEvent.mouseOver(pill.parentElement!);
      act(() => vi.advanceTimersByTime(450));
      expect(screen.getByRole('tooltip').textContent).toMatch(/not available for this format/);
      vi.useRealTimers();
    });

    it('suppresses on/generating modifier classes while disabled', () => {
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={true}
          onAttributionChange={vi.fn()}
          attributionGenerating={true}
          attributionDisabled={true}
        />,
      );
      const pill = screen.getByLabelText(/unavailable/);
      expect(pill.className).not.toContain('replay-drawer__attribution--on');
      expect(pill.className).not.toContain('replay-drawer__attribution--generating');
    });

    it('does not fire onAttributionChange when clicked while disabled', () => {
      const onChange = vi.fn();
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          attributionOn={false}
          onAttributionChange={onChange}
          attributionDisabled={true}
        />,
      );
      fireEvent.click(screen.getByLabelText(/unavailable/));
      expect(onChange).not.toHaveBeenCalled();
    });
  });

  // bd-ew0vak6b: the Edit pill turns q2-preview block editing on/off.
  // Same anatomy as the Authors pill; rendered only when both props are
  // supplied. Labels: "Editing on" / "Editing off" / "Editing unavailable…".
  describe('Edit toggle', () => {
    it('is hidden when the props are not supplied', () => {
      render(<ReplayDrawer state={makeState()} controls={controls} />);
      expect(screen.queryByLabelText(/Editing/)).toBeNull();
    });

    it('renders in collapsed state when previewEditing + onPreviewEditingChange are passed', () => {
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          previewEditing={true}
          onPreviewEditingChange={vi.fn()}
        />,
      );
      expect(screen.getByLabelText(/Editing on/)).toBeDefined();
    });

    it('renders in expanded state', () => {
      const activeState = makeState({
        isActive: true,
        historyLength: 100,
        currentIndex: 42,
        currentContent: 'hello',
        currentHeads: null,
        timestamp: 1710000000,
        actor: 'abcdef0123456789abcdef0123456789',
        chunkActors: [],
      });
      render(
        <ReplayDrawer
          state={activeState}
          controls={controls}
          previewEditing={true}
          onPreviewEditingChange={vi.fn()}
        />,
      );
      expect(screen.getByLabelText(/Editing on/)).toBeDefined();
    });

    it('reflects previewEditing via aria-pressed and the on-class', () => {
      const { rerender } = render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          previewEditing={true}
          onPreviewEditingChange={vi.fn()}
        />,
      );
      const on = screen.getByLabelText(/Editing on/);
      expect(on.getAttribute('aria-pressed')).toBe('true');
      expect(on.className).toContain('replay-drawer__edit--on');

      rerender(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          previewEditing={false}
          onPreviewEditingChange={vi.fn()}
        />,
      );
      const off = screen.getByLabelText(/Editing off/);
      expect(off.getAttribute('aria-pressed')).toBe('false');
      expect(off.className).not.toContain('replay-drawer__edit--on');
    });

    it('clicking calls onPreviewEditingChange with the negation', () => {
      const onChange = vi.fn();
      const { rerender } = render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          previewEditing={true}
          onPreviewEditingChange={onChange}
        />,
      );
      fireEvent.click(screen.getByLabelText(/Editing on/));
      expect(onChange).toHaveBeenCalledWith(false);

      rerender(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          previewEditing={false}
          onPreviewEditingChange={onChange}
        />,
      );
      fireEvent.click(screen.getByLabelText(/Editing off/));
      expect(onChange).toHaveBeenLastCalledWith(true);
    });

    it('clicking does not trigger replay enter', () => {
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          previewEditing={true}
          onPreviewEditingChange={vi.fn()}
        />,
      );
      fireEvent.click(screen.getByLabelText(/Editing on/));
      expect(controls.enter).not.toHaveBeenCalled();
    });

    it('renders disabled (no aria-pressed, no on-class) for unsupported formats', () => {
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          previewEditing={true}
          onPreviewEditingChange={vi.fn()}
          previewEditingDisabled={true}
        />,
      );
      const pill = screen.getByLabelText(/Editing unavailable/) as HTMLButtonElement;
      expect(pill.disabled).toBe(true);
      expect(pill.getAttribute('aria-pressed')).toBeNull();
      expect(pill.className).not.toContain('replay-drawer__edit--on');
    });

    it('does not fire onPreviewEditingChange when clicked while disabled', () => {
      const onChange = vi.fn();
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          previewEditing={true}
          onPreviewEditingChange={onChange}
          previewEditingDisabled={true}
        />,
      );
      fireEvent.click(screen.getByLabelText(/Editing unavailable/));
      expect(onChange).not.toHaveBeenCalled();
    });

    it('renders before the Authors pill in the right-hand cluster', () => {
      render(
        <ReplayDrawer
          state={makeState()}
          controls={controls}
          previewEditing={true}
          onPreviewEditingChange={vi.fn()}
          attributionOn={false}
          onAttributionChange={vi.fn()}
        />,
      );
      const edit = screen.getByLabelText(/Editing on/);
      const authors = screen.getByLabelText(/Authors/);
      // DOCUMENT_POSITION_FOLLOWING (4): `authors` comes after `edit`.
      expect(edit.compareDocumentPosition(authors) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });
  });
});
