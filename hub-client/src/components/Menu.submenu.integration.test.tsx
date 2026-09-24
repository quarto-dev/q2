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
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import { Menu, MenuItem, MenuSubmenu } from './Menu';

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
