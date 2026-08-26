/**
 * JoinCollectionLanding — invite-first onboarding for a shared collection.
 *
 * Rendered for #/join-collection/ links. A fresh browser never sees the
 * setup screen: App auto-creates a personal root collection silently while
 * this screen asks the one question that matters — who you are. Joining
 * subscribes this browser to the collection document (appending its doc id
 * to the collections pointer array); the collection's projects arrive by
 * sync, shared for real.
 */

import { useState, useEffect } from 'react';
import Tooltip from './Tooltip';
import type { CollectionsStatus } from '../hooks/useCollectionSets';
import { CollectionConnectError } from '../services/collectionConnectError';
import type { UserSettings } from '../services/storage/types';
import * as userSettingsService from '../services/userSettings';
import type { JoinCollectionRoute } from '../utils/routing';
import { DEFAULT_SYNC_SERVER } from '../utils/routing';
import './ProjectsHome.css';

const COLOR_PALETTE = [
  '#E91E63', '#9C27B0', '#3F51B5', '#2196F3',
  '#00BCD4', '#009688', '#4CAF50', '#FF9800',
  '#FF5722', '#795548',
];

function initialsFor(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return '?';
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

interface Props {
  route: JoinCollectionRoute;
  status: CollectionsStatus;
  /** Subscribe this browser to the collection document. */
  onSubscribe: (collectionDocId: string, syncServer: string) => Promise<void>;
  /** Navigate home once the join completes. */
  onDone: () => void;
  /**
   * Recover from an expired session. Defaults to a page reload:
   * main.tsx saves the current hash (this join route) before React
   * mounts, the login screen appears, and the post-auth restore brings
   * the user straight back here.
   */
  onSignInAgain?: () => void;
}

/** Join failure presented to the user: message plus recovery affordance. */
interface JoinError {
  message: string;
  canSignInAgain: boolean;
}

export default function JoinCollectionLanding({
  route,
  status,
  onSubscribe,
  onDone,
  onSignInAgain = () => window.location.reload(),
}: Props) {
  const [userSettings, setUserSettings] = useState<UserSettings | null>(null);
  const [name, setName] = useState('');
  const [color, setColor] = useState(COLOR_PALETTE[0]);
  const [joining, setJoining] = useState(false);
  const [error, setError] = useState<JoinError | null>(null);

  useEffect(() => {
    userSettingsService.getUserIdentity().then((s) => {
      setUserSettings(s);
      setName(s.userName);
      setColor(s.userColor);
    }).catch((err) => console.error('Failed to load identity:', err));
  }, []);

  const ready = status === 'connected';

  const handleJoin = async () => {
    if (!name.trim() || !ready || joining) return;
    setJoining(true);
    setError(null);
    try {
      if (userSettings && name.trim() !== userSettings.userName) {
        await userSettingsService.updateUserName(name.trim());
      }
      if (userSettings && color !== userSettings.userColor) {
        await userSettingsService.updateUserColor(color);
      }
      await onSubscribe(route.collectionId, route.syncServer || DEFAULT_SYNC_SERVER);
      onDone();
    } catch (err) {
      console.error('Join failed:', err);
      // CollectionConnectError carries user-facing copy classified by
      // failure mode (bd-tux4m6od); anything else falls back to its
      // own message.
      setError({
        message: err instanceof Error ? err.message : 'Could not join the collection.',
        canSignInAgain:
          err instanceof CollectionConnectError && err.kind === 'auth-expired',
      });
    } finally {
      setJoining(false);
    }
  };

  return (
    <div className="projects-home">
      <div className="qh-join">
        <div className="qh-join-card">
          <div className="qh-join-kicker">COLLECTION INVITATION</div>
          <h1>
            {route.inviter} invited you to <span className="qh-join-collection-name">{route.collectionName}</span>
          </h1>
          <p className="qh-join-sub">
            A shared collection of Quarto projects — its contents sync to you when you join.
          </p>
          {error && (
            <div className="qh-error inline">
              {error.message}
              {error.canSignInAgain && (
                <button className="qh-btn qh-error-action" onClick={onSignInAgain}>
                  Sign in again
                </button>
              )}
            </div>
          )}

          <div className="qh-join-identity">
            <div className="qh-field-label">How you'll appear to the team</div>
            <div className="qh-join-identity-row">
              <span className="qh-avatar big" style={{ backgroundColor: color }}>
                {initialsFor(name || '?')}
              </span>
              <input
                className="qh-input"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Your name"
                autoFocus
              />
            </div>
            <div className="qh-swatches">
              {COLOR_PALETTE.map((c) => (
                <Tooltip key={c} content={c}>
                  <button
                    className={`qh-swatch ${color === c ? 'selected' : ''}`}
                    style={{ backgroundColor: c }}
                    onClick={() => setColor(c)}
                    aria-label={`Cursor color ${c}`}
                  />
                </Tooltip>
              ))}
            </div>
          </div>

          <div className="qh-join-actions">
            <button className="qh-btn primary" onClick={handleJoin} disabled={!ready || joining || !name.trim()}>
              {joining ? 'Joining…' : ready ? `Join ${route.collectionName}` : 'Connecting…'}
            </button>
          </div>
          <p className="qh-join-note">
            Joining subscribes you to this collection — anyone with the link can join and
            add or remove projects.
          </p>
        </div>
      </div>
    </div>
  );
}
