/**
 * Full-page card shown when the personal root collection could not be
 * established or reconnected (collections status 'error').
 *
 * Replaces the old ProjectSetSetup form on that path (bd-4h1hv60p): the
 * root is now created silently on first run, so the only thing left to
 * offer a user here is another attempt. There is deliberately no sync
 * server field — which server a deployment uses is fixed at build time
 * (`VITE_DEFAULT_SYNC_SERVER`), never a user decision.
 *
 * Reuses the projects-home empty-state styling so it reads like the
 * "Couldn't load your projects" state next door.
 */

import { common } from '../strings';
import './ProjectsHome.css';

interface Props {
  /** Error text from the collections hook. */
  error: string | null;
  /** Re-run the collections initialization. */
  onRetry: () => void;
}

export default function ProjectSetError({ error, onRetry }: Props) {
  return (
    <div className="projects-home">
      <main id="main-content" tabIndex={-1} className="qh-main">
        <div className="qh-empty-state" role="alert">
          <h2>Couldn't connect to the sync server</h2>
          <p>
            Your project list lives on the sync server, and it did not answer.
            {error ? ` (${error})` : ''}
          </p>
          <div className="qh-empty-actions">
            <button type="button" className="qh-btn primary" onClick={onRetry}>
              {common.retry}
            </button>
          </div>
        </div>
      </main>
    </div>
  );
}
