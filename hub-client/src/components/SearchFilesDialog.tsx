/**
 * SearchFilesDialog — full-text search over the project, in a modal.
 * Type to search (debounced); ArrowUp/ArrowDown pick a result, Enter
 * opens it, Escape closes (owned by ModalDialog). Clicking a result opens
 * it too.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import ModalDialog from './ModalDialog';
import { getFileIcon } from './fileTreeRowHelpers';
import {
  buildSnippet,
  findFirstMatch,
  type MatchRange,
  type SearchFiles,
  type SearchResult,
} from '../services/search';
import { dialogs, fileSidebar } from '../strings';
import './SearchFilesDialog.css';

export interface SearchFilesDialogProps {
  isOpen: boolean;
  files: FileEntry[];
  searchFiles: SearchFiles;
  /** Live text per path, for match snippets. Optional. */
  fileContents?: Map<string, string>;
  onClose: () => void;
  /** Open `file`; `match` is the first matched term's range, when known. */
  onSelectFile: (file: FileEntry, match: MatchRange | null) => void;
}

export default function SearchFilesDialog({ isOpen, ...rest }: SearchFilesDialogProps) {
  // Mount fresh per open: the query and results start empty each time.
  if (!isOpen) return null;
  return <SearchFilesForm {...rest} />;
}

function SearchFilesForm({
  files,
  searchFiles,
  fileContents,
  onClose,
  onSelectFile,
}: Omit<SearchFilesDialogProps, 'isOpen'>) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchResult[]>([]);
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const filesByPath = useMemo(() => new Map(files.map((f) => [f.path, f])), [files]);
  const isSearching = query.trim() !== '';

  useEffect(() => {
    const t = setTimeout(() => inputRef.current?.focus(), 50);
    return () => clearTimeout(t);
  }, []);

  // Debounced search; ignore stale resolutions. State updates happen in
  // the timer callback, never synchronously in the effect body.
  useEffect(() => {
    let cancelled = false;
    const handle = setTimeout(
      () => {
        if (!isSearching) {
          if (!cancelled) setResults([]);
          return;
        }
        void searchFiles(query, { limit: 50 }).then((r) => {
          if (!cancelled) {
            setResults(r);
            setActiveIndex(0);
          }
        });
      },
      isSearching ? 120 : 0
    );
    return () => {
      cancelled = true;
      clearTimeout(handle);
    };
  }, [searchFiles, query, isSearching]);

  const open = useCallback(
    (result: SearchResult) => {
      const file = filesByPath.get(result.path);
      if (!file) return;
      const content = fileContents?.get(result.path);
      onSelectFile(file, content ? findFirstMatch(content, result.terms) : null);
      onClose();
    },
    [filesByPath, fileContents, onSelectFile, onClose]
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        if (results.length === 0) return;
        e.preventDefault();
        const delta = e.key === 'ArrowDown' ? 1 : -1;
        const next = (activeIndex + delta + results.length) % results.length;
        setActiveIndex(next);
        listRef.current
          ?.querySelector(`[data-index="${next}"]`)
          ?.scrollIntoView({ block: 'nearest' });
      } else if (e.key === 'Enter') {
        if (e.target instanceof HTMLButtonElement) return;
        e.preventDefault();
        const hit = results[activeIndex];
        if (hit) open(hit);
      }
    },
    [results, activeIndex, open]
  );

  return (
    <ModalDialog
      title={dialogs.searchFiles.title}
      className="search-files-dialog"
      onClose={onClose}
      onKeyDown={handleKeyDown}
    >
      <div className="dialog-content">
        <input
          ref={inputRef}
          type="search"
          className="qh-input focus-accent search-files-input"
          placeholder={fileSidebar.searchPlaceholder}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label={fileSidebar.searchLabel}
          aria-controls="search-files-results"
          aria-activedescendant={
            results[activeIndex] ? `search-result-${activeIndex}` : undefined
          }
        />
        <div
          ref={listRef}
          id="search-files-results"
          className="search-files-results"
          role={results.length > 0 ? 'listbox' : undefined}
          aria-label={results.length > 0 ? fileSidebar.resultsLabel : undefined}
        >
          {isSearching && results.length === 0 && (
            <div className="empty-state">
              <p>{fileSidebar.noMatches}</p>
            </div>
          )}
          {results.map((result, i) => {
            if (!filesByPath.has(result.path)) return null;
            const fileName = result.path.split('/').pop() || result.path;
            const dir = result.path.slice(0, result.path.length - fileName.length);
            const content = fileContents?.get(result.path);
            const snippet = content ? buildSnippet(content, result.terms) : [];
            const isActive = i === activeIndex;
            return (
              <div
                key={result.path}
                id={`search-result-${i}`}
                data-index={i}
                role="option"
                aria-selected={isActive}
                className={`search-result qh-row-hover qh-active-accent-row ${isActive ? 'active' : ''}`}
                onMouseMove={() => setActiveIndex(i)}
                onClick={() => open(result)}
              >
                <div className="search-result-header">
                  {getFileIcon(result.path)}
                  <span className="search-result-name qh-truncate">{fileName}</span>
                  {dir && <span className="search-result-path qh-truncate">{dir}</span>}
                </div>
                {snippet.length > 0 && (
                  <div className="search-result-snippet qh-truncate">
                    {snippet.map((seg, j) =>
                      seg.match ? <mark key={j}>{seg.text}</mark> : <span key={j}>{seg.text}</span>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </ModalDialog>
  );
}
