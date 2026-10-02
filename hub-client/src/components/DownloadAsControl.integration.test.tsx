/**
 * @vitest-environment jsdom
 *
 * "Download as" control (pandoc-host H5): menu semantics, the described disabled state, the
 * progress/cancel surface and the live region.
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, within } from '@testing-library/react';
import DownloadAsControl from './DownloadAsControl';
import { download } from '../strings';
import type { DownloadFormat, DownloadStatus } from '../pandoc/downloadController';

afterEach(cleanup);

const DOCX: DownloadFormat = { key: 'docx', label: 'Word', extension: 'docx', mime: 'application/docx' };
const idle: DownloadStatus = { phase: 'idle' };

function setup(props: Partial<React.ComponentProps<typeof DownloadAsControl>> = {}) {
  const handlers = { onSelect: vi.fn(), onCancel: vi.fn(), onDismiss: vi.fn() };
  const view = render(<DownloadAsControl formats={[DOCX]} status={idle} {...handlers} {...props} />);
  return { ...handlers, ...view };
}

describe('DownloadAsControl', () => {
  it('opens a menu of the formats and selects one', () => {
    const { onSelect } = setup();
    const button = screen.getByRole('button', { name: 'Download as' });
    expect(button.getAttribute('aria-haspopup')).toBe('menu');
    expect(button.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(button);
    expect(button.getAttribute('aria-expanded')).toBe('true');
    const menu = screen.getByRole('menu', { name: 'Download as' });
    fireEvent.click(within(menu).getByRole('menuitem', { name: 'Word' }));
    expect(onSelect).toHaveBeenCalledWith(DOCX);
  });

  it('Escape closes the menu and returns focus to the button', () => {
    setup();
    const button = screen.getByRole('button', { name: 'Download as' });
    fireEvent.click(button);
    const menu = screen.getByRole('menu');
    fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(button);
  });

  it('mentions the one-time download in the menu', () => {
    setup();
    fireEvent.click(screen.getByRole('button', { name: 'Download as' }));
    expect(screen.getByText(/about 16 MB/)).toBeTruthy();
  });

  it('a disabled control is aria-disabled, described by text, and does not open', () => {
    setup({ disabledReason: 'Download is unavailable: documents with format latex cannot be converted.' });
    const button = screen.getByRole('button', { name: 'Download as' });
    expect(button.getAttribute('aria-disabled')).toBe('true');
    expect(button.getAttribute('aria-haspopup')).toBeNull();
    const describedBy = button.getAttribute('aria-describedby')!;
    expect(document.getElementById(describedBy)?.textContent).toMatch(/latex/);
    fireEvent.click(button);
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('shows byte progress while the converter downloads, and Cancel calls onCancel', () => {
    const { onCancel } = setup({
      status: { phase: 'working', clickId: 1, format: DOCX, stage: 'loading', load: { phase: 'download', loaded: 5 * 1048576, total: 10 * 1048576 } },
    });
    const text = screen.getAllByText(/Downloading the converter… 5.0 MB of 10.0 MB/);
    expect(text.length).toBeGreaterThan(0);
    const bar = screen.getByRole('progressbar');
    expect(bar.getAttribute('value')).toBe(String(5 * 1048576));
    expect(bar.getAttribute('max')).toBe(String(10 * 1048576));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalled();
  });

  it('the live region announces phase changes without byte counts', () => {
    setup({
      status: { phase: 'working', clickId: 1, format: DOCX, stage: 'loading', load: { phase: 'download', loaded: 5 * 1048576, total: 10 * 1048576 } },
    });
    const live = screen.getByRole('status', { name: 'Download status' });
    expect(live.textContent).toBe('Downloading the converter…');
  });

  it('reports a finished download with its warnings and unexecuted cells', () => {
    const { onDismiss } = setup({
      status: {
        phase: 'done',
        clickId: 1,
        format: DOCX,
        fileName: 'report.docx',
        notices: [],
        unexecutedCells: 2,
        warnings: [{ origin: 'rust', kind: 'warning', code: 'Q-11-1', title: 'pandoc warning', problem: 'missing figure.png' }],
      },
    });
    expect(screen.getByRole('status', { name: 'Download status' }).textContent).toBe('Downloaded report.docx.');
    expect(screen.getByText('2 code cells were not executed; their output is not in the file.')).toBeTruthy();
    expect(screen.getByText('1 warning')).toBeTruthy();
    expect(screen.getByText(/missing figure.png/)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(onDismiss).toHaveBeenCalled();
  });

  it('a failure shows the plain-language state and the diagnostics, and announces it', () => {
    setup({
      status: {
        phase: 'failed',
        clickId: 1,
        format: DOCX,
        state: 'pandoc-error',
        notices: [],
        diagnostics: [{ origin: 'rust', kind: 'error', code: 'Q-20-3', title: 'pandoc failed', problem: 'exit status: 83' }],
      },
    });
    expect(screen.getByRole('status', { name: 'Download status' }).textContent).toMatch(/converter reported an error/);
    expect(screen.getByText(/\[Q-20-3\]/)).toBeTruthy();
    expect(screen.getByText(/exit status: 83/)).toBeTruthy();
  });

  it('every failure state has its own plain-language copy', () => {
    const states = Object.keys(download.failed).filter((s) => s !== 'done' && s !== 'cancelled');
    expect(states.sort()).toEqual(
      ['blocked', 'crashed', 'download-failed', 'invalid-request', 'native-error', 'native-failed', 'offline', 'out-of-memory', 'pandoc-error', 'request-failed', 'timeout', 'unsupported'].sort(),
    );
    for (const state of states) {
      const { unmount } = setup({ status: { phase: 'failed', clickId: 1, format: DOCX, state: state as never, notices: [], diagnostics: [] } });
      const text = screen.getByRole('status', { name: 'Download status' }).textContent;
      expect(text, state).toContain((download.failed as Record<string, string>)[state]);
      expect((download.failed as Record<string, string>)[state].length, state).toBeGreaterThan(10);
      unmount();
    }
  });

  it('a host failure shows the host diagnostic message', () => {
    setup({
      status: {
        phase: 'failed',
        clickId: 1,
        format: DOCX,
        state: 'timeout',
        notices: [],
        diagnostics: [{ origin: 'host', kind: 'error', code: 'pandoc-timeout', message: 'pandoc did not finish within 120 s' }],
      },
    });
    expect(screen.getByText(/did not finish within 120 s/)).toBeTruthy();
  });
});
