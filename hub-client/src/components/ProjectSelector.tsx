import { useState, useEffect, useCallback } from 'react';
import { useTheme } from './ThemeContext';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';
import type { ProjectSetEntry } from '@quarto/quarto-automerge-schema';
import type { ProjectSetStatus } from '../hooks/useProjectSet';
import type { UserSettings } from '../services/storage/types';
import * as projectStorage from '../services/projectStorage';
import * as userSettingsService from '../services/userSettings';
import {
  getProjectChoices,
  createProject as wasmCreateProject,
  importProjectFromZip,
  type ProjectChoice,
  type ProjectFile,
} from '@quarto/preview-runtime';
import { DEFAULT_SYNC_SERVER, buildProjectSetLinkUrl } from '../utils/routing';
import ShareDialog from './ShareDialog';
import './ProjectSelector.css';

interface Props {
  /** Called when a project is selected, with optional file path override from share link */
  onSelectProject: (project: ProjectEntry, filePathOverride?: string) => void;
  isConnecting?: boolean;
  error?: string | null;
  /** Called when a new project is created with scaffold files */
  onProjectCreated?: (files: ProjectFile[], title: string, projectType: string, syncServer: string) => void;
  /** Whether the app is connected to a hub (has a signed-in session). */
  isHubConnected?: boolean;
  /** Called when the user chooses to connect to a hub (triggers sign-in). */
  onConnectToHub?: () => void;
  /** Called when user signs out. Passed only when signed in to a hub. */
  onSignOut?: () => void;
  /** Authenticated user's email (for display). */
  authEmail?: string;
  /** Authenticated user's Google avatar URL. */
  authPicture?: string | null;
  /** Called when the user changes their screen name. */
  onScreenNameChange?: (name: string) => void;
  /** Called when the user changes their cursor color. */
  onColorChange?: (color: string) => void;
  /** Authenticated user's OIDC display name (for screen name reset). */
  authName?: string | null;
  /** The document ID of the connected project set (for "Link Another Browser" UI). */
  projectSetDocId?: string | null;
  /** The sync server URL for the project set. */
  projectSetSyncServer?: string | null;
  /** Current status of the project set connection. */
  projectSetStatus?: ProjectSetStatus;
  /** Projects from the synced project set (if connected). */
  projectSetEntries?: ProjectSetEntry[];
  /** Remove a project from the synced set. */
  onRemoveProjectFromSet?: (indexDocId: string) => void;
  /** Touch a project in the synced set (update lastAccessed). */
  onTouchProject?: (indexDocId: string) => void;
  /** Add a project to the synced set (used by the Connect form). */
  onAddProjectToSet?: (entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>) => void;
}

// Curated color palette for user selection (10 colors, single row)
const COLOR_PALETTE = [
  '#E91E63', '#9C27B0', '#3F51B5', '#2196F3',
  '#00BCD4', '#009688', '#4CAF50', '#FF9800',
  '#FF5722', '#795548',
];

export default function ProjectSelector({
  onSelectProject,
  isConnecting,
  error: connectionError,
  onProjectCreated,
  isHubConnected,
  onConnectToHub,
  onSignOut,
  authEmail,
  authPicture,
  onScreenNameChange,
  onColorChange,
  authName,
  projectSetDocId,
  projectSetSyncServer,
  projectSetStatus,
  projectSetEntries,
  onRemoveProjectFromSet,
  onTouchProject,
  onAddProjectToSet,
}: Props) {
  const [projects, setProjects] = useState<ProjectEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [showConnectForm, setShowConnectForm] = useState(false);
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [showImportForm, setShowImportForm] = useState(false);

  // Connect form state
  const [indexDocId, setIndexDocId] = useState('');
  const [syncServer, setSyncServer] = useState(DEFAULT_SYNC_SERVER);
  const [description, setDescription] = useState('');
  const [formError, setFormError] = useState<string | null>(null);

  // Create form state
  const [createProjectType, setCreateProjectType] = useState('');
  const [createProjectTitle, setCreateProjectTitle] = useState('');
  const [isCreating, setIsCreating] = useState(false);
  const [projectChoices, setProjectChoices] = useState<ProjectChoice[]>([]);
  const [loadingChoices, setLoadingChoices] = useState(false);

  // Sync Server URL field shared by Create + Import (new-project forms, as
  // opposed to Connect's join-an-existing-project form above). Editable, but
  // must default to empty when not connected to a hub: a non-empty default
  // silently turns local creation into a hub-creation attempt with no
  // session, which createNewProject's resolveActorId callback doesn't abort
  // on (client.ts) — the project gets created and wired to a real WS
  // adapter, then immediately torn down by the auth-loss-teardown effect.
  // Reset on each form open so it tracks projectSetSyncServer if it changes
  // mid-session (e.g. the user connects to a hub between opens).
  const [newProjectSyncServer, setNewProjectSyncServer] = useState(() => projectSetSyncServer ?? '');

  // Import-from-ZIP form state
  const [importTitle, setImportTitle] = useState('');
  const [importFile, setImportFile] = useState<File | null>(null);
  const [isImporting, setIsImporting] = useState(false);

  // User identity state
  const [userSettings, setUserSettings] = useState<UserSettings | null>(null);
  const [editingName, setEditingName] = useState(false);
  const [editNameValue, setEditNameValue] = useState('');

  // Theme
  const { colorScheme, cycleColorScheme } = useTheme();

  // "Link Another Browser" dialog state
  const [showLinkDialog, setShowLinkDialog] = useState(false);

  const projectSetLinkUrl = projectSetDocId && projectSetSyncServer
    ? buildProjectSetLinkUrl(projectSetDocId, projectSetSyncServer)
    : undefined;

  // Identity section collapsed state (persisted to localStorage)
  const [identityCollapsed, setIdentityCollapsed] = useState(() => {
    const saved = localStorage.getItem('qh-identity-collapsed');
    return saved === 'true';
  });

  const toggleIdentityCollapsed = () => {
    setIdentityCollapsed(prev => {
      const newValue = !prev;
      localStorage.setItem('qh-identity-collapsed', String(newValue));
      return newValue;
    });
  };

  // When using project set, derive projects from set entries.
  // Also true when the project set is still loading/connecting (entries not yet available).
  const projectSetConnecting = projectSetStatus === 'loading' || projectSetStatus === 'connecting';
  const useProjectSet = !!projectSetEntries || projectSetConnecting;

  const loadProjects = useCallback(async () => {
    if (useProjectSet) {
      // Projects come from the project set (or it's still connecting) — no need to load from IDB
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const entries = await projectStorage.listProjects();
      setProjects(entries);
    } catch (err) {
      console.error('Failed to load projects:', err);
      setFormError('Failed to load projects');
    } finally {
      setLoading(false);
    }
  }, [useProjectSet]);

  const loadUserSettings = useCallback(async () => {
    try {
      const settings = await userSettingsService.getUserIdentity();
      setUserSettings(settings);
    } catch (err) {
      console.error('Failed to load user settings:', err);
    }
  }, []);

  const loadProjectChoices = useCallback(async () => {
    setLoadingChoices(true);
    try {
      const choices = await getProjectChoices();
      setProjectChoices(choices);
      // Set default selection to first choice
      if (choices.length > 0 && !createProjectType) {
        setCreateProjectType(choices[0].id);
      }
    } catch (err) {
      console.error('Failed to load project choices:', err);
      // Fall back to showing an error - choices are required for creation
    } finally {
      setLoadingChoices(false);
    }
  }, [createProjectType]);

  useEffect(() => {
    loadProjects();
    loadUserSettings();
    loadProjectChoices();
  }, [loadProjects, loadUserSettings, loadProjectChoices]);


  const handleStartEditName = () => {
    if (userSettings) {
      setEditNameValue(userSettings.userName);
      setEditingName(true);
    }
  };

  const handleSaveName = async () => {
    if (!editNameValue.trim()) {
      return;
    }
    try {
      const updated = await userSettingsService.updateUserName(editNameValue.trim());
      setUserSettings(updated);
      setEditingName(false);
      onScreenNameChange?.(updated.userName);
    } catch (err) {
      console.error('Failed to update name:', err);
    }
  };

  const handleCancelEditName = () => {
    setEditingName(false);
    setEditNameValue('');
  };

  const handleColorChange = async (color: string) => {
    try {
      const updated = await userSettingsService.updateUserColor(color);
      setUserSettings(updated);
      onColorChange?.(updated.userColor);
    } catch (err) {
      console.error('Failed to update color:', err);
    }
  };

  const handleResetName = async () => {
    if (!authName) return;
    try {
      const updated = await userSettingsService.updateUserName(authName);
      setUserSettings(updated);
      onScreenNameChange?.(updated.userName);
    } catch (err) {
      console.error('Failed to reset name:', err);
    }
  };

  const handleRandomizeName = async () => {
    try {
      const reset = await userSettingsService.resetUserIdentity();
      // Keep the color if user had one selected
      if (userSettings && userSettings.userColor !== reset.userColor) {
        const updated = await userSettingsService.updateUserColor(userSettings.userColor);
        setUserSettings(updated);
        onScreenNameChange?.(updated.userName);
      } else {
        setUserSettings(reset);
        onScreenNameChange?.(reset.userName);
      }
    } catch (err) {
      console.error('Failed to randomize name:', err);
    }
  };

  const handleSelectProject = async (project: ProjectEntry) => {
    await projectStorage.touchProject(project.id);
    onTouchProject?.(project.indexDocId);
    onSelectProject(project);
  };

  const handleSelectProjectFromSet = async (entry: ProjectSetEntry) => {
    // Ensure a local IDB entry exists (needed for URL routing with local IDs)
    let localProject = await projectStorage.getProjectByIndexDocId(entry.indexDocId);
    if (!localProject) {
      localProject = await projectStorage.addProject(
        // A local-only project set entry has no syncServer; the empty
        // string is the local sentinel in the IDB layer (falsy → the
        // sync client's storage-only path).
        entry.indexDocId,
        entry.syncServer ?? '',
        entry.description,
      );
    }
    await projectStorage.touchProject(localProject.id);
    onTouchProject?.(entry.indexDocId);
    onSelectProject(localProject);
  };

  const handleConnectProject = async (e: React.FormEvent) => {
    e.preventDefault();
    setFormError(null);

    if (!indexDocId.trim()) {
      setFormError('Index Document ID is required');
      return;
    }
    if (!syncServer.trim()) {
      setFormError('Sync Server URL is required');
      return;
    }

    try {
      // Ensure the document ID has the automerge: prefix
      let normalizedDocId = indexDocId.trim();
      if (!normalizedDocId.startsWith('automerge:')) {
        normalizedDocId = `automerge:${normalizedDocId}`;
      }
      const syncServerValue = syncServer.trim();
      const descriptionValue = description.trim() || undefined;

      // Reuse any existing IDB entry so repeatedly connecting to the same
      // doc is idempotent (and doesn't collide with the unique index on
      // indexDocId). Happens any time a share-link visit already wrote to
      // IDB and the user then opens the Connect form with the same ID.
      let project = await projectStorage.getProjectByIndexDocId(normalizedDocId);
      if (!project) {
        project = await projectStorage.addProject(
          normalizedDocId,
          syncServerValue,
          descriptionValue,
        );
      }

      // Also add to the synced project set. In project-set mode the landing
      // page renders its list from projectSetEntries, not IDB, so without
      // this the project would be invisible even after a successful write.
      // Safe to call even if already present — addProjectToSet dedupes.
      if (useProjectSet && onAddProjectToSet) {
        try {
          onAddProjectToSet({
            indexDocId: normalizedDocId,
            syncServer: syncServerValue,
            description: descriptionValue ?? project.description,
          });
        } catch (err) {
          // Non-fatal — the reconciler will sweep this up on the next
          // `connected` transition.
          console.warn('Failed to add to synced project set:', err);
        }
      }

      setIndexDocId('');
      setDescription('');
      setShowConnectForm(false);
      await loadProjects();

      onSelectProject(project);
    } catch (err) {
      console.error('Failed to add project:', err);
      setFormError(
        err instanceof Error ? `Failed to add project: ${err.message}` : 'Failed to add project.',
      );
    }
  };

  const handleCancelConnect = () => {
    setShowConnectForm(false);
    setIndexDocId('');
  };

  const handleCreateProject = async (e: React.FormEvent) => {
    e.preventDefault();
    setFormError(null);

    if (!createProjectTitle.trim()) {
      setFormError('Project title is required');
      return;
    }

    if (!createProjectType) {
      setFormError('Please select a project type');
      return;
    }

    // Empty targets local-only creation; otherwise a hub sync server.
    const createTargetServer = newProjectSyncServer.trim();

    setIsCreating(true);

    try {
      console.log('Creating project:', { type: createProjectType, title: createProjectTitle });

      const result = await wasmCreateProject(createProjectType, createProjectTitle.trim());

      if (!result.success) {
        setFormError(result.error || 'Failed to create project');
        return;
      }

      if (!result.files || result.files.length === 0) {
        setFormError('No files were generated for this project type');
        return;
      }

      console.log('Project scaffold created:', result.files.map(f => f.path));

      // Call the callback with the scaffold files
      // The parent component (or k-tsqm task) will handle Automerge document creation
      if (onProjectCreated) {
        onProjectCreated(result.files, createProjectTitle.trim(), createProjectType, createTargetServer);
      } else {
        // If no callback, show success message with file list
        const fileList = result.files.map(f => f.path).join(', ');
        setFormError(`Project scaffold created! Files: ${fileList}\n\n(Automerge integration pending - k-tsqm)`);
      }

      // Reset form on success (only if callback handled it)
      if (onProjectCreated) {
        setCreateProjectTitle('');
        setShowCreateForm(false);
      }
    } catch (err) {
      console.error('Failed to create project:', err);
      setFormError(err instanceof Error ? err.message : 'Failed to create project');
    } finally {
      setIsCreating(false);
    }
  };

  const handleImportFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setFormError(null);
    const file = e.target.files?.[0] ?? null;
    setImportFile(file);
    // Prefill the title from the ZIP filename (minus extension) if the
    // user hasn't already typed one. Stays editable afterward.
    if (file && !importTitle.trim()) {
      setImportTitle(file.name.replace(/\.zip$/i, ''));
    }
  };

  const handleCancelImport = () => {
    setShowImportForm(false);
    setImportFile(null);
    setImportTitle('');
    setFormError(null);
  };

  const handleImportZip = async (e: React.FormEvent) => {
    e.preventDefault();
    setFormError(null);

    if (!importFile) {
      setFormError('Please choose a ZIP file to import');
      return;
    }

    if (!importTitle.trim()) {
      setFormError('Project title is required');
      return;
    }

    // Empty targets local-only creation; otherwise a hub sync server.
    const importTargetServer = newProjectSyncServer.trim();

    setIsImporting(true);

    try {
      const bytes = new Uint8Array(await importFile.arrayBuffer());
      const files = importProjectFromZip(bytes);

      if (files.length === 0) {
        setFormError('The archive contains no usable files');
        return;
      }

      if (onProjectCreated) {
        onProjectCreated(files, importTitle.trim(), 'imported', importTargetServer);
        // Reset the form; the parent handles navigation into the project.
        setShowImportForm(false);
        setImportFile(null);
        setImportTitle('');
      } else {
        setFormError(`Imported ${files.length} file(s), but no handler is wired to create the project.`);
      }
    } catch (err) {
      console.error('Failed to import project from ZIP:', err);
      setFormError(
        err instanceof Error ? `Failed to import ZIP: ${err.message}` : 'Failed to import ZIP.',
      );
    } finally {
      setIsImporting(false);
    }
  };

  const handleDeleteProject = async (e: React.MouseEvent, project: ProjectEntry) => {
    e.stopPropagation();
    if (confirm(`Delete "${project.description}"?`)) {
      await projectStorage.deleteProject(project.id);
      onRemoveProjectFromSet?.(project.indexDocId);
      await loadProjects();
    }
  };

  const handleDeleteProjectFromSet = (e: React.MouseEvent, entry: ProjectSetEntry) => {
    e.stopPropagation();
    if (confirm(`Delete "${entry.description}"?`)) {
      onRemoveProjectFromSet?.(entry.indexDocId);
    }
  };

  const handleExport = async () => {
    // When using project set, export from the set; otherwise from IDB
    let json: string;
    if (useProjectSet && projectSetEntries) {
      const exportData = {
        schemaVersion: 4,
        exportedAt: new Date().toISOString(),
        projects: projectSetEntries.map((e) => ({
          id: '', // Not meaningful for set entries
          indexDocId: e.indexDocId,
          syncServer: e.syncServer,
          description: e.description,
          createdAt: e.addedAt,
          lastAccessed: e.lastAccessed,
        })),
      };
      json = JSON.stringify(exportData, null, 2);
    } else {
      json = await projectStorage.exportProjects();
    }
    const blob = new Blob([json], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'quarto-hub-projects.json';
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleImport = async () => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'application/json';
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (file) {
        const text = await file.text();
        try {
          const count = await projectStorage.importProjects(text);
          alert(`Imported ${count} project(s)`);
          await loadProjects();
        } catch (err) {
          console.error('Failed to import:', err);
          setFormError('Failed to import projects. Invalid JSON format.');
        }
      }
    };
    input.click();
  };

  if (loading) {
    return <div className="project-selector"><div className="loading">Loading projects...</div></div>;
  }

  return (
    <div className="project-selector">
      <div className="modal">
        <div className="modal-header">
          <div className="header-text">
            <h1>Quarto Hub</h1>
            <p className="tagline">Multiplayer editing for your Quarto projects</p>
          </div>
          <div className="header-actions">
            {/*
              Account-level hub connection control (bd-u4p8xhdc). When not
              connected, offer "Connect to a hub" (triggers sign-in). When
              connected, show the signed-in identity + Sign out. This is
              session-scoped state, kept out of the per-project action row so
              it does not collide with "Connect to Project" (join-by-doc-id).
            */}
            {isHubConnected ? (
              onSignOut && (
                <button
                  className="sign-out-btn"
                  onClick={onSignOut}
                  title={authEmail ? `Signed in as ${authEmail}` : 'Sign out'}
                >
                  {authPicture && (
                    <img src={authPicture} alt="" className="auth-avatar" referrerPolicy="no-referrer" />
                  )}
                  <span>{authEmail ? `Signed in as ${authEmail}` : 'Signed in'} · Sign out</span>
                </button>
              )
            ) : (
              onConnectToHub && (
                <button
                  className="sign-out-btn"
                  onClick={onConnectToHub}
                  title="Connect to a hub to sync and collaborate"
                >
                  <span>Connect to a hub</span>
                </button>
              )
            )}
            <button
              className="theme-toggle"
              onClick={cycleColorScheme}
              title={colorScheme === 'auto' ? 'Theme: Follow system' : colorScheme === 'dark' ? 'Theme: Dark' : 'Theme: Light'}
            >
              {colorScheme === 'auto' ? '💻' : colorScheme === 'dark' ? '🌙' : '☀️'}
            </button>
          </div>
        </div>

        {connectionError && <div className="error">{connectionError}</div>}
        {formError && <div className="error">{formError}</div>}
        {isConnecting && <div className="connecting">Connecting to sync server...</div>}

        <div className="projects-list">
          <h2>Your Projects</h2>
          {useProjectSet ? (
            // Render from synced project set
            projectSetConnecting ? (
              <div className="connecting">Connecting to project set...</div>
            ) : projectSetEntries!.length === 0 ? (
              <p className="empty">No projects yet. Add one below.</p>
            ) : (
              <ul>
                {projectSetEntries!.map((entry) => (
                  <li key={entry.indexDocId} onClick={() => handleSelectProjectFromSet(entry)}>
                    <div className="project-info">
                      <span className="project-name">{entry.description}</span>
                      <span className="project-meta">
                        <span className="project-server">{entry.syncServer}</span>
                        <span className="project-docid" title={entry.indexDocId}>
                          {entry.indexDocId.replace(/^automerge:/, '').slice(0, 8)}...
                        </span>
                      </span>
                    </div>
                    <button
                      className="delete-btn"
                      onClick={(e) => handleDeleteProjectFromSet(e, entry)}
                      title="Delete project"
                    >
                      &times;
                    </button>
                  </li>
                ))}
              </ul>
            )
          ) : (
            // Render from local IDB (legacy fallback)
            projects.length === 0 ? (
              <p className="empty">No projects yet. Add one below.</p>
            ) : (
              <ul>
                {projects.map((project) => (
                  <li key={project.id} onClick={() => handleSelectProject(project)}>
                    <div className="project-info">
                      <span className="project-name">{project.description}</span>
                      <span className="project-meta">
                        <span className="project-server">{project.syncServer}</span>
                        <span className="project-docid" title={project.indexDocId}>
                          {project.indexDocId.replace(/^automerge:/, '').slice(0, 8)}...
                        </span>
                      </span>
                    </div>
                    <button
                      className="delete-btn"
                      onClick={(e) => handleDeleteProject(e, project)}
                      title="Delete project"
                    >
                      &times;
                    </button>
                  </li>
                ))}
              </ul>
            )
          )}
        </div>

        {/* Hide action buttons and forms while project set is connecting */}
        {!projectSetConnecting && <>
        <div className="divider">
          <span>OR</span>
        </div>

        {/* Show buttons when no form is visible */}
        {!showConnectForm && !showCreateForm && !showImportForm && (
          <div className="action-buttons">
            <button
              className="action-btn create-btn"
              onClick={() => { setNewProjectSyncServer(projectSetSyncServer ?? ''); setShowCreateForm(true); setShowConnectForm(false); setShowImportForm(false); }}
            >
              <span className="action-btn-text">
                <span className="action-btn-title">
                  <span className="action-btn-icon">+</span>
                  Create New Project
                </span>
                <span className="action-btn-hint">Start a new Quarto project</span>
              </span>
            </button>
            <button
              className="action-btn connect-btn"
              onClick={() => { setShowConnectForm(true); setShowCreateForm(false); setShowImportForm(false); }}
            >
              <span className="action-btn-text">
                <span className="action-btn-title">
                  <span className="action-btn-icon">↗</span>
                  Connect to Project
                </span>
                <span className="action-btn-hint">Join an existing QH project</span>
              </span>
            </button>
            <button
              className="action-btn import-btn"
              onClick={() => { setNewProjectSyncServer(projectSetSyncServer ?? ''); setShowImportForm(true); setShowCreateForm(false); setShowConnectForm(false); }}
            >
              <span className="action-btn-text">
                <span className="action-btn-title">
                  <span className="action-btn-icon">⬆</span>
                  Import from ZIP
                </span>
                <span className="action-btn-hint">Create a project from a .zip archive</span>
              </span>
            </button>
          </div>
        )}

        {/* Create New Project form */}
        {showCreateForm && (
          <form className="add-form create-form" onSubmit={handleCreateProject}>
            <h2>Create New Project</h2>
            <p className="form-hint">Create a new Quarto project with starter files</p>
            <div className="form-group">
              <label htmlFor="projectType">Project Type</label>
              {loadingChoices ? (
                <div className="select-loading">Loading project types...</div>
              ) : projectChoices.length === 0 ? (
                <div className="select-error">Failed to load project types. Please refresh.</div>
              ) : (
                <select
                  id="projectType"
                  value={createProjectType}
                  onChange={(e) => setCreateProjectType(e.target.value)}
                >
                  {projectChoices.map((choice) => (
                    <option key={choice.id} value={choice.id}>
                      {choice.name} — {choice.description}
                    </option>
                  ))}
                </select>
              )}
            </div>
            <div className="form-group">
              <label htmlFor="projectTitle">Project Title</label>
              <input
                id="projectTitle"
                type="text"
                value={createProjectTitle}
                onChange={(e) => setCreateProjectTitle(e.target.value)}
                placeholder="My Awesome Project"
                autoFocus
              />
            </div>
            <div className="form-group">
              <label htmlFor="createSyncServer">Sync Server URL</label>
              <input
                id="createSyncServer"
                type="text"
                value={newProjectSyncServer}
                onChange={(e) => setNewProjectSyncServer(e.target.value)}
                placeholder="wss://sync.automerge.org"
              />
            </div>
            <div className="form-actions">
              <button type="button" onClick={() => setShowCreateForm(false)}>Cancel</button>
              <button
                type="submit"
                className="primary"
                disabled={isCreating || loadingChoices || projectChoices.length === 0}
              >
                {isCreating ? 'Creating...' : 'Create Project'}
              </button>
            </div>
          </form>
        )}

        {/* Import from ZIP form */}
        {showImportForm && (
          <form className="add-form import-form" onSubmit={handleImportZip}>
            <h2>Import from ZIP</h2>
            <p className="form-hint">Create a new project from the contents of a .zip archive</p>
            <div className="form-group">
              <label htmlFor="importZipFile">ZIP File</label>
              <input
                id="importZipFile"
                type="file"
                accept=".zip,application/zip"
                onChange={handleImportFileChange}
              />
            </div>
            <div className="form-group">
              <label htmlFor="importTitle">Project Title</label>
              <input
                id="importTitle"
                type="text"
                value={importTitle}
                onChange={(e) => setImportTitle(e.target.value)}
                placeholder="My Imported Project"
              />
            </div>
            <div className="form-group">
              <label htmlFor="importSyncServer">Sync Server URL</label>
              <input
                id="importSyncServer"
                type="text"
                value={newProjectSyncServer}
                onChange={(e) => setNewProjectSyncServer(e.target.value)}
                placeholder="wss://sync.automerge.org"
              />
            </div>
            <div className="form-actions">
              <button type="button" onClick={handleCancelImport}>Cancel</button>
              <button
                type="submit"
                className="primary"
                disabled={isImporting || !importFile}
              >
                {isImporting ? 'Importing...' : 'Import Project'}
              </button>
            </div>
          </form>
        )}

        {/* Connect to Project form */}
        {showConnectForm && (
          <form className="add-form" onSubmit={handleConnectProject}>
            <h2>Connect to Project</h2>
            <p className="form-hint">
              Enter the document ID of an existing Automerge project
            </p>
            <div className="form-group">
              <label htmlFor="indexDocId">Index Document ID</label>
              <input
                id="indexDocId"
                type="text"
                value={indexDocId}
                onChange={(e) => setIndexDocId(e.target.value)}
                placeholder="bs58-encoded document ID"
              />
            </div>
            <div className="form-group">
              <label htmlFor="syncServer">Sync Server URL</label>
              <input
                id="syncServer"
                type="text"
                value={syncServer}
                onChange={(e) => setSyncServer(e.target.value)}
                placeholder="wss://sync.automerge.org"
              />
            </div>
            <div className="form-group">
              <label htmlFor="description">Description (optional)</label>
              <input
                id="description"
                type="text"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="My Project"
              />
            </div>
            <div className="form-actions">
              <button type="button" onClick={handleCancelConnect}>Cancel</button>
              <button type="submit" className="primary">Connect</button>
            </div>
          </form>
        )}
        </>}

        {userSettings && (
          <div className={`user-identity ${identityCollapsed ? 'collapsed' : ''}`}>
            <div className="section-header-row">
              <h2>Your Identity</h2>
              {identityCollapsed && (
                <span
                  className="collapsed-name"
                  style={{ color: userSettings.userColor }}
                >
                  {userSettings.userName}
                </span>
              )}
              <button
                className="collapse-toggle"
                onClick={toggleIdentityCollapsed}
                title={identityCollapsed ? 'Expand' : 'Collapse'}
              >
                {identityCollapsed ? '▸' : '▾'}
              </button>
            </div>

            {!identityCollapsed && (
              <>
            <p className="identity-hint">This is how others see you during collaboration</p>

            <div className="identity-preview">
              <span
                className="identity-color-dot"
                style={{ backgroundColor: userSettings.userColor }}
              />
              {editingName ? (
                <div className="identity-name-edit">
                  <input
                    type="text"
                    value={editNameValue}
                    onChange={(e) => setEditNameValue(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') handleSaveName();
                      if (e.key === 'Escape') handleCancelEditName();
                    }}
                    autoFocus
                  />
                  <button type="button" onClick={handleSaveName} className="save-btn">
                    Save
                  </button>
                  <button type="button" onClick={handleCancelEditName} className="cancel-btn">
                    Cancel
                  </button>
                </div>
              ) : (
                <span className="identity-name" onClick={handleStartEditName}>
                  {userSettings.userName}
                  <span className="edit-hint">(click to edit)</span>
                </span>
              )}
            </div>

            <div className="identity-actions">
              {authName && (
                <button type="button" onClick={handleResetName} className="randomize-btn">
                  Reset
                </button>
              )}
              <button type="button" onClick={handleRandomizeName} className="randomize-btn">
                Randomize
              </button>
            </div>

            <div className="color-picker">
              <label>Cursor Color</label>
              <div className="color-swatches">
                {COLOR_PALETTE.map((color) => (
                  <button
                    key={color}
                    type="button"
                    className={`color-swatch ${userSettings.userColor === color ? 'selected' : ''}`}
                    style={{ backgroundColor: color }}
                    onClick={() => handleColorChange(color)}
                    title={color}
                  />
                ))}
              </div>
            </div>
              </>
            )}
          </div>
        )}

        {projectSetDocId && (
          <div className="project-set-info">
            <div className="project-set-header">
              <h2>Project Set</h2>
              <span className="project-set-id" title={projectSetDocId}>
                {projectSetDocId.replace(/^automerge:/, '').slice(0, 12)}...
              </span>
            </div>
            {projectSetLinkUrl && (
              <button
                className="link-browser-btn"
                onClick={() => setShowLinkDialog(true)}
              >
                Link Another Browser
              </button>
            )}
          </div>
        )}

        <div className="import-export">
          <button onClick={handleImport}>Import from JSON</button>
          <button onClick={handleExport}>Export to JSON</button>
        </div>

        <ShareDialog
          isOpen={showLinkDialog}
          shareableUrl={projectSetLinkUrl}
          onClose={() => setShowLinkDialog(false)}
        />

        <div className="version-info">
          <span className="commit-hash" title={`Built: ${__BUILD_TIME__}\nCommit date: ${__GIT_COMMIT_DATE__}`}>
            {__GIT_COMMIT_HASH__}
          </span>
        </div>
      </div>
    </div>
  );
}
