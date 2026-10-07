/**
 * @vitest-environment jsdom
 */
import { describe, expect, it, vi, afterEach } from 'vitest';
import { renderHook, waitFor, cleanup } from '@testing-library/react';
import { useWindowFileDrop } from './useWindowFileDrop';

afterEach(cleanup);

/** A DragEvent whose DataTransfer is a plain object (jsdom has none). `types` and `files` are what the hook reads. */
function dragEvent(type: 'dragover' | 'drop', opts: { types: string[]; files?: File[] }): DragEvent {
  const e = new Event(type, { bubbles: true, cancelable: true }) as DragEvent;
  Object.defineProperty(e, 'dataTransfer', {
    value: { types: opts.types, files: opts.files ?? [], items: [], dropEffect: 'none' },
  });
  return e;
}

const doc = () => new File(['x'], 'a.docx');

describe('useWindowFileDrop', () => {
  it('routes a file drop and prevents the browser default', async () => {
    const onDrop = vi.fn();
    renderHook(() => useWindowFileDrop({ enabled: true, onDrop }));
    const f = doc();
    const over = dragEvent('dragover', { types: ['Files'] });
    document.dispatchEvent(over);
    expect(over.defaultPrevented).toBe(true);
    expect((over.dataTransfer as { dropEffect: string }).dropEffect).toBe('copy');

    const drop = dragEvent('drop', { types: ['Files'], files: [f] });
    document.dispatchEvent(drop);
    expect(drop.defaultPrevented).toBe(true);
    await waitFor(() => expect(onDrop).toHaveBeenCalledTimes(1));
    expect(onDrop.mock.calls[0][0]).toEqual({ files: [{ file: f, relativePath: 'a.docx' }], folders: [] });
  });

  it('ignores an event a component already handled (defaultPrevented)', async () => {
    const onDrop = vi.fn();
    renderHook(() => useWindowFileDrop({ enabled: true, onDrop }));
    const handled = dragEvent('drop', { types: ['Files'], files: [doc()] });
    handled.preventDefault();
    document.dispatchEvent(handled);
    await new Promise((r) => setTimeout(r, 10));
    expect(onDrop).not.toHaveBeenCalled();
  });

  it('leaves a non-file drag alone', async () => {
    const onDrop = vi.fn();
    renderHook(() => useWindowFileDrop({ enabled: true, onDrop }));
    for (const type of ['dragover', 'drop'] as const) {
      const e = dragEvent(type, { types: ['text/plain', 'application/x-hub-file'] });
      document.dispatchEvent(e);
      expect(e.defaultPrevented).toBe(false);
    }
    await new Promise((r) => setTimeout(r, 10));
    expect(onDrop).not.toHaveBeenCalled();
  });

  it('registers nothing when disabled (import unavailable)', async () => {
    const onDrop = vi.fn();
    renderHook(() => useWindowFileDrop({ enabled: false, onDrop }));
    const drop = dragEvent('drop', { types: ['Files'], files: [doc()] });
    document.dispatchEvent(drop);
    expect(drop.defaultPrevented).toBe(false);
    await new Promise((r) => setTimeout(r, 10));
    expect(onDrop).not.toHaveBeenCalled();
  });

  it('removes its listeners on unmount and when disabled', () => {
    const onDrop = vi.fn();
    const { unmount, rerender } = renderHook(({ enabled }) => useWindowFileDrop({ enabled, onDrop }), { initialProps: { enabled: true } });
    rerender({ enabled: false });
    const a = dragEvent('dragover', { types: ['Files'] });
    document.dispatchEvent(a);
    expect(a.defaultPrevented).toBe(false);
    rerender({ enabled: true });
    unmount();
    const b = dragEvent('dragover', { types: ['Files'] });
    document.dispatchEvent(b);
    expect(b.defaultPrevented).toBe(false);
  });

  it('calls the latest handler without re-registering', async () => {
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = renderHook(({ onDrop }) => useWindowFileDrop({ enabled: true, onDrop }), { initialProps: { onDrop: first } });
    rerender({ onDrop: second });
    document.dispatchEvent(dragEvent('drop', { types: ['Files'], files: [doc()] }));
    await waitFor(() => expect(second).toHaveBeenCalledTimes(1));
    expect(first).not.toHaveBeenCalled();
  });

  it('a stopPropagation handler on an element keeps its drop from reaching the document', () => {
    // The sidebar, Monaco and Add-asset all stop propagation, which is why a window listener is safe.
    const onDrop = vi.fn();
    renderHook(() => useWindowFileDrop({ enabled: true, onDrop }));
    const el = document.createElement('div');
    el.addEventListener('drop', (e) => e.stopPropagation());
    document.body.appendChild(el);
    const drop = dragEvent('drop', { types: ['Files'], files: [doc()] });
    el.dispatchEvent(drop);
    expect(drop.defaultPrevented).toBe(false);
    expect(onDrop).not.toHaveBeenCalled();
    el.remove();
  });
});
