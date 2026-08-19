/**
 * Tests for SettingsTab.
 *
 * Settings holds preferences only. Actions that produce artifacts (Export
 * ZIP, Screenshot Preview) live in the Project tab — the screenshot button
 * used to live here and its absence is pinned by these tests.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import SettingsTab from './SettingsTab';

describe('SettingsTab', () => {
  afterEach(cleanup);

  it('renders the preference toggles', () => {
    render(<SettingsTab scrollSyncEnabled={true} onScrollSyncChange={() => {}} />);

    expect(screen.getByText('Scroll sync')).toBeTruthy();
    expect(screen.getByText('Collapse error overlay')).toBeTruthy();
    expect(screen.getByText('Nesting cursor')).toBeTruthy();
    expect(screen.getByText('Rich-text editor')).toBeTruthy();
  });

  it('contains no action buttons — settings are preferences only', () => {
    render(<SettingsTab scrollSyncEnabled={true} onScrollSyncChange={() => {}} />);

    expect(screen.queryByRole('button')).toBeNull();
    expect(screen.queryByText(/Screenshot/)).toBeNull();
  });

  it('has no section headings — the toggles read as one flat list', () => {
    render(<SettingsTab scrollSyncEnabled={true} onScrollSyncChange={() => {}} />);

    expect(screen.queryByText('Editor')).toBeNull();
    expect(screen.queryByText('Preview')).toBeNull();
    expect(document.querySelector('.section-label')).toBeNull();
  });

  it('calls onScrollSyncChange when the scroll sync toggle changes', () => {
    const onScrollSyncChange = vi.fn();
    render(<SettingsTab scrollSyncEnabled={true} onScrollSyncChange={onScrollSyncChange} />);

    fireEvent.click(screen.getByRole('checkbox', { name: /Scroll sync/ }));

    expect(onScrollSyncChange).toHaveBeenCalledWith(false);
  });
});
