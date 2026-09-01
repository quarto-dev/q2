/**
 * BranchBar — branch strip above the document editor.
 *
 * Shows the implicit "main" branch plus the file's local-only branches
 * (see services/branchService.ts). Clicking a chip switches the editor to
 * that branch; "Fork" creates a new branch from the currently viewed state;
 * "Merge to main" CRDT-merges the active branch back into the synced doc.
 *
 * Purely presentational — all behavior lives in branchService, wired
 * through useDocBranches in Editor.tsx.
 */

import { useRef, useState } from 'react';
import type { BranchMeta } from '../services/branchService';
import './BranchBar.css';

/** Fork glyph (same style as ProjectsHome's duplicate affordance). */
const forkIcon = (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <circle cx="6" cy="5" r="2.2" stroke="currentColor" strokeWidth="2" />
    <circle cx="18" cy="5" r="2.2" stroke="currentColor" strokeWidth="2" />
    <circle cx="12" cy="19" r="2.2" stroke="currentColor" strokeWidth="2" />
    <path d="M6 7.5v1.5c0 1.7 1.3 3 3 3h6c1.7 0 3-1.3 3-3V7.5M12 12v4.5" stroke="currentColor" strokeWidth="2" />
  </svg>
);

/** Git-compare glyph: two nodes with lines curving toward each other. */
const compareIcon = (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <circle cx="6" cy="5" r="2.2" stroke="currentColor" strokeWidth="2" />
    <circle cx="18" cy="19" r="2.2" stroke="currentColor" strokeWidth="2" />
    <path d="M6 7.5V15c0 2.2 1.8 4 4 4h2.5" stroke="currentColor" strokeWidth="2" />
    <path d="M18 16.5V9c0-2.2-1.8-4-4-4h-2.5" stroke="currentColor" strokeWidth="2" />
  </svg>
);

/** Git-merge glyph: branch line curving into the main line. */
const mergeIcon = (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <circle cx="6" cy="5" r="2.2" stroke="currentColor" strokeWidth="2" />
    <circle cx="6" cy="19" r="2.2" stroke="currentColor" strokeWidth="2" />
    <circle cx="18" cy="12" r="2.2" stroke="currentColor" strokeWidth="2" />
    <path d="M6 7.5v9M6 8c0 4 4 4 9.5 4" stroke="currentColor" strokeWidth="2" />
  </svg>
);

interface BranchBarProps {
  branches: BranchMeta[];
  /** Active branch id, or null when on main. */
  activeBranchId: string | null;
  /** Disable all controls (e.g. during replay mode). */
  disabled?: boolean;
  /** Whether the diff view (branch vs main) is showing. */
  comparing?: boolean;
  onSwitch: (branchId: string | null) => void;
  onFork: (name: string) => void;
  onMerge: (branchId: string) => void;
  onDelete: (branchId: string) => void;
  onToggleCompare?: () => void;
}

export default function BranchBar({
  branches,
  activeBranchId,
  disabled = false,
  comparing = false,
  onSwitch,
  onFork,
  onMerge,
  onDelete,
  onToggleCompare,
}: BranchBarProps) {
  const [naming, setNaming] = useState(false);
  const [draftName, setDraftName] = useState('');
  const inputRef = useRef<HTMLInputElement | null>(null);

  const startNaming = () => {
    if (disabled) return;
    setDraftName('');
    setNaming(true);
  };

  const confirmFork = () => {
    setNaming(false);
    onFork(draftName.trim());
  };

  const handleNameKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      confirmFork();
    } else if (e.key === 'Escape') {
      setNaming(false);
    }
  };

  return (
    <div className="branch-bar" role="toolbar" aria-label="Document branches">
      <span className="branch-bar-icon" aria-hidden="true">⑂</span>
      <div className="branch-bar-chips">
        <button
          className={`branch-chip${activeBranchId === null ? ' active' : ''}`}
          onMouseDown={() => onSwitch(null)}
          disabled={disabled}
          title="Switch to the shared main document"
        >
          main
        </button>
        {branches.map((branch) => (
          <button
            key={branch.id}
            className={`branch-chip${activeBranchId === branch.id ? ' active' : ''}`}
            onMouseDown={() => onSwitch(branch.id)}
            disabled={disabled}
            title="Local branch — not shared with collaborators"
          >
            {branch.name}
            <span
              className="branch-chip-delete"
              role="button"
              aria-label={`Delete branch ${branch.name}`}
              title={`Delete branch ${branch.name}`}
              onClick={(e) => {
                e.stopPropagation();
                if (!disabled) onDelete(branch.id);
              }}
            >
              ×
            </span>
          </button>
        ))}
      </div>
      <div className="branch-bar-actions">
        {naming ? (
          <input
            ref={inputRef}
            className="branch-name-input"
            placeholder="branch name"
            value={draftName}
            autoFocus
            onChange={(e) => setDraftName(e.target.value)}
            onKeyDown={handleNameKeyDown}
            onBlur={() => setNaming(false)}
          />
        ) : (
          <button className="branch-action-btn" onClick={startNaming} disabled={disabled}>
            {forkIcon} Fork
          </button>
        )}
        {activeBranchId !== null && onToggleCompare && (
          <button
            className={`branch-action-btn${comparing ? ' comparing' : ''}`}
            onClick={onToggleCompare}
            disabled={disabled}
            title="Toggle a diff between main and this branch"
          >
            {compareIcon} Compare with main
          </button>
        )}
        {activeBranchId !== null && (
          <button
            className="branch-action-btn branch-merge-btn"
            onClick={() => onMerge(activeBranchId)}
            disabled={disabled}
            title="Merge this branch into the shared main document and delete it"
          >
            {mergeIcon} Merge to main
          </button>
        )}
      </div>
    </div>
  );
}
