import { useState, useEffect, useCallback } from 'react';
import type { ProjectEntry } from '../types/project';
import * as projectStorage from '../services/projectStorage';
import './ProjectSelector.css';

interface Props {
  onSelectProject: (project: ProjectEntry) => void;
  isConnecting?: boolean;
  error?: string | null;
}

export default function ProjectSelector({ onSelectProject, isConnecting, error: connectionError }: Props) {
  const [projects, setProjects] = useState<ProjectEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [showAddForm, setShowAddForm] = useState(false);

  // Form state
  const [indexDocId, setIndexDocId] = useState('');
  const [syncServer, setSyncServer] = useState('wss://sync.automerge.org');
  const [description, setDescription] = useState('');
  const [formError, setFormError] = useState<string | null>(null);

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

  useEffect(() => {
    loadProjects();
  }, [loadProjects]);

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
            <h2>Add New Project</h2>
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
            + Add New Project
          </button>
        )}

        <div className="import-export">
          <button onClick={handleImport}>Import from JSON</button>
          <button onClick={handleExport}>Export to JSON</button>
        </div>
      </div>
    </div>
  );
}
