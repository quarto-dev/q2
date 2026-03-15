import { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import type { ReplayState, ReplayControls } from '../hooks/useReplayMode';
import './ReplayDrawer.css';

interface Props {
  state: ReplayState;
  controls: ReplayControls;
  disabled?: boolean;
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

export default function ReplayDrawer({ state, controls, disabled }: Props) {
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

  // Two stops per chunk (start + end at same color) to prevent SVG interpolation.
  // Lerps from blue (1 author) to orange (5+ authors).
  const gradientStops = useMemo(() => {
    const chunks = state.chunkAuthors;
    const n = chunks.length || 1;
    const lerp = (a: number, b: number, t: number) => Math.round(a + (b - a) * t);
    const stops: { color: string; offset: number }[] = [];
    for (let i = 0; i < chunks.length; i++) {
      const t = Math.min(1, Math.max(0, (chunks[i] - 1) / 4));
      const color = `rgb(${lerp(74, 204, t)},${lerp(128, 122, t)},${lerp(204, 74, t)})`;
      stops.push({ color, offset: (i / n) * 100 });
      stops.push({ color, offset: ((i + 1) / n) * 100 });
    }
    return stops;
  }, [state.chunkAuthors]);

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
      <div className="replay-drawer__header" onClick={controls.exit} role="button" aria-label="Collapse history">
        <span className="replay-drawer__toggle">
          <span className="replay-drawer__chevron">&#x25BC;</span>
          <span>Replay</span>
        </span>

        <div className="replay-drawer__info">
          <span className="replay-drawer__position">
            {state.currentIndex + 1} of {state.historyLength}
          </span>
          {state.timestamp && (
            <span className="replay-drawer__timestamp">
              {formatFullTimestamp(state.timestamp)}
              <span className="replay-drawer__relative">{formatTimestamp(state.timestamp)}</span>
            </span>
          )}
        </div>

        <button
          className="replay-drawer__btn replay-drawer__btn--apply"
          onClick={(e) => { e.stopPropagation(); controls.apply(); }}
        >
          Restore
        </button>
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
            <defs>
              <linearGradient id="replay-waveform-grad" gradientUnits="userSpaceOnUse" x1="0" x2="100" y1="0" y2="0">
                {gradientStops.map((s, i) => (
                  <stop key={i} offset={`${s.offset}%`} stopColor={s.color} />
                ))}
              </linearGradient>
            </defs>
            <rect width={100} height={1} fill="#1f3460" />
            <rect width={progressPercent} height={1} fill="url(#replay-waveform-grad)" />
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
      </div>
    </div>
  );
}
