import { useState, useEffect, useCallback } from 'react';
import type { ProjectEntry } from '../types/project';
import type { UserSettings } from '../services/storage/types';
import * as projectStorage from '../services/projectStorage';
import * as userSettingsService from '../services/userSettings';
import './ProjectSelector.css';

interface Props {
  onSelectProject: (project: ProjectEntry) => void;
  isConnecting?: boolean;
  error?: string | null;
}

// Curated color palette for user selection
const COLOR_PALETTE = [
  '#E91E63', '#9C27B0', '#673AB7', '#3F51B5',
  '#2196F3', '#00BCD4', '#009688', '#4CAF50',
  '#8BC34A', '#FF9800', '#FF5722', '#795548',
];

export default function ProjectSelector({ onSelectProject, isConnecting, error: connectionError }: Props) {
  const [projects, setProjects] = useState<ProjectEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [showAddForm, setShowAddForm] = useState(false);

  // Form state
  const [indexDocId, setIndexDocId] = useState('');
  const [syncServer, setSyncServer] = useState('wss://sync.automerge.org');
  const [description, setDescription] = useState('');
  const [formError, setFormError] = useState<string | null>(null);

  // User identity state
  const [userSettings, setUserSettings] = useState<UserSettings | null>(null);
  const [editingName, setEditingName] = useState(false);
  const [editNameValue, setEditNameValue] = useState('');

  const loadProjects = useCallback(async () => {
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
  }, []);

  const loadUserSettings = useCallback(async () => {
    try {
      const settings = await userSettingsService.getUserIdentity();
      setUserSettings(settings);
    } catch (err) {
      console.error('Failed to load user settings:', err);
    }
  }, []);

  useEffect(() => {
    loadProjects();
    loadUserSettings();
  }, [loadProjects, loadUserSettings]);

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
    } catch (err) {
      console.error('Failed to update color:', err);
    }
  };

  const handleRandomizeName = async () => {
    try {
      // Reset generates a new random name
      const reset = await userSettingsService.resetUserIdentity();
      // But keep the color if user had one selected
      if (userSettings && userSettings.userColor !== reset.userColor) {
        const updated = await userSettingsService.updateUserColor(userSettings.userColor);
        setUserSettings(updated);
      } else {
        setUserSettings(reset);
      }
    } catch (err) {
      console.error('Failed to randomize name:', err);
    }
  };

  const handleSelectProject = async (project: ProjectEntry) => {
    await projectStorage.touchProject(project.id);
    onSelectProject(project);
  };

  const handleAddProject = async (e: React.FormEvent) => {
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

      const project = await projectStorage.addProject(
        normalizedDocId,
        syncServer.trim(),
        description.trim() || undefined
      );
      setIndexDocId('');
      setDescription('');
      setShowAddForm(false);
      await loadProjects();
      onSelectProject(project);
    } catch (err) {
      console.error('Failed to add project:', err);
      setFormError('Failed to add project. The document ID may already exist.');
    }
  };

  const handleDeleteProject = async (e: React.MouseEvent, project: ProjectEntry) => {
    e.stopPropagation();
    if (confirm(`Delete "${project.description}"?`)) {
      await projectStorage.deleteProject(project.id);
      await loadProjects();
    }
  };

  const handleExport = async () => {
    const json = await projectStorage.exportProjects();
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
        <h1>Select a Project</h1>

        {connectionError && <div className="error">{connectionError}</div>}
        {formError && <div className="error">{formError}</div>}
        {isConnecting && <div className="connecting">Connecting to sync server...</div>}

        <div className="projects-list">
          <h2>Your Projects</h2>
          {projects.length === 0 ? (
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
          )}
        </div>

        <div className="divider">
          <span>OR</span>
        </div>

        {showAddForm ? (
          <form className="add-form" onSubmit={handleAddProject}>
            <h2>Connect to Project</h2>
            <p className="form-hint">Enter the document ID of an existing Automerge project</p>
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
              <button type="button" onClick={() => setShowAddForm(false)}>Cancel</button>
              <button type="submit" className="primary">Add Project</button>
            </div>
          </form>
        ) : (
          <button className="add-btn" onClick={() => setShowAddForm(true)}>
            + Connect to Project
          </button>
        )}

        {userSettings && (
          <div className="user-identity">
            <h2>Your Identity</h2>
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
              <button type="button" onClick={handleRandomizeName} className="randomize-btn">
                Randomize Name
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
          </div>
        )}

        <div className="import-export">
          <button onClick={handleImport}>Import from JSON</button>
          <button onClick={handleExport}>Export to JSON</button>
        </div>

        <div className="version-info">
          <span className="commit-hash" title={`Built: ${__BUILD_TIME__}\nCommit date: ${__GIT_COMMIT_DATE__}`}>
            {__GIT_COMMIT_HASH__}
          </span>
        </div>
      </div>
    </div>
  );
}
