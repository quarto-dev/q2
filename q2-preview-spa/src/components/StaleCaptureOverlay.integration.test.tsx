/**
 * Unit tests for StaleCaptureOverlay (Phase C.5, bd-kw93.5).
 *
 * Renders the component in isolation, asserting the UX seam:
 *   - The button label reflects the sidecar `state` field.
 *   - Clicking POSTs to /api/preview/re-execute with the active path.
 *   - 409 surfaces a "already in flight" message; 4xx/5xx show the
 *     server body.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';

import { StaleCaptureOverlay } from './StaleCaptureOverlay';

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 202 })));
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('StaleCaptureOverlay', () => {
  it('renders a Re-execute button by default', () => {
    render(<StaleCaptureOverlay activePath="doc.qmd" />);
    const button = screen.getByRole('button', { name: /re-execute/i });
    expect(button).not.toBeNull();
    expect((button as HTMLButtonElement).disabled).toBe(false);
  });

  it('disables the button and shows "Executing…" when state is running', () => {
    render(<StaleCaptureOverlay activePath="doc.qmd" state="running" />);
    const button = screen.getByRole('button', { name: /re-execute/i });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByText(/Executing…/i)).not.toBeNull();
  });

  it('POSTs to /api/preview/re-execute with the active path on click', async () => {
    const fetchMock = vi.fn(async () => new Response('', { status: 202 }));
    vi.stubGlobal('fetch', fetchMock);

    render(<StaleCaptureOverlay activePath="posts/post1.qmd" />);
    fireEvent.click(screen.getByRole('button', { name: /re-execute/i }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(1);
    });

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe('/api/preview/re-execute');
    expect(init.method).toBe('POST');
    const body = JSON.parse(init.body as string);
    expect(body).toEqual({ path: 'posts/post1.qmd' });
  });

  it('surfaces a 409 response as an "already in flight" message', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('busy', { status: 409 })),
    );

    render(<StaleCaptureOverlay activePath="doc.qmd" />);
    fireEvent.click(screen.getByRole('button', { name: /re-execute/i }));

    await waitFor(() => {
      expect(screen.queryByText(/already in flight/i)).not.toBeNull();
    });
  });

  it('surfaces a non-202 / non-409 body inline', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('bad path', { status: 400 })),
    );

    render(<StaleCaptureOverlay activePath="doc.qmd" />);
    fireEvent.click(screen.getByRole('button', { name: /re-execute/i }));

    await waitFor(() => {
      expect(screen.queryByText(/Re-execute failed \(400\): bad path/i)).not.toBeNull();
    });
  });

  it('shows the recorded lastError when no POST has been made yet', () => {
    render(
      <StaleCaptureOverlay
        activePath="doc.qmd"
        state="error"
        lastError="engine timed out"
      />,
    );
    expect(screen.queryByText(/engine timed out/i)).not.toBeNull();
  });
});
