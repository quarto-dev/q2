/**
 * Project Set Setup Component
 *
 * Shown when no project set pointer exists in IndexedDB. Handles:
 * 1. Fresh setup (new user): Create new project set or link to existing one
 * 2. Migration (existing user): Migrate old IDB projects to a synced project set
 */

import { useState, useCallback } from 'react';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';
import { DEFAULT_SYNC_SERVER } from '../utils/routing';
import { exportData } from '../services/projectStorage';
import './ProjectSetSetup.css';

interface Props {
  /** Whether this user has old IDB projects that need migration. */
  hasMigration: boolean;
  /** Old IDB projects (only set when hasMigration is true). */
  legacyProjects: ProjectEntry[];
  /** Error message from a previous attempt. */
  error: string | null;
  /** Whether an operation is in progress. */
  isConnecting: boolean;
  /** Create a new project set. */
  onCreateProjectSet: (syncServer: string) => Promise<void>;
  /** Link to an existing project set (from another browser). */
  onLinkProjectSet: (projectSetDocId: string, syncServer: string) => Promise<void>;
  /** Migrate old IDB projects into a new project set. */
  onMigrateProjects: (syncServer: string) => Promise<void>;
  /** Merge old IDB projects into an existing project set. */
  onMergeIntoProjectSet: (projectSetDocId: string, syncServer: string) => Promise<void>;
}

type SetupMode = 'choose' | 'link' | 'merge';

export default function ProjectSetSetup({
  hasMigration,
  legacyProjects,
  error,
  isConnecting,
  onCreateProjectSet,
  onLinkProjectSet,
  onMigrateProjects,
  onMergeIntoProjectSet,
}: Props) {
  const [mode, setMode] = useState<SetupMode>('choose');
  const [linkDocId, setLinkDocId] = useState('');
  const [syncServer, setSyncServer] = useState(DEFAULT_SYNC_SERVER);
  const [formError, setFormError] = useState<string | null>(null);

  const handleDownloadBackup = useCallback(async () => {
    try {
      const json = await exportData();
      const blob = new Blob([json], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = 'quarto-hub-projects-backup.json';
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      console.error('Failed to export backup:', err);
    }
  }, []);

  const handleMigrate = useCallback(async () => {
    setFormError(null);
    if (!syncServer.trim()) {
      setFormError('Sync server URL is required');
      return;
    }
    await onMigrateProjects(syncServer.trim());
  }, [syncServer, onMigrateProjects]);

  const handleCreateNew = useCallback(async () => {
    setFormError(null);
    if (!syncServer.trim()) {
      setFormError('Sync server URL is required');
      return;
    }
    await onCreateProjectSet(syncServer.trim());
  }, [syncServer, onCreateProjectSet]);

  const handleLink = useCallback(async () => {
    setFormError(null);
    if (!linkDocId.trim()) {
      setFormError('Project set ID is required');
      return;
    }
    if (!syncServer.trim()) {
      setFormError('Sync server URL is required');
      return;
    }

    // Normalize: add automerge: prefix if missing
    let normalizedDocId = linkDocId.trim();
    if (!normalizedDocId.startsWith('automerge:')) {
      normalizedDocId = `automerge:${normalizedDocId}`;
    }

    if (hasMigration) {
      await onMergeIntoProjectSet(normalizedDocId, syncServer.trim());
    } else {
      await onLinkProjectSet(normalizedDocId, syncServer.trim());
    }
  }, [linkDocId, syncServer, hasMigration, onLinkProjectSet, onMergeIntoProjectSet]);

  const displayError = error || formError;

  // ---- Migration screen ----
  if (hasMigration && mode === 'choose') {
    return (
      <div className="project-set-setup">
        <div className="setup-modal">
          <div className="setup-header">
            <h1>Quarto Hub</h1>
            <p className="setup-tagline">Upgrade: Synced Project List</p>
          </div>

          <div className="setup-explanation">
            <p>
              Your project list can now sync across browsers. We'll move your
              {' '}<strong>{legacyProjects.length} project{legacyProjects.length !== 1 ? 's' : ''}</strong>{' '}
              to a synced document so you can access them from any browser.
            </p>
          </div>

          {displayError && <div className="setup-error">{displayError}</div>}

          <div className="setup-backup">
            <button
              className="backup-btn"
              onClick={handleDownloadBackup}
              disabled={isConnecting}
            >
              Download Backup
            </button>
            <span className="backup-hint">
              Recommended: save a backup of your project list before migrating.
            </span>
          </div>

          <div className="setup-form-group">
            <label htmlFor="migrate-sync-server">Sync Server URL</label>
            <input
              id="migrate-sync-server"
              type="text"
              value={syncServer}
              onChange={(e) => setSyncServer(e.target.value)}
              placeholder="wss://sync.automerge.org"
              disabled={isConnecting}
            />
          </div>

          <div className="setup-actions">
            <button
              className="setup-primary-btn"
              onClick={handleMigrate}
              disabled={isConnecting}
            >
              {isConnecting ? 'Migrating...' : 'Migrate Projects'}
            </button>
            <button
              className="setup-secondary-btn"
              onClick={() => setMode('merge')}
              disabled={isConnecting}
            >
              I already have a project set on another browser
            </button>
          </div>
        </div>
      </div>
    );
  }

  // ---- Merge screen (migration with existing project set) ----
  if (mode === 'merge') {
    return (
      <div className="project-set-setup">
        <div className="setup-modal">
          <div className="setup-header">
            <h1>Quarto Hub</h1>
            <p className="setup-tagline">Merge with Existing Project Set</p>
          </div>

          <div className="setup-explanation">
            <p>
              Paste the project set link from your other browser. Your local
              projects will be merged into the existing set.
            </p>
          </div>

          {displayError && <div className="setup-error">{displayError}</div>}

          <div className="setup-form-group">
            <label htmlFor="merge-doc-id">Project Set ID</label>
            <input
              id="merge-doc-id"
              type="text"
              value={linkDocId}
              onChange={(e) => setLinkDocId(e.target.value)}
              placeholder="bs58-encoded document ID"
              disabled={isConnecting}
              autoFocus
            />
          </div>
          <div className="setup-form-group">
            <label htmlFor="merge-sync-server">Sync Server URL</label>
            <input
              id="merge-sync-server"
              type="text"
              value={syncServer}
              onChange={(e) => setSyncServer(e.target.value)}
              placeholder="wss://sync.automerge.org"
              disabled={isConnecting}
            />
          </div>

          <div className="setup-actions">
            <button
              className="setup-secondary-btn"
              onClick={() => setMode('choose')}
              disabled={isConnecting}
            >
              Back
            </button>
            <button
              className="setup-primary-btn"
              onClick={handleLink}
              disabled={isConnecting}
            >
              {isConnecting ? 'Merging...' : 'Merge & Link'}
            </button>
          </div>
        </div>
      </div>
    );
  }

  // ---- Link screen (fresh setup, linking to existing) ----
  if (mode === 'link') {
    return (
      <div className="project-set-setup">
        <div className="setup-modal">
          <div className="setup-header">
            <h1>Quarto Hub</h1>
            <p className="setup-tagline">Link to Existing Project Set</p>
          </div>

          <div className="setup-explanation">
            <p>
              Paste the project set link from your other browser to sync your
              project list to this one.
            </p>
          </div>

          {displayError && <div className="setup-error">{displayError}</div>}

          <div className="setup-form-group">
            <label htmlFor="link-doc-id">Project Set ID</label>
            <input
              id="link-doc-id"
              type="text"
              value={linkDocId}
              onChange={(e) => setLinkDocId(e.target.value)}
              placeholder="bs58-encoded document ID"
              disabled={isConnecting}
              autoFocus
            />
          </div>
          <div className="setup-form-group">
            <label htmlFor="link-sync-server">Sync Server URL</label>
            <input
              id="link-sync-server"
              type="text"
              value={syncServer}
              onChange={(e) => setSyncServer(e.target.value)}
              placeholder="wss://sync.automerge.org"
              disabled={isConnecting}
            />
          </div>

          <div className="setup-actions">
            <button
              className="setup-secondary-btn"
              onClick={() => setMode('choose')}
              disabled={isConnecting}
            >
              Back
            </button>
            <button
              className="setup-primary-btn"
              onClick={handleLink}
              disabled={isConnecting}
            >
              {isConnecting ? 'Linking...' : 'Link Project Set'}
            </button>
          </div>
        </div>
      </div>
    );
  }

  // ---- Fresh setup screen (no migration needed) ----
  return (
    <div className="project-set-setup">
      <div className="setup-modal">
        <div className="setup-header">
          <h1>Quarto Hub</h1>
          <p className="setup-tagline">Multiplayer editing for your Quarto projects</p>
        </div>

        <div className="setup-explanation">
          <p>
            Get started by creating a new project set, or link to an existing
            one from another browser.
          </p>
        </div>

        {displayError && <div className="setup-error">{displayError}</div>}

        <div className="setup-form-group">
          <label htmlFor="setup-sync-server">Sync Server URL</label>
          <input
            id="setup-sync-server"
            type="text"
            value={syncServer}
            onChange={(e) => setSyncServer(e.target.value)}
            placeholder="wss://sync.automerge.org"
            disabled={isConnecting}
          />
        </div>

        <div className="setup-actions">
          <button
            className="setup-primary-btn"
            onClick={handleCreateNew}
            disabled={isConnecting}
          >
            {isConnecting ? 'Creating...' : 'Create New Project Set'}
          </button>
          <button
            className="setup-secondary-btn"
            onClick={() => setMode('link')}
            disabled={isConnecting}
          >
            Link to Existing Project Set
          </button>
        </div>
      </div>
    </div>
  );
}
