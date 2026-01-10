/**
 * Project Tab Component
 *
 * Displays project information:
 * - Project name
 * - Index document ID (click to copy automerge URL)
 * - "Choose New Project" button
 */

import { useState, useCallback } from 'react';
import type { ProjectEntry } from '../../types/project';
import './ProjectTab.css';

interface ProjectTabProps {
  project: ProjectEntry;
  onChooseNewProject: () => void;
}

export default function ProjectTab({ project, onChooseNewProject }: ProjectTabProps) {
  const [copied, setCopied] = useState(false);

  const handleCopyDocId = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(project.indexDocId);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  }, [project.indexDocId]);

  // Display truncated doc ID (remove automerge: prefix, show first 8 chars)
  const truncatedDocId = project.indexDocId.replace(/^automerge:/, '').slice(0, 12) + '...';

  return (
    <div className="project-tab">
      <div className="project-tab-section">
        <label className="section-label">Project Name</label>
        <div className="project-name">{project.description}</div>
      </div>

      <div className="project-tab-section">
        <label className="section-label">Index Document ID</label>
        <button
          className="doc-id-button"
          onClick={handleCopyDocId}
          title={`Click to copy: ${project.indexDocId}`}
        >
          <span className="doc-id-value">{truncatedDocId}</span>
          <span className="copy-indicator">{copied ? 'Copied!' : 'Copy'}</span>
        </button>
      </div>

      <div className="project-tab-section">
        <label className="section-label">Sync Server</label>
        <div className="sync-server">{project.syncServer}</div>
      </div>

      <div className="project-tab-actions">
        <button className="choose-project-btn" onClick={onChooseNewProject}>
          Choose New Project
        </button>
      </div>
    </div>
  );
}
