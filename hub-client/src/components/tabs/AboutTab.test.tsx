/**
 * Tests for the AboutTab keyboard-shortcuts reference: it must render
 * every group and entry from the shortcut map (utils/keyboardShortcuts)
 * so the reference can never silently drift from the registry.
 *
 * Also covers the changelog/more-info modal theming (GH #624): the iframe
 * document sees none of the app's theme classes, so AboutTab must inject
 * theme-matched styles into the rendered HTML — including when the app is
 * in dark mode, where hardcoded light colors were near-invisible.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest';
import type { ComponentProps } from 'react';
import { render, screen, cleanup, waitFor } from '@testing-library/react';
import AboutTab from './AboutTab';
import { ThemeProvider } from '../ThemeContext';
import { SHORTCUT_GROUPS } from '../../utils/keyboardShortcuts';

vi.mock('@quarto/preview-runtime', () => ({
  renderContentToHtml: vi.fn(async () => ({
    success: true,
    html: '<!DOCTYPE html>\n<html>\n<head>\n<meta charset="utf-8">\n</head>\n<body><p>entry</p></body>\n</html>',
  })),
  isWasmReady: () => true,
}));

// jsdom lacks matchMedia, which ThemeProvider reads for 'auto' mode.
// (The shared stub in test-utils/setup.ts is only wired into the
// integration config, not the unit config.)
vi.stubGlobal('matchMedia', (query: string) => ({
  matches: false,
  media: query,
  onchange: null,
  addListener: vi.fn(),
  removeListener: vi.fn(),
  addEventListener: vi.fn(),
  removeEventListener: vi.fn(),
  dispatchEvent: vi.fn(),
}));

function seedColorScheme(colorScheme: 'light' | 'dark') {
  localStorage.setItem(
    'quarto-hub:preferences',
    JSON.stringify({
      version: 1,
      scrollSyncEnabled: true,
      errorOverlayCollapsed: true,
      colorScheme,
      unlockNestingCursor: true,
      richText: true,
    }),
  );
}

function renderAboutTab(props: ComponentProps<typeof AboutTab>) {
  return render(
    <ThemeProvider>
      <AboutTab {...props} />
    </ThemeProvider>,
  );
}

describe('AboutTab keyboard shortcuts reference', () => {
  afterEach(cleanup);

  it('renders every group and entry from the shortcut map', () => {
    renderAboutTab({ wasmStatus: 'loading' });

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

describe('AboutTab changelog modal theming (GH #624)', () => {
  beforeEach(() => {
    localStorage.clear();
  });
  afterEach(cleanup);

  async function openChangelog() {
    const button = await screen.findByRole('button', { name: 'View Changelog' });
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
    button.click();
    const iframe = (await screen.findByTitle('Changelog')) as HTMLIFrameElement;
    return iframe.srcdoc || iframe.getAttribute('srcdoc') || '';
  }

  it('injects dark-theme styles into the iframe when the app is dark', async () => {
    seedColorScheme('dark');
    renderAboutTab({ wasmStatus: 'ready' });

    const srcdoc = await openChangelog();
    expect(srcdoc).toContain('color-scheme: dark');
    expect(srcdoc).not.toContain('color: #333');
  });

  it('injects light-theme styles into the iframe when the app is light', async () => {
    seedColorScheme('light');
    renderAboutTab({ wasmStatus: 'ready' });

    const srcdoc = await openChangelog();
    expect(srcdoc).toContain('color-scheme: light');
  });
});
