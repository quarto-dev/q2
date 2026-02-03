/**
 * Minimal Header Component
 *
 * A slim header bar that displays the current file path on the left
 * and project name with navigation on the right.
 */

import './MinimalHeader.css';

interface MinimalHeaderProps {
  currentFilePath: string | null;
  projectName: string;
  onChooseNewProject: () => void;
  /** Called when user wants to share the project */
  onShare?: () => void;
}

export default function MinimalHeader({
  currentFilePath,
  projectName,
  onChooseNewProject,
  onShare,
}: MinimalHeaderProps) {
  return (
    <header className="minimal-header">
      <div className="header-left">
        {currentFilePath ? (
          <span className="file-path">{currentFilePath}</span>
        ) : (
          <span className="file-path empty">No file selected</span>
        )}
      </div>
      <div className="header-right">
        <span className="project-name">{projectName}</span>
        {onShare && (
          <button className="share-btn" onClick={onShare} title="Share this project">
            Share
          </button>
        )}
        <button className="choose-project-btn" onClick={onChooseNewProject}>
          Switch
        </button>
      </div>
    </header>
  );
}
