/**
 * Tooltip — the single tooltip primitive for hub-client, replacing native
 * `title` attributes (which are unstyled, delay-less, and inaccessible to
 * touch/keyboard users).
 *
 * Behavior (WAI-ARIA APG tooltip pattern):
 * - Shows after a 400ms hover delay; immediately on keyboard focus
 * - Wired via `aria-describedby` on the wrapped element
 * - Escape dismisses; hides on pointer-leave and blur
 * - Sits below the anchor (the native title-tooltip position); flips
 *   above only when it would overflow the viewport bottom, and clamps
 *   horizontally inside the viewport
 * - Non-interactive content only — never put focusable elements inside
 *
 * The bubble renders in a portal on document.body with fixed positioning,
 * so it is never clipped by overflow containers (the sidebar is
 * overflow-hidden and would cut off an in-place bubble — native title
 * tooltips never clipped because the OS drew them).
 *
 * Usage: wrap a single element that accepts aria-describedby and event
 * handlers (a DOM element or a component forwarding them):
 *
 *   <Tooltip content="Switch project"><button …/></Tooltip>
 *
 * Phase 1 deliverable of the UI/UX modernization (bd-iguk0hpd).
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
import { createPortal } from 'react-dom';
import './Tooltip.css';

const HOVER_DELAY_MS = 400;
const VIEWPORT_MARGIN = 4;
const GAP = 6;

export interface TooltipProps {
  content: ReactNode;
  /** The wrapped child is a block-level row (file items, cards): the
   * wrapper renders display:contents so the row keeps its layout. */
  block?: boolean;
  children: ReactElement;
}

export default function Tooltip({ content, block, children }: TooltipProps) {
  const [visible, setVisible] = useState(false);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
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
    setPos(null);
  }, []);

  useEffect(() => () => window.clearTimeout(delayTimer.current), []);

  // Measure the anchor (the wrapped child, not the wrapper span — a block
  // wrapper is display:contents and has no box) and place the bubble
  // centered below it — the native title-tooltip position — flipping
  // above only at the viewport bottom and clamping horizontally.
  const updatePosition = useCallback(() => {
    const anchor = rootRef.current?.firstElementChild ?? rootRef.current;
    const tip = tipRef.current;
    if (!anchor || !tip) return;
    const a = anchor.getBoundingClientRect();
    const t = tip.getBoundingClientRect();
    let x = a.left + a.width / 2 - t.width / 2;
    x = Math.max(
      VIEWPORT_MARGIN,
      Math.min(x, window.innerWidth - VIEWPORT_MARGIN - t.width),
    );
    const fitsBelow = a.bottom + GAP + t.height <= window.innerHeight - VIEWPORT_MARGIN;
    const y = fitsBelow ? a.bottom + GAP : a.top - t.height - GAP;
    setPos({ x, y });
  }, []);

  useEffect(() => {
    if (!visible) return;
    updatePosition();
    // Track the anchor while visible (scrolling a sidebar under an open
    // tooltip would otherwise leave the bubble behind).
    window.addEventListener('scroll', updatePosition, true);
    window.addEventListener('resize', updatePosition);
    return () => {
      window.removeEventListener('scroll', updatePosition, true);
      window.removeEventListener('resize', updatePosition);
    };
  }, [visible, updatePosition]);

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
    <>
      <span
        ref={rootRef}
        className={`qh-tooltip-anchor${block ? ' qh-tooltip-anchor-block' : ''}`}
        onMouseEnter={() => show(false)}
        onMouseLeave={hide}
      >
        {wrapped}
      </span>
      {visible &&
        createPortal(
          <span
            ref={tipRef}
            role="tooltip"
            id={id}
            className="qh-tooltip"
            style={{
              // Hidden until measured, so it never flashes unpositioned.
              visibility: pos ? 'visible' : 'hidden',
              ...(pos ? { top: pos.y, left: pos.x } : {}),
            }}
          >
            {content}
          </span>,
          document.body,
        )}
    </>
  );
}
