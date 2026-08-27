/**
 * Project Top Bar
 *
 * Project-scoped chrome: share action, project name, switch-project
 * action. Sits to the left of DocumentTopBar in the `.top-bars` row;
 * eventually this bar lives with the rest of the project UI in a
 * left-hand column.
 */

import { SwitchIcon, ShareIcon } from './icons';
import Tooltip from './Tooltip';
import { header } from '../strings';
import './TopBars.css';

interface ProjectTopBarProps {
  projectName: string;
  onChooseNewProject: () => void;
  /** Called when user wants to share the project */
  onShare?: () => void;
}

export default function ProjectTopBar({
  projectName,
  onChooseNewProject,
  onShare,
}: ProjectTopBarProps) {
  return (
    <header className="top-bar project-top-bar">
      <Tooltip content={header.switchProject}>
        <button
          className="qh-icon-btn boxed"
          onClick={onChooseNewProject}
          aria-label={header.switchProject}
        >
          <SwitchIcon />
        </button>
      </Tooltip>
      <span className="project-name">{projectName}</span>
      {onShare && (
        <Tooltip content={header.shareProject}>
          <button
            className="qh-icon-btn boxed header-share-btn"
            onClick={onShare}
            aria-label={header.shareProject}
          >
            <ShareIcon />
          </button>
        </Tooltip>
      )}
    </header>
  );
}
