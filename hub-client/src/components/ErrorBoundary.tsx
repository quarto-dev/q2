/**
 * Reusable React error boundary.
 *
 * A throw in a descendant's render/lifecycle unmounts only this subtree and
 * shows a fallback, instead of propagating up and blanking the whole app. Wrap
 * isolatable regions (e.g. the Monaco `<Editor>`) so a bug in one pane can't
 * take down the session.
 *
 * Note: like all React error boundaries, this catches errors thrown during
 * render/lifecycle — NOT async callbacks or event handlers. Monaco's own
 * background tokenizer throws are already absorbed by its `safeTokenize`.
 */

import { Component } from 'react';
import type { ErrorInfo, ReactNode } from 'react';

interface Props {
  children: ReactNode;
  /** Custom fallback. A function receives the error + a reset() to retry. */
  fallback?: ReactNode | ((error: Error, reset: () => void) => ReactNode);
  /** Side-effect hook for the caught error (logging/telemetry). */
  onError?: (error: Error, info: ErrorInfo) => void;
}

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('[ErrorBoundary] Caught error:', error, info);
    this.props.onError?.(error, info);
  }

  private reset = (): void => this.setState({ error: null });

  render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;

    const { fallback } = this.props;
    if (typeof fallback === 'function') return fallback(error, this.reset);
    if (fallback !== undefined) return fallback;
    return <DefaultFallback error={error} onReset={this.reset} />;
  }
}

function DefaultFallback({ error, onReset }: { error: Error; onReset: () => void }) {
  return (
    <div
      role="alert"
      style={{
        padding: '24px',
        margin: '16px',
        backgroundColor: '#fee',
        border: '1px solid #fcc',
        borderRadius: '6px',
        fontFamily: 'system-ui, sans-serif',
        fontSize: '14px',
        color: '#900',
      }}
    >
      <h3 style={{ margin: '0 0 8px 0' }}>Something went wrong</h3>
      <p style={{ margin: '0 0 12px 0' }}>{error.message}</p>
      <div style={{ display: 'flex', gap: '8px' }}>
        <button onClick={onReset}>Try again</button>
        <button onClick={() => window.location.reload()}>Reload</button>
      </div>
    </div>
  );
}
