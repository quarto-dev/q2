/**
 * Tests for the AboutTab keyboard-shortcuts reference: it must render
 * every group and entry from the shortcut map (utils/keyboardShortcuts)
 * so the reference can never silently drift from the registry.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import AboutTab from './AboutTab';
import { SHORTCUT_GROUPS } from '../../utils/keyboardShortcuts';

describe('AboutTab keyboard shortcuts reference', () => {
  afterEach(cleanup);

  it('renders every group and entry from the shortcut map', () => {
    render(<AboutTab wasmStatus="loading" />);

    expect(screen.getByText('Keyboard Shortcuts')).toBeTruthy();
    for (const group of SHORTCUT_GROUPS) {
      expect(screen.getByText(group.title)).toBeTruthy();
      for (const entry of group.entries) {
        // Key combos repeat across groups ("↑ / ↓") — assert presence.
        expect(screen.getAllByText(entry.keys).length).toBeGreaterThan(0);
        expect(screen.getAllByText(entry.action).length).toBeGreaterThan(0);
      }
    }
  });
});
