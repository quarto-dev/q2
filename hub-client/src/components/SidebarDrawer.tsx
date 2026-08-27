import { header } from '../strings';
import type { useSidebarDrawer } from '../hooks/useSidebarDrawer';

/**
 * Responsive sidebar wrapper (Phase 5 narrow-viewport design). Above
 * 900px the wrapper is `display: contents` — layout-transparent, the
 * sidebar sits in the flex row as always. At ≤900px it becomes a modal
 * overlay drawer: off-canvas until opened, with a scrim, dialog
 * semantics, and (via useSidebarDrawer) Escape close + focus management.
 *
 * The wrapper stays mounted when the drawer is closed — `inert` keeps
 * its controls out of the tab order and AT tree while off-canvas.
 */
export default function SidebarDrawer({
  drawer,
  children,
}: {
  drawer: ReturnType<typeof useSidebarDrawer>;
  children: React.ReactNode;
}) {
  const { isDrawer, drawerOpen, close, drawerRef, drawerKeyDown } = drawer;
  return (
    <>
      <div
        ref={drawerRef}
        id="sidebar-drawer"
        className={`sidebar-drawer${drawerOpen ? ' open' : ''}`}
        role={isDrawer ? 'dialog' : undefined}
        aria-modal={isDrawer || undefined}
        aria-label={isDrawer ? header.sidebarDrawerLabel : undefined}
        inert={isDrawer && !drawerOpen}
        onKeyDown={drawerKeyDown}
      >
        {children}
      </div>
      {drawerOpen && <div className="drawer-scrim" onClick={close} aria-hidden="true" />}
    </>
  );
}
