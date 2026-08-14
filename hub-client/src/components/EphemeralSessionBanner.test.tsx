/**
 * Tests for the ephemeral-session banner shown in the editor when the
 * serving `q2 preview` session runs without --allow-edit.
 *
 * The data path behind it (fetch + validation of /api/preview/config)
 * is covered by previewConfig.test.ts and usePreviewSession.test.tsx;
 * these tests pin the banner's rendered contract: the copy a user
 * sees, the status role, and the hover text naming the --allow-edit
 * fix.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import EphemeralSessionBanner from './EphemeralSessionBanner';

afterEach(cleanup);

describe('EphemeralSessionBanner', () => {
  it('renders the ephemeral-session copy as a status region', () => {
    render(<EphemeralSessionBanner />);

    const banner = screen.getByRole('status');
    expect(banner.className).toBe('ephemeral-session-banner');
    expect(banner.textContent).toContain("edits won't be saved to disk");
  });

  it('hover text explains the cause and names the --allow-edit fix', () => {
    render(<EphemeralSessionBanner />);

    const { title } = screen.getByRole('status');
    expect(title).toContain('never written');
    expect(title).toContain('--allow-edit');
  });
});
