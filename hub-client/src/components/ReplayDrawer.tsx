import { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import type { ReplayState, ReplayControls } from '../hooks/useReplayMode';
import { actorColor } from '../utils/palette';
import type { ActorIdentity } from '@quarto/preview-runtime';
import { getActorId } from '@quarto/preview-runtime';
import './ReplayDrawer.css';
import './ViewToggleControl.css';

interface Props {
  state: ReplayState;
  controls: ReplayControls;
  disabled?: boolean;
  identities?: Record<string, ActorIdentity>;
  /**
   * Attribution overlay state. Lives alongside replay because both
   * surfaces share the same per-actor colour palette and the
   * attribution inspection is a peer of replay. Both props must be
   * supplied to render the toggle; if either is omitted (e.g. in
   * a non-editor surface) the toggle is hidden.
   *
   * Session-only — kept as React state in the parent, never
   * persisted, so the overlay resets on reload.
   */
  attributionOn?: boolean;
  onAttributionChange?: (next: boolean) => void;
  /**
   * Whether the attribution producer (`useAttribution` in
   * ReactPreview) is currently building or updating the payload.
   * When true the pill border animates with a rotating gradient so
   * the user knows work is happening on a large document. Default
   * `false`; effectively gated by `attributionOn` upstream because
   * the hook only generates when the toggle is on.
   */
  attributionGenerating?: boolean;
  /**
   * When true the pill renders greyed-out and non-interactive. Used
   * for formats that don't surface attribution visually
   * (everything but q2-debug / q2-preview today). The `attributionOn`
   * state is preserved across the disabled period so toggling back
   * to a supported format restores the user's previous preference.
   */
  attributionDisabled?: boolean;
  /**
   * Comment-bubble display mode (three-way toggle rendered beside the
   * Authors pill): 'expand' pins every comment popup open, 'show' is
   * the default hover/click chrome, 'hide' removes the bubbles. Both
   * props must be supplied to render the toggle. Session-only, like
   * the attribution toggle.
   */
  commentsMode?: CommentsMode;
  onCommentsModeChange?: (next: CommentsMode) => void;
}

type CommentsMode = 'expand' | 'show' | 'hide';

/**
 * Three small square buttons in the `view-toggle-btn` style (same CSS
 * as the header's layout toggle): expand all comments / show bubbles /
 * hide bubbles.
 */
function CommentsModeToggle({
  mode,
  onChange,
}: {
  mode: CommentsMode;
  onChange: (next: CommentsMode) => void;
}) {
  // Speech-bubble outline shared by the show/hide icons; the expand
  // icon is the same bubble with a taller body.
  const bubblePath =
    'M1 0 h10 a1 1 0 0 1 1 1 v5 a1 1 0 0 1 -1 1 H5 L2 10 V7 H1 a1 1 0 0 1 -1 -1 V1 a1 1 0 0 1 1 -1 Z';
  const tallBubblePath =
    'M1 0 h10 a1 1 0 0 1 1 1 v6 a1 1 0 0 1 -1 1 H5 L2 10 V8 H1 a1 1 0 0 1 -1 -1 V1 a1 1 0 0 1 1 -1 Z';
  return (
    <div
      className="view-toggle-control"
      role="group"
      aria-label="Comment display mode"
      style={{ marginLeft: '6px' }}
    >
      <button
        className={`view-toggle-btn${mode === 'expand' ? ' active' : ''}`}
        onClick={(e) => {
          e.stopPropagation();
          onChange('expand');
        }}
        title="Expand all comments"
        aria-label="Expand comments"
      >
        <svg width="12" height="10" viewBox="0 0 12 10">
          <path d={tallBubblePath} fill="currentColor" />
        </svg>
      </button>
      <button
        className={`view-toggle-btn${mode === 'show' ? ' active' : ''}`}
        onClick={(e) => {
          e.stopPropagation();
          onChange('show');
        }}
        title="Show comment bubbles"
        aria-label="Show comments"
      >
        <svg width="12" height="10" viewBox="0 0 12 10">
          <path d={bubblePath} fill="currentColor" />
        </svg>
      </button>
      <button
        className={`view-toggle-btn${mode === 'hide' ? ' active' : ''}`}
        onClick={(e) => {
          e.stopPropagation();
          onChange('hide');
        }}
        title="Hide comment bubbles"
        aria-label="Hide comments"
      >
        <svg width="12" height="10" viewBox="0 0 12 10">
          <path d={bubblePath} fill="currentColor" opacity="0.25" />
          <line x1="1" y1="9" x2="11" y2="1" stroke="currentColor" strokeWidth="1.5" />
        </svg>
      </button>
    </div>
  );
}

interface AttributionToggleProps {
  attributionOn: boolean;
  onAttributionChange: (next: boolean) => void;
  generating: boolean;
  disabled: boolean;
}

function AttributionToggle({ attributionOn, onAttributionChange, generating, disabled }: AttributionToggleProps) {
  const classes = [
    'replay-drawer__attribution',
    attributionOn && !disabled && 'replay-drawer__attribution--on',
    generating && !disabled && 'replay-drawer__attribution--generating',
  ]
    .filter(Boolean)
    .join(' ');
  const titleText = disabled
    ? 'Authors overlay is not available for this format'
    : attributionOn
      ? 'Hide authors overlay'
      : 'Show authors overlay';
  const ariaLabel = disabled
    ? 'Authors overlay unavailable for this format'
    : `Authors overlay ${attributionOn ? 'on' : 'off'}`;
  return (
    <button
      type="button"
      className={classes}
      onClick={(e) => {
        e.stopPropagation();
        onAttributionChange(!attributionOn);
      }}
      disabled={disabled}
      aria-pressed={disabled ? undefined : attributionOn}
      aria-label={ariaLabel}
      aria-busy={generating && !disabled || undefined}
      title={titleText}
    >
      <span className="replay-drawer__attribution-dot" />
      <span className="replay-drawer__attribution-label">Authors</span>
    </button>
  );
}

function formatRelativeTime(ts: number): string {
  const now = Date.now();
  const diffMs = now - ts * 1000;
  const diffSec = Math.floor(diffMs / 1000);
  if (diffSec < 60) return 'just now';
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDays = Math.floor(diffHr / 24);
  if (diffDays < 30) return `${diffDays}d ago`;
  // Beyond 30 days, show short date
  const date = new Date(ts * 1000);
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function formatTimestamp(ts: number | null): string {
  if (ts === null) return '';
  return formatRelativeTime(ts);
}

function formatFullTimestamp(ts: number | null): string {
  if (ts === null) return '';
  const date = new Date(ts * 1000);
  return date.toLocaleString();
}

export default function ReplayDrawer({
  state,
  controls,
  disabled,
  identities,
  attributionOn,
  onAttributionChange,
  attributionGenerating,
  attributionDisabled,
  commentsMode,
  onCommentsModeChange,
}: Props) {
  const showAttributionToggle =
    attributionOn !== undefined && onAttributionChange !== undefined;
  const showCommentsToggle =
    commentsMode !== undefined && onCommentsModeChange !== undefined;
  const currentActorId = getActorId();
  const drawerRef = useRef<HTMLDivElement>(null);

  // Auto-focus the drawer when replay mode activates so keyboard shortcuts work immediately
  useEffect(() => {
    if (state.isActive) {
      drawerRef.current?.focus();
    }
  }, [state.isActive]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (!state.isActive) return;

    switch (e.key) {
      case ' ':
        e.preventDefault();
        if (state.isPlaying) {
          controls.pause();
        } else {
          controls.play();
        }
        break;
      case 'ArrowLeft':
        e.preventDefault();
        controls.stepBackward();
        break;
      case 'ArrowRight':
        e.preventDefault();
        controls.stepForward();
        break;
      case 'Home':
        e.preventDefault();
        controls.seekToStart();
        break;
      case 'End':
        e.preventDefault();
        controls.seekToEnd();
        break;
      case 'Escape':
        e.preventDefault();
        controls.exit();
        break;
    }
  }, [state.isActive, state.isPlaying, controls]);

  const handleScrubberChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    controls.seekTo(parseInt(e.target.value, 10));
  }, [controls]);

  // Tooltip state for scrubber hover
  const [scrubberTooltip, setScrubberTooltip] = useState<{ left: number; text: string } | null>(null);
  const scrubberRef = useRef<HTMLDivElement>(null);

  const handleScrubberMouseMove = useCallback((e: React.MouseEvent<HTMLInputElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const fraction = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    const index = Math.round(fraction * (state.historyLength - 1));
    const ts = controls.getTimestampAtIndex(index);
    const text = ts !== null ? formatFullTimestamp(ts) : `Change ${index + 1}`;
    // Position relative to the scrubber container
    const left = e.clientX - (scrubberRef.current?.getBoundingClientRect().left ?? rect.left);
    setScrubberTooltip({ left, text });
  }, [state.historyLength, controls]);

  const handleScrubberMouseLeave = useCallback(() => {
    setScrubberTooltip(null);
  }, []);

  // Resolve an actor's color: prefer identity color from the index document, fall back to hash-based.
  const resolveActorColor = useCallback((actor: string): string => {
    const identity = identities?.[actor];
    return identity?.color || actorColor(actor);
  }, [identities]);

  // Build per-chunk stacked rects: each chunk is a vertical column, split by actor fractions.
  const chunkRects = useMemo(() => {
    const chunks = state.chunkActors;
    const n = chunks.length || 1;
    const chunkWidth = 100 / n;
    const rects: { x: number; y: number; width: number; height: number; color: string }[] = [];
    for (let i = 0; i < chunks.length; i++) {
      const x = i * chunkWidth;
      let y = 0;
      for (const { actor, fraction } of chunks[i]) {
        rects.push({ x, y, width: chunkWidth, height: fraction, color: resolveActorColor(actor) });
        y += fraction;
      }
    }
    return rects;
  }, [state.chunkActors, resolveActorColor]);

  if (!state.isActive) {
    return (
      <div className="replay-drawer replay-drawer--collapsed">
        <button
          className="replay-drawer__toggle"
          onClick={disabled ? undefined : controls.enter}
          disabled={disabled}
          title={disabled ? 'Replay is not available for binary files' : undefined}
        >
          <span className="replay-drawer__chevron">&#x25B6;</span>
          <span>Replay</span>
        </button>
        {showAttributionToggle && (
          <AttributionToggle
            attributionOn={attributionOn!}
            onAttributionChange={onAttributionChange!}
            generating={!!attributionGenerating}
            disabled={!!attributionDisabled}
          />
        )}
        {showCommentsToggle && (
          <CommentsModeToggle mode={commentsMode!} onChange={onCommentsModeChange!} />
        )}
      </div>
    );
  }

  const progressPercent = state.historyLength > 1
    ? (state.currentIndex / (state.historyLength - 1)) * 100
    : 0;

  return (
    <div
      ref={drawerRef}
      className="replay-drawer replay-drawer--expanded"
      onKeyDown={handleKeyDown}
      tabIndex={0}
    >
      <div className="replay-drawer__header">
        <button
          type="button"
          className="replay-drawer__toggle"
          onClick={controls.exit}
          aria-label="Collapse replay"
        >
          <span className="replay-drawer__chevron">&#x25BC;</span>
          <span>Replay</span>
        </button>

        <button
          className="replay-drawer__handle"
          onClick={controls.exit}
          aria-label="Close replay"
        >
          <span className="replay-drawer__handle-bar" />
        </button>

        <div className="replay-drawer__info">
          {state.timestamp && (
            <span className="replay-drawer__timestamp">
              <span className="replay-drawer__relative">{formatTimestamp(state.timestamp)}</span>
              <span className="replay-drawer__absolute">{formatFullTimestamp(state.timestamp)}</span>
            </span>
          )}
          {state.actor && (
            <span className={`replay-drawer__actor${currentActorId === state.actor ? ' replay-drawer__actor--me' : ''}`}>
              <span
                className="replay-drawer__actor-dot"
                style={{ backgroundColor: resolveActorColor(state.actor) }}
              />
              {identities?.[state.actor]?.name || state.actor.slice(0, 8)}
            </span>
          )}
          <span className="replay-drawer__position">
            {state.currentIndex + 1}/{state.historyLength}
          </span>
        </div>

        {showAttributionToggle && (
          <AttributionToggle
            attributionOn={attributionOn!}
            onAttributionChange={onAttributionChange!}
            generating={!!attributionGenerating}
            disabled={!!attributionDisabled}
          />
        )}
        {showCommentsToggle && (
          <CommentsModeToggle mode={commentsMode!} onChange={onCommentsModeChange!} />
        )}
      </div>

      <div className="replay-drawer__controls">
        <div className="replay-drawer__transport">
          <button
            className="replay-drawer__btn"
            onClick={controls.seekToStart}
            aria-label="Skip to start"
          >
            &#x23EE;
          </button>
          <button
            className="replay-drawer__btn"
            onClick={controls.stepBackward}
            aria-label="Step backward"
          >
            &#x25C1;
          </button>
          {state.isPlaying ? (
            <button
              className="replay-drawer__btn replay-drawer__btn--play"
              onClick={controls.pause}
              aria-label="Pause"
            >
              &#x23F8;
            </button>
          ) : (
            <button
              className="replay-drawer__btn replay-drawer__btn--play"
              onClick={controls.play}
              aria-label="Play"
            >
              &#x25B6;
            </button>
          )}
          <button
            className="replay-drawer__btn"
            onClick={controls.stepForward}
            aria-label="Step forward"
          >
            &#x25B7;
          </button>
          <button
            className="replay-drawer__btn"
            onClick={controls.seekToEnd}
            aria-label="Skip to end"
          >
            &#x23ED;
          </button>
          <button
            className="replay-drawer__btn replay-drawer__btn--speed"
            onClick={controls.cycleSpeed}
            aria-label="Playback speed"
          >
            {state.playbackSpeed}x
          </button>
        </div>

        <div className="replay-waveform-container" ref={scrubberRef}>
          <svg
            className="replay-waveform"
            viewBox="0 0 100 1"
            preserveAspectRatio="none"
          >
            {/* Background */}
            <rect width={100} height={1} fill="#1f3460" />
            {/* Actor-colored chunk rects */}
            {chunkRects.map((r, i) => (
              <rect key={i} x={r.x} y={r.y} width={r.width} height={r.height} fill={r.color} />
            ))}
            {/* Dim the portion past the playhead */}
            <rect x={progressPercent} y={0} width={100 - progressPercent} height={1} fill="rgba(0,0,0,0.6)" />
            {/* Playhead */}
            <line
              x1={progressPercent} y1={0}
              x2={progressPercent} y2={1}
              stroke="rgba(255,255,255,0.6)"
              strokeWidth={0.3}
            />
          </svg>
          <input
            type="range"
            min={0}
            max={state.historyLength - 1}
            value={state.currentIndex}
            onChange={handleScrubberChange}
            onMouseMove={handleScrubberMouseMove}
            onMouseLeave={handleScrubberMouseLeave}
            className="replay-waveform__input"
            role="slider"
          />
          {scrubberTooltip && (
            <div
              className="replay-drawer__tooltip"
              style={{ left: scrubberTooltip.left }}
            >
              {scrubberTooltip.text}
            </div>
          )}
        </div>

        <button
          className="replay-drawer__btn replay-drawer__btn--apply"
          onClick={controls.apply}
        >
          Restore
        </button>
      </div>
    </div>
  );
}
