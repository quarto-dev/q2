/**
 * UpdateAvailableToast — the visible-tab half of the service-worker
 * update flow (GH #447, bd-axqunnx9). When a new SW activates while
 * this tab is visible, `pwa.ts` shows this toast instead of reloading
 * outright: the user reloads via the button, or the tab reloads itself
 * on its next transition to hidden. Dismissing only hides the toast —
 * the hide-reload path stays armed.
 *
 * Rendered as a sibling of `<App />` in the root tree so it overlays
 * both the login screen (the #447 context) and the editor. Visibility
 * comes from the module-level `pwaPrompt` store, so a `show()` fired
 * before React mounts is not lost.
 */

import { useState, useSyncExternalStore } from 'react';
import { pwaPrompt } from '../pwaPrompt';
import type { PwaPromptStore } from '../pwaPrompt';
import './UpdateAvailableToast.css';

export default function UpdateAvailableToast({
  prompt = pwaPrompt,
}: {
  prompt?: PwaPromptStore;
}) {
  const pending = useSyncExternalStore(prompt.subscribe, prompt.isPending);
  const [dismissed, setDismissed] = useState(false);

  if (!pending || dismissed) return null;

  return (
    <div className="update-available-toast" role="status">
      <span>A new version is available.</span>
      <button
        type="button"
        className="ph-btn primary"
        onClick={() => window.location.reload()}
      >
        Reload
      </button>
      <button
        type="button"
        className="update-available-toast-dismiss"
        aria-label="Dismiss"
        onClick={() => setDismissed(true)}
      >
        ×
      </button>
    </div>
  );
}
