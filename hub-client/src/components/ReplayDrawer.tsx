import { useCallback } from 'react';
import type { ReplayState, ReplayControls } from '../hooks/useReplayMode';
import './ReplayDrawer.css';

interface Props {
  state: ReplayState;
  controls: ReplayControls;
}

function formatTimestamp(ts: number | null): string {
  if (ts === null) return '';
  const date = new Date(ts);
  return date.toLocaleString();
}

export default function ReplayDrawer({ state, controls }: Props) {
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

  if (!state.isActive) {
    return (
      <div className="replay-drawer replay-drawer--collapsed">
        <button className="replay-drawer__toggle" onClick={controls.enter}>
          <span className="replay-drawer__icon">&#128339;</span>
          <span>History</span>
        </button>
      </div>
    );
  }

  return (
    <div
      className="replay-drawer replay-drawer--expanded"
      onKeyDown={handleKeyDown}
      tabIndex={0}
    >
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
        </div>

        <div className="replay-drawer__scrubber">
          <input
            type="range"
            min={0}
            max={state.historyLength - 1}
            value={state.currentIndex}
            onChange={handleScrubberChange}
            className="replay-drawer__slider"
            role="slider"
          />
        </div>

        <div className="replay-drawer__info">
          <span className="replay-drawer__position">
            {state.currentIndex + 1} of {state.historyLength}
          </span>
          {state.timestamp && (
            <span className="replay-drawer__timestamp">
              {formatTimestamp(state.timestamp)}
            </span>
          )}
        </div>

        <div className="replay-drawer__actions">
          <button
            className="replay-drawer__btn replay-drawer__btn--apply"
            onClick={controls.apply}
          >
            Apply
          </button>
          <button
            className="replay-drawer__btn replay-drawer__btn--close"
            onClick={controls.exit}
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
