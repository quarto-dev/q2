/**
 * EditorWelcomeBanner — one-time welcome bar shown under the editor toolbar
 * after arriving via an invite (bd-fxdcxbpq).
 *
 * Shown once per invite target (collection or shared project); dismissal is
 * persisted in localStorage keyed by the target's doc id, following the
 * MOVE_WARNING_KEY precedent in ProjectsHome.
 */

import { useState } from 'react';
import './EditorWelcomeBanner.css';

/** localStorage prefix; the full key is `${prefix}${targetId}`. */
export const WELCOME_DISMISSED_KEY_PREFIX = 'qh-invite-welcome-dismissed:';

export interface EditorWelcomeBannerProps {
  kind: 'collection' | 'document';
  /** Doc id of the joined collection or shared project (dismissal key). */
  targetId: string;
  /** Collection name (collection variant copy). */
  targetName: string;
  /** Display name of the person who sent the invite. */
  inviter: string;
  /** The recipient's live editing identity. */
  userName: string;
  /** Persist a new display name (from the inline rename). */
  onRename: (name: string) => Promise<void> | void;
}

function isDismissed(targetId: string): boolean {
  try {
    return localStorage.getItem(`${WELCOME_DISMISSED_KEY_PREFIX}${targetId}`) === '1';
  } catch {
    return false;
  }
}

export default function EditorWelcomeBanner({
  kind,
  targetId,
  targetName,
  inviter,
  userName,
  onRename,
}: EditorWelcomeBannerProps) {
  const [dismissed, setDismissed] = useState(() => isDismissed(targetId));
  const [editing, setEditing] = useState(false);
  const [draftName, setDraftName] = useState(userName);

  if (dismissed) return null;

  const dismiss = () => {
    try {
      localStorage.setItem(`${WELCOME_DISMISSED_KEY_PREFIX}${targetId}`, '1');
    } catch {
      // Storage unavailable — the banner still dismisses for this session.
    }
    setDismissed(true);
  };

  return (
    <div className="ewb" data-testid="editor-welcome-banner">
      <span className="ewb-text">
        {kind === 'collection' ? (
          <>
            Welcome to <strong>{targetName}</strong> — {inviter} suggested starting here. You're
            editing live as <strong>{userName}</strong>.
          </>
        ) : (
          <>
            <strong>{inviter}</strong> shared this document with you. You're editing live as{' '}
            <strong>{userName}</strong>.
          </>
        )}
      </span>
      {editing ? (
        <span className="ewb-rename">
          <input
            className="qh-input"
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            aria-label="New display name"
            autoFocus
          />
          <button
            type="button"
            className="qh-btn primary small"
            onClick={async () => {
              const name = draftName.trim();
              if (!name) return;
              await onRename(name);
              setEditing(false);
            }}
          >
            Save
          </button>
        </span>
      ) : (
        <button
          type="button"
          className="qh-btn outline small"
          onClick={() => {
            setDraftName(userName);
            setEditing(true);
          }}
        >
          Change name
        </button>
      )}
      <button
        type="button"
        className="ewb-dismiss"
        aria-label="Dismiss welcome banner"
        onClick={dismiss}
      >
        ×
      </button>
    </div>
  );
}
