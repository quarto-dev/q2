/**
 * Tests for the DocumentTopBar connection indicator.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import DocumentTopBar from './DocumentTopBar';
import { ViewModeProvider } from './ViewModeContext';

afterEach(cleanup);

// DocumentTopBar renders <ViewToggleControl/>, which reads ViewModeContext.
function renderHeader(isOnline: boolean) {
  return render(
    <ViewModeProvider>
      <DocumentTopBar currentFilePath="index.qmd" isOnline={isOnline} />
    </ViewModeProvider>,
  );
}

describe('DocumentTopBar connection indicator', () => {
  it('shows Online when connected', () => {
    renderHeader(true);
    const indicator = document.querySelector('.connection-indicator')!;
    expect(indicator.className).toContain('online');
    expect(indicator.className).not.toContain('offline');
    expect(screen.getByText('Online')).toBeDefined();
  });

  it('shows Offline when disconnected', () => {
    renderHeader(false);
    const indicator = document.querySelector('.connection-indicator')!;
    expect(indicator.className).toContain('offline');
    expect(screen.getByText('Offline')).toBeDefined();
  });
});
