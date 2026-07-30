/**
 * @vitest-environment jsdom
 *
 * Tests for the /debug.html iframe embed (bd-09aja9gl; plan:
 * claude-notes/plans/2026-07-29-hub-client-in-context-debugging.md).
 *
 * The embed deliberately hosts the SERVER-view debugger (its own
 * ephemeral Repo) next to the live editor, for live-vs-server
 * comparison; these tests cover the overlay lifecycle and the hash
 * seed, not debug.html itself.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';

import {
  openServerInspector,
  closeServerInspector,
  isServerInspectorOpen,
} from './debugServerInspector';

const CONTAINER_SELECTOR = '#quarto-debug-server-inspector-container';

describe('debugServerInspector', () => {
  beforeEach(() => {
    closeServerInspector();
  });

  afterEach(() => {
    closeServerInspector();
  });

  it('throws when no index doc id is available', () => {
    expect(() => openServerInspector(null)).toThrow(/no project connected/);
    expect(isServerInspectorOpen()).toBe(false);
  });

  it('mounts an iframe seeded with the index doc url', () => {
    openServerInspector('abc123DocId');
    expect(isServerInspectorOpen()).toBe(true);

    const iframe = document.querySelector(
      `${CONTAINER_SELECTOR} iframe`,
    ) as HTMLIFrameElement;
    expect(iframe).toBeTruthy();
    expect(iframe.src).toContain('debug.html#doc=automerge:abc123DocId');
  });

  it('does not double the automerge: prefix', () => {
    openServerInspector('automerge:abc123DocId');
    const iframe = document.querySelector(
      `${CONTAINER_SELECTOR} iframe`,
    ) as HTMLIFrameElement;
    expect(iframe.src).toContain('#doc=automerge:abc123DocId');
    expect(iframe.src).not.toContain('automerge:automerge:');
  });

  it('double-open is a no-op; close removes the overlay and is idempotent', () => {
    openServerInspector('abc123DocId');
    openServerInspector('abc123DocId');
    expect(document.querySelectorAll(CONTAINER_SELECTOR)).toHaveLength(1);

    closeServerInspector();
    expect(isServerInspectorOpen()).toBe(false);
    expect(document.querySelector(CONTAINER_SELECTOR)).toBeNull();
    expect(() => closeServerInspector()).not.toThrow();
  });

  it('the overlay close button removes it', () => {
    openServerInspector('abc123DocId');
    const btn = document.querySelector(
      `${CONTAINER_SELECTOR} button`,
    ) as HTMLButtonElement;
    expect(btn).toBeTruthy();
    btn.click();
    expect(isServerInspectorOpen()).toBe(false);
  });
});
