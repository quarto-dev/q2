import { useCallback, useEffect, useRef, useState } from 'react';
import { useMediaQuery } from './useMediaQuery';

/**
 * Sidebar drawer behavior (Phase 5 narrow-viewport design): at ≤900px the
 * sidebar leaves the flex layout and becomes a modal overlay drawer (the
 * CSS lives in Editor.css — `.sidebar-drawer` is `display: contents`
 * above the breakpoint, so the wrapper is layout-transparent there).
 *
 * The hook owns the open state, Escape close, focus-into-drawer on open,
 * focus return to the toggle on close, and the Tab trap while open.
 * `SidebarDrawer` (components/SidebarDrawer.tsx) renders the wrapper +
 * scrim from these values; DocumentTopBar renders the toggle button.
 */
export function useSidebarDrawer() {
  const isDrawer = useMediaQuery('(max-width: 900px)');
  const [open, setOpen] = useState(false);
  const toggleRef = useRef<HTMLButtonElement | null>(null);
  const drawerRef = useRef<HTMLDivElement | null>(null);

  // drawerOpen derives closed-ness above the breakpoint, so no reset
  // effect is needed; `open` persisting across a widen/narrow round-trip
  // means the drawer reopens as the user left it (VS Code's sidebar
  // behaves the same way).
  const drawerOpen = isDrawer && open;

  // Focus into the drawer on open. On close, return focus to the toggle
  // only when focus is still inside the drawer — don't steal it back
  // when the close was caused by opening a dialog from a sidebar menu.
  useEffect(() => {
    if (!drawerOpen) return;
    const drawer = drawerRef.current;
    const toggle = toggleRef.current; // captured for the cleanup
    if (!drawer) return;
    drawer.querySelector<HTMLElement>('button')?.focus();
    return () => {
      if (drawer.contains(document.activeElement)) {
        toggle?.focus();
      }
    };
  }, [drawerOpen]);

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (!drawerOpen) return;
      if (e.key === 'Escape') {
        e.stopPropagation();
        setOpen(false);
        return;
      }
      if (e.key !== 'Tab') return;
      const drawer = drawerRef.current;
      if (!drawer) return;
      // Modal trap: wrap Tab/Shift+Tab at the drawer's ends. Roving
      // tabindex="-1" rows are arrow-key territory, correctly excluded.
      const focusables = drawer.querySelectorAll<HTMLElement>(
        'button:not(:disabled), [href], input:not(:disabled), [tabindex]:not([tabindex="-1"])',
      );
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    },
    [drawerOpen],
  );

  return {
    isDrawer,
    drawerOpen,
    toggle: useCallback(() => setOpen((v) => !v), []),
    close: useCallback(() => setOpen(false), []),
    toggleRef,
    drawerRef,
    drawerKeyDown: onKeyDown,
  };
}
