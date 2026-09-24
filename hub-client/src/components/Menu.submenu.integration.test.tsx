/**
 * @vitest-environment jsdom
 *
 * MenuSubmenu viewport flip (bd-q33ylfxf follow-up): a submenu opens to the
 * right of its parent item by default. When that would run past the right
 * edge of the window (the ＋ New menu is pinned to the header's right edge,
 * so it always would), it opens to the left instead. jsdom has no layout,
 * so the geometry is mocked.
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react';
import { Menu, MenuItem, MenuSubmenu, SUBMENU_CLOSE_GRACE_MS } from './Menu';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

function mockGeometry(opts: { innerWidth: number; parentLeft: number; submenuWidth: number }) {
  Object.defineProperty(window, 'innerWidth', { value: opts.innerWidth, configurable: true });
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const rect = (left: number, width: number) =>
      ({ left, right: left + width, width, top: 0, bottom: 40, height: 40, x: left, y: 0, toJSON() {} }) as DOMRect;
    if (this.classList.contains('qh-submenu')) {
      // Default placement: just right of the parent item.
      return rect(opts.parentLeft + 200 + 4, opts.submenuWidth);
    }
    if (this.classList.contains('qh-submenu-parent')) return rect(opts.parentLeft, 200);
    return rect(0, 0);
  });
}

function renderMenu() {
  render(
    <Menu onClose={() => {}} aria-label="Root">
      <MenuSubmenu label="Templates">
        <MenuItem onSelect={() => {}}>Default</MenuItem>
      </MenuSubmenu>
    </Menu>,
  );
}

function renderTwoGroups() {
  render(
    <Menu onClose={() => {}} aria-label="Root">
      <MenuSubmenu label="Templates" subtext="Bare skeletons">
        <MenuItem onSelect={() => {}}>Default</MenuItem>
      </MenuSubmenu>
      <MenuSubmenu label="Examples" subtext="Filled-in projects">
        <MenuItem onSelect={() => {}}>Article</MenuItem>
        <MenuSubmenu label="More">
          <MenuItem onSelect={() => {}}>Deck</MenuItem>
        </MenuSubmenu>
      </MenuSubmenu>
    </Menu>,
  );
}

describe('MenuSubmenu siblings', () => {
  it('closes an open sibling when the pointer moves onto another group, even with focus inside it', () => {
    mockGeometry({ innerWidth: 1400, parentLeft: 100, submenuWidth: 220 });
    renderTwoGroups();
    // Click opens Templates; APG then focuses its first leaf (on the
    // next frame in the component, synchronously here).
    fireEvent.click(screen.getByRole('menuitem', { name: /Templates/ }));
    expect(screen.getByRole('menu', { name: 'Templates' })).toBeTruthy();
    screen.getByRole('menuitem', { name: 'Default' }).focus();
    expect(document.activeElement?.textContent).toBe('Default');

    fireEvent.mouseEnter(screen.getByRole('menuitem', { name: /Examples/ }).parentElement!);
    expect(screen.queryByRole('menu', { name: 'Templates' })).toBeNull();
    expect(screen.getByRole('menu', { name: 'Examples' })).toBeTruthy();
  });

  it('keeps a parent group open while a nested group inside it opens', () => {
    mockGeometry({ innerWidth: 1400, parentLeft: 100, submenuWidth: 220 });
    renderTwoGroups();
    fireEvent.click(screen.getByRole('menuitem', { name: /Examples/ }));
    fireEvent.mouseEnter(screen.getByRole('menuitem', { name: /More/ }).parentElement!);
    expect(screen.getByRole('menu', { name: 'Examples' })).toBeTruthy();
    expect(screen.getByRole('menu', { name: 'More' })).toBeTruthy();
  });

  it('gives a hover-opened submenu a grace period before the pointer leaving closes it', () => {
    vi.useFakeTimers();
    try {
      mockGeometry({ innerWidth: 1400, parentLeft: 100, submenuWidth: 220 });
      renderTwoGroups();
      const parent = screen.getByRole('menuitem', { name: /Templates/ }).parentElement!;
      // The menu focuses its first item (the Templates button) on open;
      // a focused subtree is deliberately never collapsed by the pointer,
      // so move focus away to exercise the pure-hover path.
      act(() => (document.activeElement as HTMLElement | null)?.blur());
      fireEvent.mouseEnter(parent);
      expect(screen.getByRole('menu', { name: 'Templates' })).toBeTruthy();

      // Leaving and coming straight back (crossing the gap) keeps it open.
      fireEvent.mouseLeave(parent);
      vi.advanceTimersByTime(SUBMENU_CLOSE_GRACE_MS / 2);
      fireEvent.mouseEnter(parent);
      vi.advanceTimersByTime(SUBMENU_CLOSE_GRACE_MS * 2);
      expect(screen.getByRole('menu', { name: 'Templates' })).toBeTruthy();

      // Leaving for good closes it once the grace period elapses.
      fireEvent.mouseLeave(parent);
      expect(screen.getByRole('menu', { name: 'Templates' })).toBeTruthy();
      act(() => {
        vi.advanceTimersByTime(SUBMENU_CLOSE_GRACE_MS + 1);
      });
      expect(screen.queryByRole('menu', { name: 'Templates' })).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it('renders the group subtext under the group label', () => {
    mockGeometry({ innerWidth: 1400, parentLeft: 100, submenuWidth: 220 });
    renderTwoGroups();
    const templates = screen.getByRole('menuitem', { name: /Templates/ });
    expect(templates.querySelector('.qh-menu-subtext')?.textContent).toBe('Bare skeletons');
  });
});

describe('MenuSubmenu placement', () => {
  it('opens to the right when there is room', () => {
    mockGeometry({ innerWidth: 1400, parentLeft: 100, submenuWidth: 220 });
    renderMenu();
    fireEvent.click(screen.getByRole('menuitem', { name: /Templates/ }));
    const submenu = screen.getByRole('menu', { name: 'Templates' });
    expect(submenu.classList.contains('qh-submenu-left')).toBe(false);
  });

  it('flips to the left when the default placement would leave the viewport', () => {
    mockGeometry({ innerWidth: 1000, parentLeft: 700, submenuWidth: 220 });
    renderMenu();
    fireEvent.click(screen.getByRole('menuitem', { name: /Templates/ }));
    const submenu = screen.getByRole('menu', { name: 'Templates' });
    expect(submenu.classList.contains('qh-submenu-left')).toBe(true);
  });

  it('stays to the right when neither side fits, rather than clipping at the left edge', () => {
    mockGeometry({ innerWidth: 500, parentLeft: 100, submenuWidth: 400 });
    renderMenu();
    fireEvent.click(screen.getByRole('menuitem', { name: /Templates/ }));
    const submenu = screen.getByRole('menu', { name: 'Templates' });
    expect(submenu.classList.contains('qh-submenu-left')).toBe(false);
  });
});
