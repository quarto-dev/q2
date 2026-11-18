/**
 * SplitFrame — two slots side by side with a draggable divider between
 * them. The editor/preview split of `Editor.tsx` as a component of its
 * own, with the same behaviour:
 *
 * - the split starts at 50/50 and is session-only;
 * - dragging the divider resizes instantly, clamped to 15%–85%;
 * - `setFraction` (the layout preset buttons) animates to the new split;
 * - while dragging, iframes in the slots do not swallow the pointer.
 *
 * A slot given `null` is not rendered: the other one takes the whole frame
 * and there is no divider (what the editor does for a file with no preview).
 *
 * PROTOTYPE: `Editor.tsx` still has its own copy of this.
 */

import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState, type ReactNode } from 'react';
import './SplitFrame.css';

/** How far the divider can be dragged, as the left slot's share. */
export const SPLIT_MIN = 0.15;
export const SPLIT_MAX = 0.85;

/** Matches the CSS transition (0.3s) with a little slack. */
const ANIMATION_MS = 350;

export interface SplitFrameHandle {
  /** Jump the divider to `fraction` (the left slot's share), animated. Not clamped. */
  setFraction: (fraction: number) => void;
}

export interface SplitFrameProps {
  left: ReactNode | null;
  right: ReactNode | null;
  /** The left slot's share at first. Default: 0.5. */
  defaultFraction?: number;
  /** Fires on every change of the split, dragged or set. */
  onFractionChange?: (fraction: number) => void;
  /** The divider's accessible name. */
  dividerLabel?: string;
  className?: string;
}

const SplitFrame = forwardRef<SplitFrameHandle, SplitFrameProps>(function SplitFrame(
  { left, right, defaultFraction = 0.5, onFractionChange, dividerLabel = 'Resize panes', className },
  ref,
) {
  const [fraction, setFractionState] = useState(defaultFraction);
  const [dragging, setDragging] = useState(false);
  // True briefly while `setFraction` animates; drags stay instant.
  const [animating, setAnimating] = useState(false);
  const animationTimer = useRef<number | null>(null);

  const onFractionChangeRef = useRef(onFractionChange);
  useEffect(() => {
    onFractionChangeRef.current = onFractionChange;
  });

  const update = useCallback((next: number) => {
    setFractionState(next);
    onFractionChangeRef.current?.(next);
  }, []);

  useImperativeHandle(ref, () => ({
    setFraction: (next) => {
      update(next);
      setAnimating(true);
      if (animationTimer.current !== null) window.clearTimeout(animationTimer.current);
      animationTimer.current = window.setTimeout(() => setAnimating(false), ANIMATION_MS);
    },
  }), [update]);

  useEffect(() => () => {
    if (animationTimer.current !== null) window.clearTimeout(animationTimer.current);
  }, []);

  const split = left !== null && right !== null;
  const classes = [
    'split-frame',
    dragging ? 'split-frame--dragging' : '',
    animating ? 'split-frame--animating' : '',
    className ?? '',
  ].filter((name) => name !== '').join(' ');

  return (
    <div className={classes}>
      {left !== null && (
        <div className="split-frame-pane" style={split ? { flex: `${fraction} 1 0%` } : undefined}>
          {left}
        </div>
      )}
      {split && (
        <div
          className="split-frame-divider"
          role="separator"
          aria-orientation="vertical"
          aria-label={dividerLabel}
          onPointerDown={(event) => {
            event.preventDefault();
            event.currentTarget.setPointerCapture(event.pointerId);
            setDragging(true);
          }}
          onPointerMove={(event) => {
            if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
            const frame = event.currentTarget.parentElement;
            if (frame === null) return;
            const rect = frame.getBoundingClientRect();
            const dragged = (event.clientX - rect.left) / rect.width;
            update(Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, dragged)));
          }}
          onPointerUp={(event) => {
            event.currentTarget.releasePointerCapture(event.pointerId);
            setDragging(false);
          }}
        />
      )}
      {right !== null && (
        <div className="split-frame-pane" style={split ? { flex: `${1 - fraction} 1 0%` } : undefined}>
          {right}
        </div>
      )}
    </div>
  );
});

export default SplitFrame;
