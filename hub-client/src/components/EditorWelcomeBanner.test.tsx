/**
 * Tests for EditorWelcomeBanner (bd-fxdcxbpq) — the one-time welcome bar
 * shown under the editor toolbar after arriving via an invite.
 *
 * Pins: copy for the collection and document variants, the "Change name"
 * affordance, and per-target dismissal persisted in localStorage so the
 * banner shows exactly once per collection/project.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import EditorWelcomeBanner, { WELCOME_DISMISSED_KEY_PREFIX } from './EditorWelcomeBanner';

const TARGET_ID = '2Agx7kENjysHSujsVgirvykVKECf';

type Props = Parameters<typeof EditorWelcomeBanner>[0];

function renderBanner(overrides: Partial<Props> = {}) {
  const onRename = vi.fn().mockResolvedValue(undefined);
  const props: Props = {
    kind: 'collection',
    targetId: TARGET_ID,
    targetName: 'Team docs',
    inviter: 'Carlos',
    userName: 'Amy Mora',
    onRename,
    ...overrides,
  };
  const utils = render(<EditorWelcomeBanner {...props} />);
  return { onRename, ...utils };
}

beforeEach(() => {
  localStorage.clear();
});

afterEach(cleanup);

describe('EditorWelcomeBanner', () => {
  it('collection variant: welcome copy names the collection, inviter, and live identity', () => {
    renderBanner();
    const banner = screen.getByTestId('editor-welcome-banner');
    expect(banner.textContent).toContain('Welcome to');
    expect(banner.textContent).toContain('Team docs');
    // The invite lands on the home screen, not a start document, so the
    // banner credits the inviter without claiming they "suggested" this file.
    expect(banner.textContent).toContain('Carlos invited you.');
    expect(banner.textContent).not.toContain('suggested starting here');
    expect(banner.textContent).toContain("You're editing live as");
    expect(banner.textContent).toContain('Amy Mora');
  });

  it('document variant: shared-document copy', () => {
    renderBanner({ kind: 'document' });
    const banner = screen.getByTestId('editor-welcome-banner');
    expect(banner.textContent).toContain('Carlos');
    expect(banner.textContent).toContain('shared this document with you.');
    expect(banner.textContent).toContain("You're editing live as");
    expect(banner.textContent).toContain('Amy Mora');
  });

  it('"Change name" opens an inline rename prefilled with the current name; Save calls onRename', async () => {
    const { onRename } = renderBanner();
    fireEvent.click(screen.getByRole('button', { name: 'Change name' }));
    const input = screen.getByRole('textbox') as HTMLInputElement;
    expect(input.value).toBe('Amy Mora');
    fireEvent.change(input, { target: { value: 'Amy M.' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(onRename).toHaveBeenCalledWith('Amy M.');
  });

  it('inline rename ignores empty names', () => {
    const { onRename } = renderBanner();
    fireEvent.click(screen.getByRole('button', { name: 'Change name' }));
    fireEvent.change(screen.getByRole('textbox'), { target: { value: '   ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    expect(onRename).not.toHaveBeenCalled();
  });

  it('dismissing hides the banner and persists per target id', () => {
    renderBanner();
    fireEvent.click(screen.getByRole('button', { name: /dismiss/i }));
    expect(screen.queryByTestId('editor-welcome-banner')).toBeNull();
    expect(localStorage.getItem(`${WELCOME_DISMISSED_KEY_PREFIX}${TARGET_ID}`)).toBe('1');
  });

  it('renders nothing when the target was already dismissed', () => {
    localStorage.setItem(`${WELCOME_DISMISSED_KEY_PREFIX}${TARGET_ID}`, '1');
    renderBanner();
    expect(screen.queryByTestId('editor-welcome-banner')).toBeNull();
  });

  it('a dismissal for one target does not hide the banner for another', () => {
    localStorage.setItem(`${WELCOME_DISMISSED_KEY_PREFIX}other-doc`, '1');
    renderBanner();
    expect(screen.getByTestId('editor-welcome-banner')).toBeTruthy();
  });
});
