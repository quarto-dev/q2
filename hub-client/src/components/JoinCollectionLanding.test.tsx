/**
 * Tests for JoinCollectionLanding error presentation (bd-tux4m6od).
 *
 * The join flow used to surface automerge-repo's raw
 * `Document <id> is unavailable` rejection verbatim. These tests pin the
 * new behavior: classified CollectionConnectError messages are shown
 * as-is (the service owns the copy), an auth-expired failure offers a
 * "Sign in again" action, and unclassified errors keep the previous
 * fallback rendering.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react';
import JoinCollectionLanding from './JoinCollectionLanding';
import { CollectionConnectError } from '../services/collectionConnectError';
import type { JoinCollectionRoute } from '../utils/routing';

vi.mock('../services/userSettings', () => ({
  getUserIdentity: vi.fn().mockResolvedValue({
    key: 'identity',
    userId: 'u-1',
    userName: 'Test User',
    userColor: '#E91E63',
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
  }),
  updateUserName: vi.fn().mockResolvedValue(undefined),
  updateUserColor: vi.fn().mockResolvedValue(undefined),
}));

const DOC_ID = '2Agx7kENjysHSujsVgirvykVKECf';

const route: JoinCollectionRoute = {
  type: 'join-collection',
  collectionId: DOC_ID,
  collectionName: 'Personal',
  inviter: 'Carlos Scheidegger',
  syncServer: 'wss://quarto-hub.com/ws',
};

/** Render, wait for the identity to load, and click the join button. */
async function renderAndJoin(
  onSubscribe: (docId: string, syncServer: string) => Promise<void>,
  extraProps: Partial<Parameters<typeof JoinCollectionLanding>[0]> = {},
) {
  const onDone = vi.fn();
  render(
    <JoinCollectionLanding
      route={route}
      status="connected"
      onSubscribe={onSubscribe}
      onDone={onDone}
      {...extraProps}
    />,
  );
  const button = await screen.findByRole('button', { name: 'Join Personal' });
  // The name field fills asynchronously from the identity; the button is
  // disabled until it does.
  await waitFor(() => expect(button.hasAttribute('disabled')).toBe(false));
  fireEvent.click(button);
  return { onDone };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(cleanup);

describe('JoinCollectionLanding error presentation', () => {
  it('shows the classified message for a not-found failure', async () => {
    await renderAndJoin(() =>
      Promise.reject(new CollectionConnectError('not-found', DOC_ID)),
    );
    const alert = await screen.findByText(
      /This collection isn't available on the sync server/,
    );
    expect(alert.textContent).toContain(DOC_ID);
    expect(alert.textContent).toContain('ask them to open Quarto Hub and share it again');
  });

  it('shows the classified message for an offline failure', async () => {
    await renderAndJoin(() =>
      Promise.reject(new CollectionConnectError('offline', DOC_ID)),
    );
    const alert = await screen.findByText(/you appear to be offline/);
    expect(alert.textContent).toContain('Check your connection and retry.');
  });

  it('auth-expired: shows the session message and a "Sign in again" action', async () => {
    const onSignInAgain = vi.fn();
    await renderAndJoin(
      () => Promise.reject(new CollectionConnectError('auth-expired', DOC_ID)),
      { onSignInAgain },
    );
    await screen.findByText(/Your session has expired/);
    const signIn = screen.getByRole('button', { name: 'Sign in again' });
    fireEvent.click(signIn);
    expect(onSignInAgain).toHaveBeenCalledTimes(1);
  });

  it('non-auth failures do not offer the sign-in action', async () => {
    await renderAndJoin(() =>
      Promise.reject(new CollectionConnectError('not-found', DOC_ID)),
    );
    await screen.findByText(/This collection isn't available/);
    expect(screen.queryByRole('button', { name: 'Sign in again' })).toBeNull();
  });

  it('falls back to err.message for unclassified errors', async () => {
    await renderAndJoin(() => Promise.reject(new Error('some unexpected failure')));
    await screen.findByText('some unexpected failure');
  });

  it('falls back to a generic message for non-Error rejections', async () => {
    await renderAndJoin(() => Promise.reject('nope'));
    await screen.findByText('Could not join the collection.');
  });

  it('a successful join calls onDone and shows no error', async () => {
    const { onDone } = await renderAndJoin(() => Promise.resolve());
    await waitFor(() => expect(onDone).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(/unavailable|expired|offline/)).toBeNull();
  });

  it('the join button re-enables after a failure so the user can retry', async () => {
    await renderAndJoin(() =>
      Promise.reject(new CollectionConnectError('offline', DOC_ID)),
    );
    await screen.findByText(/you appear to be offline/);
    const button = screen.getByRole('button', { name: 'Join Personal' });
    expect(button.hasAttribute('disabled')).toBe(false);
  });
});
