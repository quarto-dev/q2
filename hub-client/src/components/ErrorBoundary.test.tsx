/**
 * Tests for the reusable ErrorBoundary.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import { useState } from 'react';
import { ErrorBoundary } from './ErrorBoundary';

afterEach(cleanup);

// A child that throws on render — React surfaces it to the nearest boundary.
function Boom({ when = true }: { when?: boolean }) {
  if (when) throw new Error('kaboom');
  return <div>child ok</div>;
}

// React logs caught boundary errors to console.error; silence it per test.
function withSilencedConsole(fn: () => void) {
  const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
  try {
    fn();
  } finally {
    spy.mockRestore();
  }
}

describe('ErrorBoundary', () => {
  it('renders children unchanged when nothing throws', () => {
    render(
      <ErrorBoundary>
        <div>safe child</div>
      </ErrorBoundary>,
    );
    expect(screen.getByText('safe child')).toBeTruthy();
  });

  it('renders an actionable fallback (not a blank tree) when a child throws', () => {
    withSilencedConsole(() => {
      render(
        <ErrorBoundary>
          <Boom />
        </ErrorBoundary>,
      );
    });
    // The whole subtree must not vanish: the fallback surfaces the error and a
    // recovery affordance instead of blanking the session.
    expect(screen.getByText(/something went wrong/i)).toBeTruthy();
    expect(screen.getByText('kaboom')).toBeTruthy();
    expect(screen.getByRole('button', { name: /reload/i })).toBeTruthy();
  });

  it('renders a custom fallback node when provided', () => {
    withSilencedConsole(() => {
      render(
        <ErrorBoundary fallback={<div>editor unavailable</div>}>
          <Boom />
        </ErrorBoundary>,
      );
    });
    expect(screen.getByText('editor unavailable')).toBeTruthy();
  });

  it('passes the error + reset() to a function fallback, and reset() recovers', () => {
    function Harness() {
      const [explode, setExplode] = useState(true);
      return (
        <ErrorBoundary
          fallback={(error, reset) => (
            <button
              onClick={() => {
                setExplode(false);
                reset();
              }}
            >
              retry: {error.message}
            </button>
          )}
        >
          <Boom when={explode} />
        </ErrorBoundary>
      );
    }
    withSilencedConsole(() => {
      render(<Harness />);
      fireEvent.click(screen.getByText('retry: kaboom'));
    });
    expect(screen.getByText('child ok')).toBeTruthy();
  });

  it('invokes onError with the caught error', () => {
    const onError = vi.fn();
    withSilencedConsole(() => {
      render(
        <ErrorBoundary onError={onError}>
          <Boom />
        </ErrorBoundary>,
      );
    });
    expect(onError).toHaveBeenCalledOnce();
    expect(onError.mock.calls[0][0]).toBeInstanceOf(Error);
  });
});
