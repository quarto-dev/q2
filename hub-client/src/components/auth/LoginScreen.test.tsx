/**
 * Tests for LoginScreen.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import type { ReactNode } from 'react';

import { AuthProviderRoot } from '../../auth/AuthProvider';
import { createMockAuthProvider, type MockAuthProvider } from '../../auth/MockAuthProvider';
import { landing } from '../../strings';
import { LoginScreen } from './LoginScreen';

let mock: MockAuthProvider;

beforeEach(() => {
  mock = createMockAuthProvider();
});

afterEach(() => {
  cleanup();
  vi.unstubAllEnvs();
});

function withProvider(children: ReactNode) {
  return (
    <AuthProviderRoot provider={mock.provider}>{children}</AuthProviderRoot>
  );
}

/**
 * The landing page at quarto-hub.com (bd-g0uyp2v1).
 *
 * This screen is what anyone without a session sees, including someone
 * who followed an invite link, and it used to say only "Quarto Hub" and
 * "Sign in with Google to continue" — nothing about what the product is.
 * The intro is borrowed from the project's own site and must survive the
 * error and expiry states, where the invite-only note is the most useful
 * thing on the page.
 */
describe('LoginScreen intro', () => {
  it('says what Quarto Hub is', () => {
    render(withProvider(<LoginScreen />));
    expect(screen.getByText(/Prose and code belong in one place/)).toBeTruthy();
    expect(
      screen.getByText(/a Quarto editor in the browser that renders while you type/),
    ).toBeTruthy();
  });

  it('says the product is collaborative, not just a browser editor', () => {
    const { container } = render(withProvider(<LoginScreen />));
    const what = container.querySelector('.ls-what')!.textContent!;
    // "So do the people" only gestures at it. If the description does
    // not deliver it, the page describes a single-player editor and the
    // tagline's second line has nothing behind it.
    expect(what, 'the description never mentions sharing').toMatch(/\bshare\b/i);
    expect(what, 'the description never mentions anyone else').toMatch(
      /\b(team|teammates|collaborat\w*|colleagues|together)\b/i,
    );
  });

  it('breaks the tagline after the first sentence', () => {
    const { container } = render(withProvider(<LoginScreen />));
    const lines = container.querySelectorAll('.ls-tagline-line');
    expect(lines).toHaveLength(2);
    expect(lines[0].textContent).toBe('Prose and code belong in one place.');
    expect(lines[1].textContent).toBe('So do the people.');
  });

  it('no longer lists "what works today"', () => {
    render(withProvider(<LoginScreen />));
    for (const item of ['What works today', 'Websites and docs', 'Meeting notes', 'Agents']) {
      expect(screen.queryByText(item), `"${item}" should be gone`).toBeNull();
    }
  });

  /** Child class names of the card, in document order. */
  function cardOrder(container: HTMLElement): string[] {
    return [...container.querySelector('.ls-card')!.children].map((el) => el.className);
  }

  it('puts the invite-only line and Learn more above the sign-in button', () => {
    const { container } = render(withProvider(<LoginScreen />));
    const order = cardOrder(container);
    const idx = (cls: string) => order.findIndex((c) => c.includes(cls));
    expect(idx('ls-footnote')).toBeGreaterThan(-1);
    expect(idx('ls-actions')).toBeGreaterThan(idx('ls-footnote'));
    // Nothing below the button.
    expect(idx('ls-actions')).toBe(order.length - 1);
  });

  it('slots a status line between the footnote and the button when there is one', () => {
    const { container } = render(withProvider(<LoginScreen errorReason="denied" />));
    const order = cardOrder(container);
    const idx = (cls: string) => order.findIndex((c) => c.includes(cls));
    expect(idx('ls-error')).toBeGreaterThan(idx('ls-footnote'));
    expect(idx('ls-actions')).toBeGreaterThan(idx('ls-error'));
    expect(idx('ls-actions')).toBe(order.length - 1);
  });

  it('carries Learn more inside the description, not the invite-only footnote', () => {
    const { container } = render(withProvider(<LoginScreen />));
    const link = screen.getByRole('link', { name: /Learn more/i });
    const what = container.querySelector('.ls-what')!;
    const footnote = container.querySelector('.ls-footnote')!;

    expect(what.contains(link), 'Learn more should follow the description').toBe(true);
    expect(footnote.contains(link), 'Learn more should not sit in the footnote').toBe(
      false,
    );
    // An inline continuation of the sentence, so it takes the separating
    // space that a block-level link had to omit.
    expect(what.textContent).toBe(`${landing.what} ${landing.learnMore}`);
    expect(footnote.textContent).toBe(landing.inviteOnly);
  });

  it('says the service is invite only, so a visitor who cannot get in learns why', () => {
    render(withProvider(<LoginScreen />));
    expect(screen.getByText(/invite only/i)).toBeTruthy();
  });

  it('links Learn more at the Quarto Hub site, in a new tab', () => {
    render(withProvider(<LoginScreen />));
    const link = screen.getByRole('link', { name: /Learn more/i });
    expect(link.getAttribute('href')).toBe('https://quarto-dev.github.io/quarto-hub/');
    // Leaving in the same tab abandons the sign-in a visitor came here
    // to complete — and on an invite, the invite link with it.
    expect(link.getAttribute('target')).toBe('_blank');
    expect(link.getAttribute('rel')).toBe('noopener noreferrer');
  });

  it('does not claim an account is unnecessary while showing a sign-in wall', () => {
    // The site says a project link needs no account or Quarto install.
    // True of the product's intent, but this deployment is allowlisted
    // and the visitor is looking at a sign-in button, so the claim would
    // contradict its own page.
    render(withProvider(<LoginScreen />));
    expect(screen.queryByText(/don't need an account/i)).toBeNull();
    expect(screen.queryByText(/no account/i)).toBeNull();
  });

  it('keeps the intro alongside an auth error', () => {
    render(withProvider(<LoginScreen errorReason="denied" />));
    expect(screen.getByText(/not authorized to access this hub/i)).toBeTruthy();
    expect(screen.getByText(/Prose and code belong in one place/)).toBeTruthy();
    expect(screen.getByText(/invite only/i)).toBeTruthy();
  });

  it('keeps the intro alongside a session-expiry message', () => {
    render(withProvider(<LoginScreen message="Your session expired — please sign in again." />));
    expect(screen.getByText(/session expired/i)).toBeTruthy();
    expect(screen.getByText(/Prose and code belong in one place/)).toBeTruthy();
  });
});

describe('LoginScreen', () => {
  it("renders the provider's SignInButton with loginUri = origin + /auth/callback", () => {
    render(withProvider(<LoginScreen />));

    // SignInButton mounted (mock renders a data-testid button).
    expect(screen.getByTestId('auth-signin')).toBeTruthy();

    // loginUri threaded through to the provider.
    expect(mock.lastLoginUri).not.toBeNull();
    expect(mock.lastLoginUri).toBe(window.location.origin + '/auth/callback');
  });

  it('prefixes the callback with the hub base path under a subpath mount', () => {
    vi.stubEnv('VITE_HUB_BASE_PATH', '/subpath');
    render(withProvider(<LoginScreen />));

    expect(mock.lastLoginUri).toBe(window.location.origin + '/subpath/auth/callback');
  });

  it('leaves the status slot empty in the default state, rather than echoing the button', () => {
    const { container } = render(withProvider(<LoginScreen />));
    // The provider's button already says "Continue with Google"; a
    // "Sign in with Google to continue" line directly above it said the
    // same thing twice, which only got louder once both were centered.
    expect(screen.queryByText(/sign in with google/i)).toBeNull();
    expect(container.querySelector('.ls-note')).toBeNull();
    expect(container.querySelector('.ls-error')).toBeNull();
    expect(screen.queryByText(/not authorized/i)).toBeNull();
    expect(screen.queryByText(/didn't complete/i)).toBeNull();
  });

  // One case per user-facing message. Eleven distinct causes used to
  // collapse into the "not authorized" sentence, sending users who needed
  // a reload to an administrator instead.
  it('tells a stale client its bundle is out of date', () => {
    render(withProvider(<LoginScreen errorReason="stale_client" />));
    expect(screen.getByText(/out of date.*try again/i)).toBeTruthy();
    expect(screen.queryByText(/not authorized/i)).toBeNull();
  });

  it('tells a broken-down sign-in to try again', () => {
    render(withProvider(<LoginScreen errorReason="restart" />));
    expect(screen.getByText(/didn't complete.*try again/i)).toBeTruthy();
    expect(screen.queryByText(/not authorized/i)).toBeNull();
  });

  it('tells a refused identity it is not authorized', () => {
    render(withProvider(<LoginScreen errorReason="denied" />));
    expect(screen.getByText(/not authorized to access this hub/i)).toBeTruthy();
  });

  it('reports a hub-side failure as a hub-side failure', () => {
    render(withProvider(<LoginScreen errorReason="server" />));
    expect(screen.getByText(/went wrong on the hub/i)).toBeTruthy();
    expect(screen.queryByText(/not authorized/i)).toBeNull();
  });

  // A bare `/?auth_error` from a pre-E1 hub parses to `''`, which is
  // falsy — the error must still show, and as the retry copy rather than
  // the alarming one.
  it('renders the retry copy for an empty reason, not nothing', () => {
    render(withProvider(<LoginScreen errorReason="" />));
    expect(screen.getByText(/didn't complete.*try again/i)).toBeTruthy();
    expect(screen.queryByText(/Sign in with Google to continue/i)).toBeNull();
  });

  it('renders the retry copy for an unknown reason, never "not authorized"', () => {
    render(withProvider(<LoginScreen errorReason="something-we-do-not-know" />));
    expect(screen.getByText(/didn't complete.*try again/i)).toBeTruthy();
    expect(screen.queryByText(/not authorized/i)).toBeNull();
  });

  it('never renders the reason string itself', () => {
    render(withProvider(<LoginScreen errorReason="<img src=x onerror=1>" />));
    expect(screen.queryByText(/onerror/i)).toBeNull();
    expect(screen.getByText(/didn't complete.*try again/i)).toBeTruthy();
  });

  it('renders a custom message (session expiry) instead of the default copy', () => {
    render(withProvider(<LoginScreen message="Your session expired — please sign in again." />));
    expect(screen.getByText(/session expired/i)).toBeTruthy();
    expect(screen.queryByText(/Sign in with Google to continue/i)).toBeNull();
  });

  it('error copy wins over a custom message', () => {
    render(withProvider(<LoginScreen errorReason="denied" message="Your session expired — please sign in again." />));
    expect(screen.getByText(/not authorized/i)).toBeTruthy();
    expect(screen.queryByText(/session expired/i)).toBeNull();
  });
});
