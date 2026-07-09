/**
 * JoinCollectionLanding — invite-first onboarding for a shared collection
 * (explore/projects-collections-ui exploration).
 *
 * Rendered for #/join-collection/ links. A fresh browser never sees the
 * project-set setup screen: App auto-creates a set silently while this
 * screen asks the one question that matters — who you are. Joining adds
 * the invite's project entries (real doc ids, so they sync for real) and
 * creates the collection locally with mock membership.
 */

import { useState, useEffect } from 'react';
import type { ProjectSetEntry } from '@quarto/quarto-automerge-schema';
import type { ProjectSetStatus } from '../hooks/useProjectSet';
import type { UserSettings } from '../services/storage/types';
import * as userSettingsService from '../services/userSettings';
import type { JoinCollectionRoute } from '../utils/routing';
import { createSharedCollectionFromInvite, type CollectionMember } from '../hooks/useCollections';
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
  projectSetStatus: ProjectSetStatus;
  onAddProjectToSet: (entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>) => void;
  /** Navigate home once the join completes. */
  onDone: () => void;
}

export default function JoinCollectionLanding({ route, projectSetStatus, onAddProjectToSet, onDone }: Props) {
  const [userSettings, setUserSettings] = useState<UserSettings | null>(null);
  const [name, setName] = useState('');
  const [color, setColor] = useState(COLOR_PALETTE[0]);
  const [joining, setJoining] = useState(false);

  useEffect(() => {
    userSettingsService.getUserIdentity().then((s) => {
      setUserSettings(s);
      setName(s.userName);
      setColor(s.userColor);
    }).catch((err) => console.error('Failed to load identity:', err));
  }, []);

  const ready = projectSetStatus === 'connected';

  const handleJoin = async () => {
    if (!name.trim() || !ready) return;
    setJoining(true);
    try {
      if (userSettings && name.trim() !== userSettings.userName) {
        await userSettingsService.updateUserName(name.trim());
      }
      if (userSettings && color !== userSettings.userColor) {
        await userSettingsService.updateUserColor(color);
      }
      for (const entry of route.entries) {
        const indexDocId = entry.indexDocId.startsWith('automerge:')
          ? entry.indexDocId
          : `automerge:${entry.indexDocId}`;
        onAddProjectToSet({ indexDocId, syncServer: entry.syncServer, description: entry.description });
      }
      const members: CollectionMember[] = [
        {
          name: route.inviter,
          initials: initialsFor(route.inviter),
          color: '#7C3AED',
          joinedAt: new Date().toISOString(),
          isOwner: true,
        },
        {
          name: name.trim(),
          initials: initialsFor(name.trim()),
          color,
          joinedAt: new Date().toISOString(),
          isYou: true,
        },
      ];
      createSharedCollectionFromInvite({
        collectionId: route.collectionId,
        name: route.collectionName,
        // Store ids in the same un-prefixed form the invite carries; the
        // home view matches them against project-set entries either way.
        projectIds: route.entries.map((e) => e.indexDocId.replace(/^automerge:/, '')),
        members,
      });
      onDone();
    } finally {
      setJoining(false);
    }
  };

  return (
    <div className="projects-home">
      <div className="ph-join">
        <div className="ph-join-card">
          <div className="ph-join-kicker">COLLECTION INVITATION</div>
          <h1>
            {route.inviter} invited you to <span className="ph-join-collection-name">{route.collectionName}</span>
          </h1>
          <p className="ph-join-sub">
            {route.entries.length === 0
              ? 'A shared collection of Quarto projects.'
              : `A shared collection of ${route.entries.length} Quarto project${route.entries.length === 1 ? '' : 's'}:`}
          </p>
          {route.entries.length > 0 && (
            <ul className="ph-join-projects">
              {route.entries.slice(0, 5).map((e) => (
                <li key={e.indexDocId}>{e.description || 'Untitled project'}</li>
              ))}
              {route.entries.length > 5 && <li className="muted">and {route.entries.length - 5} more…</li>}
            </ul>
          )}

          <div className="ph-join-identity">
            <div className="ph-field-label">How you'll appear to the team</div>
            <div className="ph-join-identity-row">
              <span className="ph-avatar big" style={{ backgroundColor: color }}>
                {initialsFor(name || '?')}
              </span>
              <input
                className="ph-input"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Your name"
                autoFocus
              />
            </div>
            <div className="ph-swatches">
              {COLOR_PALETTE.map((c) => (
                <button
                  key={c}
                  className={`ph-swatch ${color === c ? 'selected' : ''}`}
                  style={{ backgroundColor: c }}
                  onClick={() => setColor(c)}
                  title={c}
                />
              ))}
            </div>
          </div>

          <div className="ph-join-actions">
            <button className="ph-btn primary" onClick={handleJoin} disabled={!ready || joining || !name.trim()}>
              {joining ? 'Joining…' : ready ? `Join ${route.collectionName}` : 'Connecting…'}
            </button>
          </div>
          <p className="ph-join-note">
            Joining adds these projects to your list. This invite flow is part of a UI
            exploration — collection membership isn't synced to other members yet.
          </p>
        </div>
      </div>
    </div>
  );
}
