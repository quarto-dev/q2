/**
 * Tests for the in-preview error overlay.
 *
 * Bug 2 (bd-mwtf) cares about: when the active page failed
 * Pass-1 in the WASM render, hub-client now passes structured
 * diagnostics through to the overlay. The previous behavior
 * (only the generic 'Project render produced no output for the
 * active page' string) is gone — the overlay now renders the
 * line/column/title from the structured diagnostic.
 *
 * `usePreference` is mocked because the preference plumbing
 * isn't under test here, and vitest 4's jsdom + the project's
 * vitest config produces a non-functional `localStorage` (see
 * the `--localstorage-file` warning) that the real
 * `usePreference` would otherwise touch.
 */

import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';

let collapsedState = true;
const setCollapsedMock = vi.fn((v: boolean) => {
  collapsedState = v;
});

vi.mock('../../hooks/usePreference', () => ({
  usePreference: (_key: string) => [collapsedState, setCollapsedMock],
}));

import { PreviewErrorOverlay } from './PreviewErrorOverlay';

const parseDiagnostic: Diagnostic = {
  kind: 'error',
  title: '[Q-2-10] Closed Quote Without Matching Open Quote',
  problem:
    'A space is causing a quote mark to be interpreted as a quotation close.',
  hints: [],
  start_line: 11,
  start_column: 36,
  end_line: 11,
  end_column: 37,
  details: [],
};

describe('PreviewErrorOverlay (bd-mwtf parse-error display)', () => {
  it('renders nothing when not visible', () => {
    collapsedState = false;
    const { container } = render(
      <PreviewErrorOverlay
        error={{ message: 'boom', diagnostics: [parseDiagnostic] }}
        visible={false}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it('renders nothing when there is no error', () => {
    collapsedState = false;
    const { container } = render(
      <PreviewErrorOverlay error={null} visible={true} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it('renders the parse diagnostic with line + title + problem when expanded', () => {
    collapsedState = false;
    render(
      <PreviewErrorOverlay
        error={{
          message:
            'Pass 1 failed for /project/about.qmd: ' +
            '[Q-2-10] Closed Quote Without Matching Open Quote',
          diagnostics: [parseDiagnostic],
        }}
        visible={true}
      />,
    );

    expect(screen.getByText(/Render Error/)).toBeTruthy();
    expect(screen.getByText(/Line 11:/)).toBeTruthy();
    // Diagnostic title appears in the diagnostic list — pick the
    // one inside `.diagnostic-title` to disambiguate from the
    // `<pre>` message which also contains 'Q-2-10'.
    const titleEl = document.querySelector('.diagnostic-title');
    expect(titleEl?.textContent).toMatch(/Q-2-10/);
    expect(screen.getByText(/A space is causing a quote mark/, { exact: false }))
      .toBeTruthy();
  });

  it('falls back to message-only when no diagnostics are attached', () => {
    collapsedState = false;
    render(
      <PreviewErrorOverlay
        error={{ message: 'something exploded' }}
        visible={true}
      />,
    );

    expect(screen.getByText(/something exploded/)).toBeTruthy();
    expect(document.querySelector('.preview-error-diagnostics')).toBeNull();
  });

  it('renders sibling pass-1 failures with source-file attribution (bd-rqba)', () => {
    collapsedState = false;
    render(
      <PreviewErrorOverlay
        error={{
          message: "Sibling page 'about.qmd' failed to parse",
          pass1Failures: [
            {
              source_file: 'about.qmd',
              error: '[Q-2-10] Closed Quote Without Matching Open Quote',
              diagnostics: [parseDiagnostic],
            },
          ],
        }}
        visible={true}
      />,
    );

    // Source-file attribution ribbon ("about.qmd failed to parse").
    const sourceEls = document.querySelectorAll('.diagnostic-source-file');
    expect(sourceEls.length).toBe(1);
    expect(sourceEls[0].textContent).toMatch(/about\.qmd/);
    expect(sourceEls[0].textContent).toMatch(/failed to parse/);

    // Inner diagnostic gets line + title rendered same as the
    // active-page case.
    expect(screen.getByText(/Line 11:/)).toBeTruthy();
    const titleEls = document.querySelectorAll('.diagnostic-title');
    // One in the per-failure list (no top-level diagnostics here).
    expect(Array.from(titleEls).some((el) => el.textContent?.includes('Q-2-10'))).toBe(true);
  });

  it('renders multiple pass-1 failures, each attributed to its source', () => {
    collapsedState = false;
    render(
      <PreviewErrorOverlay
        error={{
          message: '2 sibling pages failed to parse',
          pass1Failures: [
            {
              source_file: 'about.qmd',
              error: 'parse error 1',
              diagnostics: [parseDiagnostic],
            },
            {
              source_file: 'posts/first.qmd',
              error: 'parse error 2',
              diagnostics: [],
            },
          ],
        }}
        visible={true}
      />,
    );

    const sourceEls = document.querySelectorAll('.diagnostic-source-file');
    expect(sourceEls.length).toBe(2);
    expect(sourceEls[0].textContent).toMatch(/about\.qmd/);
    expect(sourceEls[1].textContent).toMatch(/posts\/first\.qmd/);

    // The second failure has no structured diagnostics → falls
    // back to the raw error text in a <pre>.
    const failureBlocks = document.querySelectorAll('.preview-error-pass1-failure');
    expect(failureBlocks[1].querySelector('pre')?.textContent).toMatch(/parse error 2/);
  });

  it('shows collapsed indicator (no diagnostic list) when collapsed', () => {
    collapsedState = true;
    render(
      <PreviewErrorOverlay
        error={{
          message: 'Pass 1 failed',
          diagnostics: [parseDiagnostic],
        }}
        visible={true}
      />,
    );

    // Collapsed: just an Error button, no diagnostic list.
    expect(screen.getByText(/Error/)).toBeTruthy();
    expect(document.querySelector('.preview-error-diagnostics')).toBeNull();
  });
});
