/**
 * Service-worker update flow (GH #447, bd-axqunnx9).
 *
 * The generated SW keeps `registerType: 'autoUpdate'` semantics — it
 * calls `skipWaiting()` + `clientsClaim()`, so a new version activates
 * immediately and the client module fires `onNeedReload` on `activated`
 * (`isUpdate || isExternal`). What this module overrides is *when the
 * reload happens*:
 *
 * - Tab hidden  → reload immediately. A backgrounded tab silently heals
 *   itself — the exact #447 scenario (tab left open for hours).
 * - Tab visible → show the update prompt and reload on the next
 *   transition to hidden. A visible tab is never reloaded against the
 *   user's will; the prompt's Reload button and the hide-reload are the
 *   only paths.
 *
 * The poll below is the other half of the fix: without it, an idle tab
 * only checked for updates on page load and navigation soft-updates, so
 * a tab left open for hours never learned about a deploy. With nginx's
 * `no-cache` on sw.js each check is a cheap conditional request. Focus
 * checks are throttled to one per 5 minutes (the hourly poll is not — it
 * drives the self-heal above and always runs on schedule), and a failed
 * check (e.g. offline right after waking from sleep) is swallowed and
 * retried on the next trigger.
 *
 * The prompt UI is injected as the `UpdatePrompt` interface so this
 * module stays DOM-free and unit-testable. In `vite dev` and in E2E /
 * preview-embed builds (`disable: true`) `registerSW` is a no-op stub,
 * so none of these paths run there.
 */

import { registerSW } from 'virtual:pwa-register';

/** Hourly SW update poll; also re-checks when a hidden tab becomes visible. */
const UPDATE_INTERVAL_MS = 60 * 60 * 1000;

/**
 * Minimum gap between focus-triggered update checks, so rapid tab
 * switching collapses into a single conditional request. The hourly poll
 * is deliberately not throttled: it drives the hidden-tab self-heal and
 * must run on schedule.
 */
const MIN_CHECK_GAP_MS = 5 * 60 * 1000;

export interface UpdatePrompt {
  show(): void;
}

export function setupSwUpdates(prompt: UpdatePrompt): void {
  registerSW({
    immediate: true,
    onNeedReload() {
      // New SW already activated; the running bundle is stale.
      if (document.visibilityState === 'hidden') {
        window.location.reload();
        return;
      }
      prompt.show();
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'hidden') window.location.reload();
      });
    },
    onRegisteredSW(_swUrl, registration) {
      if (!registration) return;
      // update() rejects when the machine is offline (typical right after
      // waking from sleep); swallow failures — the next poll or focus
      // simply retries.
      const tryUpdate = () => registration.update().catch(() => {});
      // The hourly poll drives the hidden-tab self-heal, so it always
      // runs on schedule and is never throttled by focus checks.
      setInterval(tryUpdate, UPDATE_INTERVAL_MS);
      let lastFocusCheck = 0;
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState !== 'visible') return;
        const now = Date.now();
        if (now - lastFocusCheck < MIN_CHECK_GAP_MS) return;
        lastFocusCheck = now;
        registration.update().catch(() => {
          // Failed check: reset the throttle so the next focus retries
          // immediately instead of waiting out the gap.
          lastFocusCheck = 0;
        });
      });
    },
    onRegisterError(error) {
      console.warn('Service worker registration failed', error);
    },
  });
}
