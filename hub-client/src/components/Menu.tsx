/**
 * Menu — the single accessible menu primitive for hub-client.
 *
 * Implements the WAI-ARIA APG menu/menu-button pattern:
 * - `role="menu"` container with `role="menuitem"` children
 * - ArrowUp/ArrowDown move focus (wrapping); Home/End jump to first/last
 * - Type-ahead: printable characters focus the next matching item
 * - ArrowRight opens a submenu (MenuSubmenu); ArrowLeft closes it
 * - Escape closes the menu and returns focus to the trigger
 * - Tab closes the menu (menus do not trap focus)
 * - Pointer-down outside the menu closes it (without stealing focus)
 * - The focused item is scrolled into view
 *
 * Usage: render inside a `.qh-menu-anchor` (position: relative) parent for
 * anchored placement, or pass `fixed={{ x, y }}` for context-menu placement
 * at cursor coordinates. The trigger element owns open/close state; pass it
 * via `triggerRef` so trigger clicks aren't treated as outside clicks and
 * focus returns to it on close.
 *
 * Destructive-action pattern (the single rule for the app): menu items
 * that destroy data use `danger` styling and must be guarded by a
 * confirmation dialog (see ProjectsHome's remove flow) unless the action
 * is trivially undoable. "Confirm vs undo" is decided per action at its
 * call site, but every destructive menu item must have one or the other.
 */

import {
  useEffect,
  useId,
  useRef,
  useCallback,
  useState,
  type ReactNode,
  type MouseEvent as ReactMouseEvent,
} from 'react';

export interface MenuProps {
  /** Close the menu. `returnFocus` is true for keyboard-driven closes. */
  onClose: (returnFocus: boolean) => void;
  /** Fixed-position placement (context menus). Omit for anchored placement. */
  fixed?: { x: number; y: number };
  /** The trigger element; clicks on it are not "outside" clicks. */
  triggerRef?: { current: HTMLElement | null };
  /** CSS selector whose matches are not "outside" clicks — use when the
   * trigger owns toggle behavior and sits inside a shared anchor (e.g.
   * '.qh-menu-anchor'). */
  ignoreOutsideSelector?: string;
  /** Accessible name for the menu. */
  'aria-label'?: string;
  className?: string;
  children: ReactNode;
}

const ITEM_SELECTOR = '[role="menuitem"]:not([aria-disabled="true"])';

export function Menu({
  onClose,
  fixed,
  triggerRef,
  ignoreOutsideSelector,
  'aria-label': ariaLabel,
  className = '',
  children,
}: MenuProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  // Element to return focus to on keyboard-driven close: the trigger if
  // given, otherwise whatever was focused when the menu opened.
  const returnFocusRef = useRef<HTMLElement | null>(null);
  if (returnFocusRef.current === null && typeof document !== 'undefined') {
    returnFocusRef.current =
      triggerRef?.current ??
      (document.activeElement instanceof HTMLElement ? document.activeElement : null);
  }

  const items = useCallback((): HTMLElement[] => {
    const root = rootRef.current;
    if (!root) return [];
    return Array.from(root.querySelectorAll<HTMLElement>(ITEM_SELECTOR));
  }, []);

  const focusItem = useCallback(
    (el: HTMLElement | undefined) => {
      if (!el) return;
      el.focus();
      el.scrollIntoView({ block: 'nearest' });
    },
    [],
  );

  const close = useCallback(
    (returnFocus: boolean) => {
      // Defer the focus return past the React commit: if the activating
      // item opened a dialog, the dialog owns focus now (its autoFocus
      // child is focused) and the menu must not steal it back.
      if (returnFocus) {
        const target = returnFocusRef.current;
        queueMicrotask(() => {
          if (document.activeElement?.closest('[role="dialog"]')) return;
          if (target?.isConnected) target.focus();
        });
      }
      onClose(returnFocus);
    },
    [onClose],
  );

  // Focus the first item on open.
  useEffect(() => {
    const first = items()[0];
    first?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Viewport-edge flip for fixed (context-menu) placement: if the menu
  // overflows the viewport, shift it back inside.
  useEffect(() => {
    const root = rootRef.current;
    if (!root || !fixed) return;
    const rect = root.getBoundingClientRect();
    const margin = 4;
    let { x, y } = fixed;
    if (rect.bottom > window.innerHeight - margin) {
      y = Math.max(margin, window.innerHeight - margin - rect.height);
    }
    if (rect.right > window.innerWidth - margin) {
      x = Math.max(margin, window.innerWidth - margin - rect.width);
    }
    if (x !== fixed.x || y !== fixed.y) {
      root.style.top = `${y}px`;
      root.style.left = `${x}px`;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Close on pointer-down outside the menu (and outside the trigger, which
  // owns its own toggle behavior).
  useEffect(() => {
    const onPointerDown = (e: PointerEvent) => {
      const target = e.target as HTMLElement;
      if (rootRef.current?.contains(target)) return;
      if (triggerRef?.current?.contains(target)) return;
      if (ignoreOutsideSelector && target.closest(ignoreOutsideSelector)) return;
      close(false);
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    return () => document.removeEventListener('pointerdown', onPointerDown, true);
  }, [close, triggerRef, ignoreOutsideSelector]);

  const typeAhead = useRef({ buffer: '', timer: 0 });
  useEffect(() => {
    const ta = typeAhead.current;
    return () => window.clearTimeout(ta.timer);
  }, []);

  const onKeyDown = (e: React.KeyboardEvent) => {
    const list = items();
    const activeIndex = list.indexOf(document.activeElement as HTMLElement);
    switch (e.key) {
      case 'ArrowDown':
      case 'ArrowUp': {
        e.preventDefault();
        const delta = e.key === 'ArrowDown' ? 1 : -1;
        const next =
          activeIndex === -1
            ? 0
            : (activeIndex + delta + list.length) % list.length;
        focusItem(list[next]);
        return;
      }
      case 'Home':
        e.preventDefault();
        focusItem(list[0]);
        return;
      case 'End':
        e.preventDefault();
        focusItem(list[list.length - 1]);
        return;
      case 'Escape':
        e.preventDefault();
        e.stopPropagation();
        close(true);
        return;
      case 'Tab':
        // Menus don't trap focus; close and let the tab proceed.
        close(false);
        return;
      default:
        break;
    }
    // Type-ahead: accumulate printable characters, focus the next item
    // whose text starts with the buffer. Space is excluded — it activates
    // the focused item rather than extending the buffer.
    if (e.key.length === 1 && e.key !== ' ' && !e.metaKey && !e.ctrlKey && !e.altKey) {
      const ta = typeAhead.current;
      window.clearTimeout(ta.timer);
      ta.buffer += e.key.toLowerCase();
      ta.timer = window.setTimeout(() => {
        ta.buffer = '';
      }, 500);
      const start = activeIndex === -1 ? 0 : activeIndex + 1;
      for (let i = 0; i < list.length; i += 1) {
        const candidate = list[(start + i) % list.length];
        if (candidate.textContent?.trim().toLowerCase().startsWith(ta.buffer)) {
          focusItem(candidate);
          break;
        }
      }
    }
  };

  // Activating an item closes the menu and returns focus to the trigger.
  // Items that need the menu to stay open (e.g. in-place "copied!"
  // feedback) stop propagation in their own handler, so this never runs.
  const onClick = (e: ReactMouseEvent) => {
    const item = (e.target as HTMLElement).closest('[role="menuitem"]');
    if (!item) return;
    // Disabled items don't activate — the click must not close the menu.
    if (item.getAttribute('aria-disabled') === 'true') return;
    close(true);
  };

  return (
    <div
      ref={rootRef}
      role="menu"
      aria-label={ariaLabel}
      className={`qh-menu${fixed ? ' qh-menu-fixed' : ''}${className ? ` ${className}` : ''}`}
      style={fixed ? { top: fixed.y, left: fixed.x } : undefined}
      onKeyDown={onKeyDown}
      onClick={onClick}
    >
      {children}
    </div>
  );
}

export interface MenuItemProps {
  onSelect: () => void;
  /** Destructive action — must be confirm-guarded or undoable (see header). */
  danger?: boolean;
  strong?: boolean;
  accent?: boolean;
  disabled?: boolean;
  /** Right-aligned hint text (e.g. a shortcut or id). */
  hint?: ReactNode;
  /** Second line of muted explanatory text. */
  subtext?: ReactNode;
  /** Keep the menu open on select (for in-place feedback like "Copied!"). */
  keepOpen?: boolean;
  children: ReactNode;
}

export function MenuItem({
  onSelect,
  danger,
  strong,
  accent,
  disabled,
  hint,
  subtext,
  keepOpen,
  children,
}: MenuItemProps) {
  const classes = [
    'qh-menu-item',
    danger ? 'danger' : '',
    strong ? 'strong' : '',
    accent ? 'accent' : '',
    hint ? 'with-hint' : '',
  ]
    .filter(Boolean)
    .join(' ');
  return (
    <button
      type="button"
      role="menuitem"
      className={classes}
      aria-disabled={disabled || undefined}
      onClick={(e) => {
        if (disabled) {
          e.preventDefault();
          return;
        }
        // keepOpen: the menu root's closer never sees the activation.
        if (keepOpen) e.stopPropagation();
        onSelect();
      }}
    >
      <span className="qh-menu-item-label">{children}</span>
      {hint && <span className="qh-menu-hint">{hint}</span>}
      {subtext && <span className="qh-menu-subtext">{subtext}</span>}
    </button>
  );
}

export function MenuDivider() {
  return <div className="qh-menu-divider" role="separator" />;
}

export function MenuLabel({ children }: { children: ReactNode }) {
  // role="presentation": a label is not a menuitem but may appear as a
  // child of role="menu".
  return (
    <div className="qh-menu-label" role="presentation">
      {children}
    </div>
  );
}

export interface MenuSubmenuProps {
  /** The parent item's label. */
  label: ReactNode;
  children: ReactNode;
}

/**
 * A submenu parent item. Opens on click, ArrowRight, or Enter/Space;
 * ArrowLeft inside the submenu closes it and refocuses this item.
 */
export function MenuSubmenu({ label, children }: MenuSubmenuProps) {
  const [open, setOpen] = useState(false);
  const itemRef = useRef<HTMLButtonElement>(null);
  const itemId = useId();

  // APG: activating the parent opens the submenu and focuses its first
  // item. Open-only, never a toggle: hover may already have opened the
  // submenu, and a toggling click would close it again under the pointer.
  const openAndFocusFirst = () => {
    setOpen(true);
    requestAnimationFrame(() => {
      const first = itemRef.current
        ?.closest('[data-submenu-parent]')
        ?.querySelector<HTMLElement>('.qh-submenu [role="menuitem"]');
      first?.focus();
    });
  };

  return (
    <div
      className="qh-menu-item qh-submenu-parent"
      data-submenu-parent
      // Hover parity with the previous ad-hoc menus: opening on hover is
      // a pointer convenience; keyboard uses ArrowRight/Enter.
      onMouseEnter={() => setOpen(true)}
      onMouseLeave={(e) => {
        // Don't collapse under the keyboard user: if focus is inside this
        // submenu, the mouse leaving must not drop the focused subtree.
        if (e.currentTarget.contains(document.activeElement)) return;
        setOpen(false);
      }}
    >
      <button
        type="button"
        role="menuitem"
        id={itemId}
        aria-haspopup="menu"
        aria-expanded={open}
        ref={itemRef}
        className="qh-menu-item-inner"
        onClick={(e) => {
          e.stopPropagation();
          openAndFocusFirst();
        }}
        onKeyDown={(e) => {
          if (e.key === 'ArrowRight' && !open) {
            e.preventDefault();
            e.stopPropagation();
            openAndFocusFirst();
          }
        }}
      >
        {label} <span className="qh-submenu-arrow" aria-hidden="true">▸</span>
      </button>
      {open && (
        <div
          className="qh-menu qh-submenu"
          role="menu"
          aria-labelledby={itemId}
          onKeyDown={(e) => {
            if (e.key === 'ArrowLeft') {
              e.preventDefault();
              e.stopPropagation();
              setOpen(false);
              itemRef.current?.focus();
            }
          }}
        >
          {children}
        </div>
      )}
    </div>
  );
}
