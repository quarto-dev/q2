/**
 * Tests for the service-worker update flow (GH #447, bd-axqunnx9).
 *
 * Pins the hybrid reload policy: a tab that is hidden when the new SW
 * activates reloads itself immediately (the self-heal path for tabs left
 * open for hours); a visible tab gets the update prompt instead and
 * reloads on its next transition to hidden — never against the user's
 * will. Also pins the update polling that lets long-lived tabs discover
 * new versions at all: an hourly interval plus a check on every
 * hidden → visible transition.
 *
 * `virtual:pwa-register` is mocked — the real module is a vite-plugin-pwa
 * build artifact (a no-op stub outside production builds), and the tests
 * need to capture and drive the callbacks passed to `registerSW`.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { RegisterSWOptions } from 'vite-plugin-pwa/types';

const { registerSWMock } = vi.hoisted(() => ({ registerSWMock: vi.fn() }));
vi.mock('virtual:pwa-register', () => ({ registerSW: registerSWMock }));

import { setupSwUpdates } from './pwa';

const ONE_HOUR_MS = 60 * 60 * 1000;

const reloadMock = vi.fn();
const prompt = { show: vi.fn() };

/** Listeners pwa.ts attached to `document`, so tests can detach them. */
const addedListeners: Array<{ type: string; listener: EventListener }> = [];

function setVisibilityState(state: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', {
    configurable: true,
    value: state,
  });
  document.dispatchEvent(new Event('visibilitychange'));
}

function registeredOptions(): RegisterSWOptions {
  expect(registerSWMock).toHaveBeenCalledOnce();
  return registerSWMock.mock.calls[0][0] as RegisterSWOptions;
}

function fakeRegistration() {
  return { update: vi.fn() } as unknown as ServiceWorkerRegistration;
}

beforeEach(() => {
  registerSWMock.mockClear();
  prompt.show.mockClear();
  reloadMock.mockClear();
  Object.defineProperty(window, 'location', {
    configurable: true,
    value: { ...window.location, reload: reloadMock },
  });
  Object.defineProperty(document, 'visibilityState', {
    configurable: true,
    value: 'visible',
  });
  const realAdd = document.addEventListener.bind(document);
  vi.spyOn(document, 'addEventListener').mockImplementation(
    (type, listener, options) => {
      addedListeners.push({ type, listener: listener as EventListener });
      realAdd(type, listener, options);
    },
  );
});

afterEach(() => {
  for (const { type, listener } of addedListeners.splice(0)) {
    document.removeEventListener(type, listener);
  }
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe('setupSwUpdates', () => {
  it('registers the service worker immediately', () => {
    setupSwUpdates(prompt);
    expect(registeredOptions().immediate).toBe(true);
  });

  it('reloads immediately when the tab is hidden on update activation', () => {
    setupSwUpdates(prompt);
    setVisibilityState('hidden');
    registeredOptions().onNeedReload!();
    expect(reloadMock).toHaveBeenCalledOnce();
    expect(prompt.show).not.toHaveBeenCalled();
  });

  it('shows the prompt without reloading when the tab is visible', () => {
    setupSwUpdates(prompt);
    registeredOptions().onNeedReload!();
    expect(prompt.show).toHaveBeenCalledOnce();
    expect(reloadMock).not.toHaveBeenCalled();
  });

  it('reloads a prompted tab on its next transition to hidden', () => {
    setupSwUpdates(prompt);
    registeredOptions().onNeedReload!();
    expect(reloadMock).not.toHaveBeenCalled();
    setVisibilityState('hidden');
    expect(reloadMock).toHaveBeenCalledOnce();
  });

  it('does not reload a prompted tab that stays visible', () => {
    setupSwUpdates(prompt);
    registeredOptions().onNeedReload!();
    setVisibilityState('visible');
    expect(reloadMock).not.toHaveBeenCalled();
  });

  it('polls for updates hourly once the SW is registered', () => {
    vi.useFakeTimers();
    const registration = fakeRegistration();
    setupSwUpdates(prompt);
    registeredOptions().onRegisteredSW!('/sw.js', registration);
    expect(registration.update).not.toHaveBeenCalled();
    vi.advanceTimersByTime(ONE_HOUR_MS);
    expect(registration.update).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(ONE_HOUR_MS);
    expect(registration.update).toHaveBeenCalledTimes(2);
  });

  it('checks for updates when a hidden tab becomes visible', () => {
    const registration = fakeRegistration();
    setupSwUpdates(prompt);
    registeredOptions().onRegisteredSW!('/sw.js', registration);
    setVisibilityState('hidden');
    expect(registration.update).not.toHaveBeenCalled();
    setVisibilityState('visible');
    expect(registration.update).toHaveBeenCalledOnce();
  });

  it('starts no polling when registration is unavailable', () => {
    vi.useFakeTimers();
    setupSwUpdates(prompt);
    registeredOptions().onRegisteredSW!('/sw.js', undefined);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('logs registration failures instead of throwing', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    setupSwUpdates(prompt);
    const error = new Error('boom');
    registeredOptions().onRegisterError!(error);
    expect(warn).toHaveBeenCalledWith(
      'Service worker registration failed',
      error,
    );
  });
});
