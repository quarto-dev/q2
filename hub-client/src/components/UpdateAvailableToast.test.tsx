/**
 * Tests for the update-available toast (GH #447, bd-axqunnx9): the
 * visible-tab half of the SW update flow. Pins the rendered contract —
 * the copy, the Reload button that reloads the page, and dismissal,
 * which only hides the toast (the hide-reload listener in pwa.ts still
 * reloads the tab when it is next backgrounded).
 *
 * Each test drives a fresh pwaPrompt store via the `prompt` prop so no
 * module-level state leaks between tests.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, act } from '@testing-library/react';
import UpdateAvailableToast from './UpdateAvailableToast';
import { createPwaPromptStore } from '../pwaPrompt';

const reloadMock = vi.fn();

beforeEach(() => {
  reloadMock.mockClear();
  Object.defineProperty(window, 'location', {
    configurable: true,
    value: { ...window.location, reload: reloadMock },
  });
});

afterEach(cleanup);

describe('UpdateAvailableToast', () => {
  it('renders nothing before an update is pending', () => {
    render(<UpdateAvailableToast prompt={createPwaPromptStore()} />);
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('appears with the update copy and a Reload button once shown', () => {
    const prompt = createPwaPromptStore();
    render(<UpdateAvailableToast prompt={prompt} />);
    act(() => prompt.show());

    const toast = screen.getByRole('status');
    expect(toast.textContent).toContain('A new version is available.');
    expect(screen.getByRole('button', { name: 'Reload' })).toBeTruthy();
  });

  it('shows on mount when show() fired before the component subscribed', () => {
    const prompt = createPwaPromptStore();
    prompt.show();
    render(<UpdateAvailableToast prompt={prompt} />);
    expect(screen.getByRole('status')).toBeTruthy();
  });

  it('reloads the page when Reload is clicked', () => {
    const prompt = createPwaPromptStore();
    render(<UpdateAvailableToast prompt={prompt} />);
    act(() => prompt.show());
    fireEvent.click(screen.getByRole('button', { name: 'Reload' }));
    expect(reloadMock).toHaveBeenCalledOnce();
  });

  it('hides on dismiss without reloading', () => {
    const prompt = createPwaPromptStore();
    render(<UpdateAvailableToast prompt={prompt} />);
    act(() => prompt.show());
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByRole('status')).toBeNull();
    expect(reloadMock).not.toHaveBeenCalled();
  });
});
