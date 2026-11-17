/**
 * @vitest-environment jsdom
 */

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import type { PresenceState } from '../../services/presenceService';
import StatusTab from './StatusTab';

const longName = 'A'.repeat(100);
const remoteUser: PresenceState = {
  peerId: 'remote-1',
  userId: 'user-1',
  userName: longName,
  userColor: '#E91E63',
  filePath: 'test.qmd',
  cursor: null,
  selection: null,
  lastSeen: Date.now(),
};

describe('StatusTab', () => {
  afterEach(cleanup);

  it('truncates long collaborator names and exposes the full name in a tooltip', () => {
    render(
      <StatusTab
        wasmStatus="ready"
        wasmError={null}
        userCount={1}
        remoteUsers={[remoteUser]}
        isOnline
      />,
    );

    const name = screen.getByText(longName);
    expect(name.className).toContain('qh-truncate');
    // The list item stays a direct child of the list (valid list markup).
    expect(name.closest('li')!.parentElement!.tagName).toBe('UL');

    fireEvent.focus(name);
    expect(screen.getByRole('tooltip').textContent).toBe(longName);
  });
});
