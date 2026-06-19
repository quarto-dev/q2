/**
 * G22: commit-status indicator unit tests.
 *
 * (a) classifyCommitOutcome — pure unit, no jsdom needed but we add the
 *     pragma anyway so the file compiles in vitest's jsdom project.
 * (b) CommitStatusBulb render — jsdom render via @testing-library/react.
 *
 * Fail-on-revert bindings:
 *   (a) Temporarily returning 'change' always → spurious case goes RED.
 *   (b) Temporarily adding 'error' to the lit set → error case goes RED.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';
import { classifyCommitOutcome, CommitStatusBulb } from './ReactPreview';

afterEach(() => {
  cleanup();
});

// ---------------------------------------------------------------------------
// (a) classifyCommitOutcome — PRIMARY binding
// ---------------------------------------------------------------------------
describe('classifyCommitOutcome', () => {
  it('returns spurious when newQmd equals renderedContent', () => {
    expect(classifyCommitOutcome('x', 'x')).toBe('spurious');
  });

  it('returns change when newQmd differs from renderedContent', () => {
    expect(classifyCommitOutcome('a', 'b')).toBe('change');
  });
});

// ---------------------------------------------------------------------------
// (b) CommitStatusBulb — SECONDARY binding
// ---------------------------------------------------------------------------
describe('CommitStatusBulb', () => {
  it('spurious: bulb is ON (--bulb-opacity 1) and pulses (data-pulse="1")', () => {
    const { container } = render(<CommitStatusBulb status="spurious" />);
    const bulb = container.querySelector('.q2-commit-bulb') as HTMLElement;
    expect(bulb).not.toBeNull();
    expect(bulb.getAttribute('data-pulse')).toBe('1');
    // CSS custom properties are part of the inline style attribute string.
    const style = bulb.getAttribute('style') ?? '';
    expect(style).toContain('--bulb-opacity: 1');
  });

  it('error: bulb is OFF (--bulb-opacity 0) — error pill owns the corner', () => {
    const { container } = render(<CommitStatusBulb status="error" />);
    const bulb = container.querySelector('.q2-commit-bulb') as HTMLElement;
    expect(bulb).not.toBeNull();
    const style = bulb.getAttribute('style') ?? '';
    expect(style).toContain('--bulb-opacity: 0');
  });

  it('idle: bulb is OFF (--bulb-opacity 0)', () => {
    const { container } = render(<CommitStatusBulb status="idle" />);
    const bulb = container.querySelector('.q2-commit-bulb') as HTMLElement;
    expect(bulb).not.toBeNull();
    const style = bulb.getAttribute('style') ?? '';
    expect(style).toContain('--bulb-opacity: 0');
  });
});
