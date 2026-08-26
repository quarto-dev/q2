/**
 * Tooltip — the single tooltip primitive for hub-client, replacing native
 * `title` attributes (which are unstyled, delay-less, and inaccessible to
 * touch/keyboard users).
 *
 * Behavior (WAI-ARIA APG tooltip pattern):
 * - Shows after a 400ms hover delay; immediately on keyboard focus
 * - Wired via `aria-describedby` on the wrapped element
 * - Escape dismisses; hides on pointer-leave and blur
 * - Flips below the anchor when it would overflow the viewport top,
 *   clamped horizontally inside the viewport
 * - Non-interactive content only — never put focusable elements inside
 *
 * Usage: wrap a single element that accepts aria-describedby and event
 * handlers (a DOM element or a component forwarding them):
 *
 *   <Tooltip content="Switch project"><button …/></Tooltip>
 *
 * Phase 1 deliverable of the UI/UX modernization plan (bd-iguk0hpd).
 */

import {
  useState,
  useRef,
  useId,
  useCallback,
  useEffect,
  cloneElement,
  isValidElement,
  type ReactElement,
  type ReactNode,
} from 'react';
import './Tooltip.css';

const HOVER_DELAY_MS = 400;

export interface TooltipProps {
  content: ReactNode;
  /** Render the anchor as display:contents so block-level children (file
   * rows, cards) keep their layout. */
  block?: boolean;
  children: ReactElement;
}

export default function Tooltip({ content, block, children }: TooltipProps) {
  const [visible, setVisible] = useState(false);
  const [placement, setPlacement] = useState<'top' | 'bottom'>('top');
  const id = useId();
  const delayTimer = useRef(0);
  const rootRef = useRef<HTMLSpanElement>(null);
  const tipRef = useRef<HTMLSpanElement>(null);

  const show = useCallback((immediate: boolean) => {
    window.clearTimeout(delayTimer.current);
    if (immediate) {
      setVisible(true);
    } else {
      delayTimer.current = window.setTimeout(() => setVisible(true), HOVER_DELAY_MS);
    }
  }, []);

  const hide = useCallback(() => {
    window.clearTimeout(delayTimer.current);
    setVisible(false);
  }, []);

  useEffect(() => () => window.clearTimeout(delayTimer.current), []);

  // Viewport-edge flip + horizontal clamp, measured after the tooltip
  // renders.
  useEffect(() => {
    if (!visible) return;
    const anchor = rootRef.current;
    const tip = tipRef.current;
    if (!anchor || !tip) return;
    const anchorRect = anchor.getBoundingClientRect();
    const tipRect = tip.getBoundingClientRect();
    const margin = 4;
    const fitsAbove = anchorRect.top - tipRect.height - margin >= 0;
    setPlacement(fitsAbove ? 'top' : 'bottom');
    const overflowRight = tipRect.right - (window.innerWidth - margin);
    if (overflowRight > 0) {
      tip.style.transform = `translateX(${-overflowRight}px)`;
    }
    const overflowLeft = tipRect.left - margin;
    if (overflowLeft < 0 && overflowRight <= 0) {
      tip.style.transform = `translateX(${-overflowLeft}px)`;
    }
  }, [visible]);

  if (!isValidElement(children)) return children;

  const child = children as ReactElement<Record<string, unknown>>;
  const childProps = child.props;

  // Hover handlers live on the wrapper span so disabled children (which
  // swallow pointer events) still get tooltips; focus/blur/Escape and the
  // aria-describedby wiring go on the child itself.
  const wrapped = cloneElement(child, {
    'aria-describedby': visible ? id : undefined,
    onFocus: (e: unknown) => {
      show(true);
      (childProps.onFocus as ((ev: unknown) => void) | undefined)?.(e);
    },
    onBlur: (e: unknown) => {
      hide();
      (childProps.onBlur as ((ev: unknown) => void) | undefined)?.(e);
    },
    onKeyDown: (e: unknown) => {
      if ((e as React.KeyboardEvent).key === 'Escape') hide();
      (childProps.onKeyDown as ((ev: unknown) => void) | undefined)?.(e);
    },
  });

  return (
    <span
      ref={rootRef}
      className={`qh-tooltip-anchor${block ? ' qh-tooltip-anchor-block' : ''}`}
      onMouseEnter={() => show(false)}
      onMouseLeave={hide}
    >
      {wrapped}
      {visible && (
        <span ref={tipRef} role="tooltip" id={id} className={`qh-tooltip qh-tooltip-${placement}`}>
          {content}
        </span>
      )}
    </span>
  );
}
