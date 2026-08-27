import { header } from '../strings';
import type { useSidebarDrawer } from '../hooks/useSidebarDrawer';

/**
 * Responsive sidebar wrapper (Phase 5 narrow-viewport design, extended
 * after design review). Above 900px the wrapper is `display: contents` —
 * layout-transparent — unless the user hid the sidebar via the header
 * toggle, when it is `display: none`. At ≤900px it becomes a modal
 * overlay drawer: off-canvas until opened, with a scrim, dialog
 * semantics, and (via useSidebarDrawer) Escape close + focus management.
 *
 * The wrapper stays mounted when the drawer is closed — `inert` keeps
 * its controls out of the tab order and AT tree while off-canvas or
 * hidden.
 */
export default function SidebarDrawer({
  drawer,
  children,
}: {
  drawer: ReturnType<typeof useSidebarDrawer>;
  children: React.ReactNode;
}) {
  const { isDrawer, drawerOpen, visible, close, drawerRef, drawerKeyDown } = drawer;
  const cls = isDrawer
    ? `sidebar-drawer${drawerOpen ? ' open' : ''}`
    : `sidebar-drawer${visible ? '' : ' hidden'}`;
  return (
    <>
      <div
        ref={drawerRef}
        id="sidebar-drawer"
        className={cls}
        role={isDrawer ? 'dialog' : undefined}
        aria-modal={isDrawer || undefined}
        aria-label={isDrawer ? header.sidebarDrawerLabel : undefined}
        inert={isDrawer ? !drawerOpen : !visible}
        onKeyDown={drawerKeyDown}
      >
        {children}
      </div>
      {drawerOpen && <div className="drawer-scrim" onClick={close} aria-hidden="true" />}
    </>
  );
}
