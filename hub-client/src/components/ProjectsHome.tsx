/**
 * ProjectsHome — full-page projects view (explore/projects-shelves-ui).
 *
 * Implements the "Short term" design from QH-ProjectManagement-July26.fig:
 * shelves + streamlined entry, buildable on today's metadata. Replaces the
 * ProjectSelector modal on this exploration branch:
 *   - header bar: logo, search (⌘K), Connect/Import ▾, ＋ New ▾, avatar menu
 *   - personal shelves with project cards (paged at 6+)
 *   - "Everything else" list with per-project ⋯ menu, Rename and Peek for
 *     unnamed projects
 *   - identity / cursor color / device linking / JSON backup relocated into
 *     the avatar menu; plumbing (doc IDs, wss) behind ⋯ → Copy
 */

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
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
import {
  DEFAULT_SYNC_SERVER,
  buildProjectSetLinkUrl,
  buildShareableUrl,
} from '../utils/routing';
import ShareDialog from './ShareDialog';
import { useShelves, setPendingShelfAssignment, type Shelf } from '../hooks/useShelves';
import './ProjectsHome.css';

interface Props {
  onSelectProject: (project: ProjectEntry, filePathOverride?: string) => void;
  isConnecting?: boolean;
  error?: string | null;
  onProjectCreated?: (files: ProjectFile[], title: string, projectType: string, syncServer: string) => void;
  onSignOut?: () => void;
  authEmail?: string;
  onScreenNameChange?: (name: string) => void;
  onColorChange?: (color: string) => void;
  projectSetDocId?: string | null;
  projectSetSyncServer?: string | null;
  projectSetStatus?: ProjectSetStatus;
  projectSetEntries?: ProjectSetEntry[];
  onRemoveProjectFromSet?: (indexDocId: string) => void;
  onTouchProject?: (indexDocId: string) => void;
  onAddProjectToSet?: (entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>) => void;
  onRenameProject?: (indexDocId: string, description: string) => void;
  onSwitchToClassicUi?: () => void;
}

/** Unified view of a project regardless of source (synced set vs legacy IDB). */
interface ProjectItem {
  indexDocId: string;
  syncServer: string;
  description: string;
  addedAt: string;
  lastAccessed: string;
}

const COLOR_PALETTE = [
  '#E91E63', '#9C27B0', '#3F51B5', '#2196F3',
  '#00BCD4', '#009688', '#4CAF50', '#FF9800',
  '#FF5722', '#795548',
];

const SHELF_PAGE_SIZE = 8; // two rows of four cards

const UNNAMED_RE = /^Project \d{4}-\d{2}-\d{2}T/;

function isUnnamed(description: string): boolean {
  return UNNAMED_RE.test(description);
}

function initialsFor(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return '?';
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase();
}

function shortId(indexDocId: string): string {
  return indexDocId.replace(/^automerge:/, '').slice(0, 8) + '…';
}

function formatOpened(iso: string): string {
  const then = new Date(iso);
  if (isNaN(then.getTime())) return '';
  const now = new Date();
  const startOfDay = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const dayDiff = Math.round((startOfDay(now) - startOfDay(then)) / 86_400_000);
  if (dayDiff <= 0) return 'today';
  if (dayDiff === 1) return 'yesterday';
  if (dayDiff < 7) return then.toLocaleDateString(undefined, { weekday: 'short' });
  if (then.getFullYear() === now.getFullYear()) {
    return then.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  }
  return then.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

function serverHost(syncServer: string): string {
  try {
    return new URL(syncServer).host;
  } catch {
    return syncServer;
  }
}

/**
 * Parse the Connect input: accepts a share link
 * (…#/share/<id>?server=…&name=…) or a bare bs58 document ID.
 */
function parseConnectInput(input: string): { docId: string; server?: string; name?: string } | null {
  const trimmed = input.trim();
  if (!trimmed) return null;
  const shareMatch = trimmed.match(/#\/share\/([^?\s]+)(?:\?([^\s]*))?/);
  if (shareMatch) {
    const params = new URLSearchParams(shareMatch[2] ?? '');
    return {
      docId: shareMatch[1],
      server: params.get('server') ?? undefined,
      name: params.get('name') ?? undefined,
    };
  }
  if (/^(automerge:)?[1-9A-HJ-NP-Za-km-z]{10,}$/.test(trimmed)) {
    return { docId: trimmed };
  }
  return null;
}

type SortOrder = 'newest' | 'oldest' | 'name';

export default function ProjectsHome({
  onSelectProject,
  isConnecting,
  error: connectionError,
  onProjectCreated,
  onSignOut,
  authEmail,
  onScreenNameChange,
  onColorChange,
  projectSetDocId,
  projectSetSyncServer,
  projectSetStatus,
  projectSetEntries,
  onRemoveProjectFromSet,
  onTouchProject,
  onAddProjectToSet,
  onRenameProject,
  onSwitchToClassicUi,
}: Props) {
  const projectSetConnecting = projectSetStatus === 'loading' || projectSetStatus === 'connecting';
  const useProjectSet = !!projectSetEntries || projectSetConnecting;

  // Legacy IDB fallback (only when no project set is in play)
  const [legacyProjects, setLegacyProjects] = useState<ProjectEntry[]>([]);
  const [loading, setLoading] = useState(true);

  const [search, setSearch] = useState('');
  const searchRef = useRef<HTMLInputElement>(null);

  // Menus / popovers. openMenu identifies the ⋯ menu by project id or
  // `shelf:<id>`; submenus and the peek popover are tracked separately.
  const [openMenu, setOpenMenu] = useState<string | null>(null);
  const [moveSubmenuOpen, setMoveSubmenuOpen] = useState(false);
  const [newMenuOpen, setNewMenuOpen] = useState(false);
  const [avatarMenuOpen, setAvatarMenuOpen] = useState(false);
  const [peekFor, setPeekFor] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  // Dialogs
  const [newDialogChoice, setNewDialogChoice] = useState<ProjectChoice | null>(null);
  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [showLinkDialog, setShowLinkDialog] = useState(false);
  const [renameFor, setRenameFor] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');

  const [formError, setFormError] = useState<string | null>(null);

  // ＋ New dialog state
  const [newTitle, setNewTitle] = useState('');
  const [newShelfId, setNewShelfId] = useState<string>('');
  const [newServer, setNewServer] = useState(DEFAULT_SYNC_SERVER);
  const [showServerField, setShowServerField] = useState(false);
  const [isCreating, setIsCreating] = useState(false);
  const [projectChoices, setProjectChoices] = useState<ProjectChoice[]>([]);

  // Connect / Import dialog state
  const [addTab, setAddTab] = useState<'connect' | 'import'>('connect');
  const [connectInput, setConnectInput] = useState('');
  const [connectServer, setConnectServer] = useState(DEFAULT_SYNC_SERVER);
  const [showConnectServer, setShowConnectServer] = useState(false);
  const [connectName, setConnectName] = useState('');
  const [importFile, setImportFile] = useState<File | null>(null);
  const [importTitle, setImportTitle] = useState('');
  const [isImporting, setIsImporting] = useState(false);

  // Identity
  const [userSettings, setUserSettings] = useState<UserSettings | null>(null);
  const [editingName, setEditingName] = useState(false);
  const [editNameValue, setEditNameValue] = useState('');

  const { colorScheme, cycleColorScheme } = useTheme();
  const { shelves, createShelf, renameShelf, deleteShelf, moveProject, reconcilePending } = useShelves();
  const [shelfPages, setShelfPages] = useState<Record<string, number>>({});
  // Drag-and-drop between shelves and the unshelved list. dropTarget is a
  // shelf id or 'unshelved'.
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const [sortOrder, setSortOrder] = useState<SortOrder>('newest');
  const [sortMenuOpen, setSortMenuOpen] = useState(false);

  const projectSetLinkUrl = projectSetDocId && projectSetSyncServer
    ? buildProjectSetLinkUrl(projectSetDocId, projectSetSyncServer)
    : undefined;

  // ---- data loading ----

  useEffect(() => {
    if (useProjectSet) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const entries = await projectStorage.listProjects();
        if (!cancelled) setLegacyProjects(entries);
      } catch (err) {
        console.error('Failed to load projects:', err);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [useProjectSet]);

  useEffect(() => {
    userSettingsService.getUserIdentity().then(setUserSettings).catch((err) => {
      console.error('Failed to load user settings:', err);
    });
    getProjectChoices().then(setProjectChoices).catch((err) => {
      console.error('Failed to load project choices:', err);
    });
  }, []);

  const items: ProjectItem[] = useMemo(() => {
    if (useProjectSet) {
      return (projectSetEntries ?? []).map((e) => ({
        indexDocId: e.indexDocId,
        syncServer: e.syncServer,
        description: e.description,
        addedAt: e.addedAt,
        lastAccessed: e.lastAccessed,
      }));
    }
    return legacyProjects.map((p) => ({
      indexDocId: p.indexDocId,
      syncServer: p.syncServer,
      description: p.description,
      addedAt: p.createdAt,
      lastAccessed: p.lastAccessed,
    }));
  }, [useProjectSet, projectSetEntries, legacyProjects]);

  const byId = useMemo(() => {
    const m = new Map<string, ProjectItem>();
    for (const it of items) m.set(it.indexDocId, it);
    return m;
  }, [items]);

  // Apply any pending "add to shelf on create" once the new entry appears.
  useEffect(() => {
    reconcilePending(items);
  }, [items, reconcilePending]);

  // ---- global listeners ----

  const closeAllMenus = useCallback(() => {
    setOpenMenu(null);
    setMoveSubmenuOpen(false);
    setNewMenuOpen(false);
    setAvatarMenuOpen(false);
    setPeekFor(null);
    setSortMenuOpen(false);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        searchRef.current?.focus();
      }
      if (e.key === 'Escape') closeAllMenus();
    };
    const onDown = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest('.qh-menu-anchor, .qh-peek')) {
        closeAllMenus();
      }
    };
    window.addEventListener('keydown', onKey);
    document.addEventListener('mousedown', onDown);
    return () => {
      window.removeEventListener('keydown', onKey);
      document.removeEventListener('mousedown', onDown);
    };
  }, [closeAllMenus]);

  // ---- actions ----

  // Custom MIME type marks drags originating from a project card/row.
  // Drop zones key off dataTransfer.types (readable during dragover, unlike
  // the payload) rather than React state, which lags the native events.
  const DRAG_TYPE = 'application/x-qh-project';

  const handleDragStart = useCallback((item: ProjectItem) => (e: React.DragEvent) => {
    e.dataTransfer.setData(DRAG_TYPE, item.indexDocId);
    e.dataTransfer.setData('text/plain', item.indexDocId);
    e.dataTransfer.effectAllowed = 'move';
    setDraggingId(item.indexDocId);
    closeAllMenus();
  }, [closeAllMenus]);

  const handleDragEnd = useCallback(() => {
    setDraggingId(null);
    setDropTarget(null);
  }, []);

  /** Drop-zone props for a shelf section (or 'unshelved' for the bottom list). */
  const dropZoneProps = useCallback((target: string) => ({
    onDragOver: (e: React.DragEvent) => {
      if (!e.dataTransfer.types.includes(DRAG_TYPE)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = 'move';
      if (dropTarget !== target) setDropTarget(target);
    },
    onDragLeave: (e: React.DragEvent) => {
      // Ignore leave events fired when moving over the zone's own children
      if (e.currentTarget.contains(e.relatedTarget as Node)) return;
      setDropTarget((t) => (t === target ? null : t));
    },
    onDrop: (e: React.DragEvent) => {
      e.preventDefault();
      const docId = e.dataTransfer.getData(DRAG_TYPE) || draggingId;
      if (docId) moveProject(docId, target === 'unshelved' ? null : target);
      handleDragEnd();
    },
  }), [draggingId, dropTarget, moveProject, handleDragEnd]);

  const handleOpen = useCallback(async (item: ProjectItem) => {
    // Ensure a local IDB entry exists (URL routing uses local ids)
    let localProject = await projectStorage.getProjectByIndexDocId(item.indexDocId);
    if (!localProject) {
      localProject = await projectStorage.addProject(item.indexDocId, item.syncServer, item.description);
    }
    await projectStorage.touchProject(localProject.id);
    onTouchProject?.(item.indexDocId);
    onSelectProject(localProject);
  }, [onSelectProject, onTouchProject]);

  const handleRemove = useCallback((item: ProjectItem) => {
    if (confirm(`Remove "${item.description}" from this device?\n\nThis doesn't delete the project for others.`)) {
      moveProject(item.indexDocId, null);
      onRemoveProjectFromSet?.(item.indexDocId);
    }
    closeAllMenus();
  }, [moveProject, onRemoveProjectFromSet, closeAllMenus]);

  const copyToClipboard = useCallback(async (text: string, label: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(label);
      setTimeout(() => setCopied(null), 2000);
    } catch (err) {
      console.error('Clipboard write failed:', err);
    }
  }, []);

  const startRename = useCallback((item: ProjectItem) => {
    setRenameFor(item.indexDocId);
    setRenameValue(isUnnamed(item.description) ? '' : item.description);
    closeAllMenus();
  }, [closeAllMenus]);

  const commitRename = useCallback(() => {
    if (renameFor && renameValue.trim()) {
      onRenameProject?.(renameFor, renameValue.trim());
    }
    setRenameFor(null);
    setRenameValue('');
  }, [renameFor, renameValue, onRenameProject]);

  const handleNewShelf = useCallback((): string | null => {
    const name = prompt('Shelf name');
    if (!name?.trim()) return null;
    return createShelf(name.trim());
  }, [createShelf]);

  const openNewDialog = useCallback((choice: ProjectChoice) => {
    setNewDialogChoice(choice);
    setNewTitle('');
    setNewShelfId('');
    setShowServerField(false);
    setFormError(null);
    setNewMenuOpen(false);
  }, []);

  const handleCreate = useCallback(async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newDialogChoice || !newTitle.trim() || !newServer.trim()) return;
    setIsCreating(true);
    setFormError(null);
    try {
      const result = await wasmCreateProject(newDialogChoice.id, newTitle.trim());
      if (!result.success || !result.files || result.files.length === 0) {
        setFormError(result.error || 'Failed to create project');
        return;
      }
      if (newShelfId) {
        setPendingShelfAssignment(newTitle.trim(), newShelfId);
      }
      onProjectCreated?.(result.files, newTitle.trim(), newDialogChoice.id, newServer.trim());
      setNewDialogChoice(null);
    } catch (err) {
      setFormError(err instanceof Error ? err.message : 'Failed to create project');
    } finally {
      setIsCreating(false);
    }
  }, [newDialogChoice, newTitle, newServer, newShelfId, onProjectCreated]);

  const handleConnect = useCallback(async (e: React.FormEvent) => {
    e.preventDefault();
    setFormError(null);
    const parsed = parseConnectInput(connectInput);
    if (!parsed) {
      setFormError('Paste a share link or a project ID');
      return;
    }
    const server = parsed.server ?? (showConnectServer ? connectServer.trim() : DEFAULT_SYNC_SERVER);
    if (!server) {
      setFormError('Sync server is required');
      return;
    }
    try {
      let normalizedDocId = parsed.docId;
      if (!normalizedDocId.startsWith('automerge:')) {
        normalizedDocId = `automerge:${normalizedDocId}`;
      }
      const name = connectName.trim() || parsed.name;
      let project = await projectStorage.getProjectByIndexDocId(normalizedDocId);
      if (!project) {
        project = await projectStorage.addProject(normalizedDocId, server, name);
      }
      if (useProjectSet && onAddProjectToSet) {
        try {
          onAddProjectToSet({
            indexDocId: normalizedDocId,
            syncServer: server,
            description: name ?? project.description,
          });
        } catch (err) {
          console.warn('Failed to add to synced project set:', err);
        }
      }
      setAddDialogOpen(false);
      setConnectInput('');
      setConnectName('');
      onSelectProject(project);
    } catch (err) {
      setFormError(err instanceof Error ? `Failed to connect: ${err.message}` : 'Failed to connect.');
    }
  }, [connectInput, connectName, connectServer, showConnectServer, useProjectSet, onAddProjectToSet, onSelectProject]);

  const handleImportZip = useCallback(async (e: React.FormEvent) => {
    e.preventDefault();
    setFormError(null);
    if (!importFile || !importTitle.trim()) {
      setFormError('Choose a ZIP file and a name');
      return;
    }
    setIsImporting(true);
    try {
      const bytes = new Uint8Array(await importFile.arrayBuffer());
      const files = importProjectFromZip(bytes);
      if (files.length === 0) {
        setFormError('The archive contains no usable files');
        return;
      }
      onProjectCreated?.(files, importTitle.trim(), 'imported', DEFAULT_SYNC_SERVER);
      setAddDialogOpen(false);
      setImportFile(null);
      setImportTitle('');
    } catch (err) {
      setFormError(err instanceof Error ? `Failed to import ZIP: ${err.message}` : 'Failed to import ZIP.');
    } finally {
      setIsImporting(false);
    }
  }, [importFile, importTitle, onProjectCreated]);

  const handleSaveName = useCallback(async () => {
    if (!editNameValue.trim()) return;
    try {
      const updated = await userSettingsService.updateUserName(editNameValue.trim());
      setUserSettings(updated);
      setEditingName(false);
      onScreenNameChange?.(updated.userName);
    } catch (err) {
      console.error('Failed to update name:', err);
    }
  }, [editNameValue, onScreenNameChange]);

  const handleColorChange = useCallback(async (color: string) => {
    try {
      const updated = await userSettingsService.updateUserColor(color);
      setUserSettings(updated);
      onColorChange?.(updated.userColor);
    } catch (err) {
      console.error('Failed to update color:', err);
    }
  }, [onColorChange]);

  const handleExportJson = useCallback(() => {
    const exportData = {
      schemaVersion: 4,
      exportedAt: new Date().toISOString(),
      projects: items.map((e) => ({
        id: '',
        indexDocId: e.indexDocId,
        syncServer: e.syncServer,
        description: e.description,
        createdAt: e.addedAt,
        lastAccessed: e.lastAccessed,
      })),
    };
    const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'quarto-hub-projects.json';
    a.click();
    URL.revokeObjectURL(url);
  }, [items]);

  const handleImportJson = useCallback(() => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'application/json';
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (!file) return;
      try {
        const count = await projectStorage.importProjects(await file.text());
        alert(`Imported ${count} project(s)`);
      } catch (err) {
        console.error('Failed to import:', err);
        alert('Failed to import projects. Invalid JSON format.');
      }
    };
    input.click();
  }, []);

  // ---- derived view data ----

  const query = search.trim().toLowerCase();
  const matches = useCallback(
    (item: ProjectItem) => !query || item.description.toLowerCase().includes(query),
    [query],
  );

  const shelvedIds = useMemo(() => {
    const s = new Set<string>();
    for (const shelf of shelves) for (const id of shelf.projectIds) s.add(id);
    return s;
  }, [shelves]);

  const everythingElse = useMemo(() => {
    const rest = items.filter((it) => !shelvedIds.has(it.indexDocId)).filter(matches);
    const sorted = [...rest];
    if (sortOrder === 'newest') {
      sorted.sort((a, b) => (a.lastAccessed < b.lastAccessed ? 1 : -1));
    } else if (sortOrder === 'oldest') {
      sorted.sort((a, b) => (a.lastAccessed > b.lastAccessed ? 1 : -1));
    } else {
      sorted.sort((a, b) => a.description.localeCompare(b.description));
    }
    return sorted;
  }, [items, shelvedIds, matches, sortOrder]);

  const sortLabel = sortOrder === 'newest' ? 'newest first' : sortOrder === 'oldest' ? 'oldest first' : 'A to Z';

  // ---- rendering ----

  if (loading || projectSetConnecting) {
    return (
      <div className="projects-home">
        <div className="ph-loading">
          {projectSetConnecting ? 'Connecting to project set…' : 'Loading projects…'}
        </div>
      </div>
    );
  }

  const renderProjectMenu = (item: ProjectItem) => (
    <div className="ph-menu" role="menu">
      <button className="ph-menu-item strong" onClick={() => { closeAllMenus(); handleOpen(item); }}>
        Open
      </button>
      <div className="ph-menu-item ph-submenu-parent">
        <button
          className="ph-menu-item-inner"
          onClick={(e) => { e.stopPropagation(); setMoveSubmenuOpen((v) => !v); }}
        >
          Move to shelf <span className="ph-submenu-arrow">▸</span>
        </button>
        {moveSubmenuOpen && (
          <div className="ph-menu ph-submenu">
            {shelves.map((shelf) => (
              <button
                key={shelf.id}
                className="ph-menu-item"
                onClick={() => { moveProject(item.indexDocId, shelf.id); closeAllMenus(); }}
              >
                {shelf.name}
              </button>
            ))}
            {shelvedIds.has(item.indexDocId) && (
              <button
                className="ph-menu-item"
                onClick={() => { moveProject(item.indexDocId, null); closeAllMenus(); }}
              >
                No shelf
              </button>
            )}
            <button
              className="ph-menu-item accent"
              onClick={() => {
                const id = handleNewShelf();
                if (id) moveProject(item.indexDocId, id);
                closeAllMenus();
              }}
            >
              ＋ New shelf…
            </button>
          </div>
        )}
      </div>
      <button
        className="ph-menu-item"
        onClick={() => copyToClipboard(
          buildShareableUrl(item.indexDocId, item.syncServer, item.description, 'index.qmd'),
          item.indexDocId + ':share',
        )}
      >
        {copied === item.indexDocId + ':share' ? 'Link copied!' : 'Share link…'}
      </button>
      <button
        className="ph-menu-item with-hint"
        onClick={() => copyToClipboard(item.indexDocId.replace(/^automerge:/, ''), item.indexDocId + ':id')}
      >
        {copied === item.indexDocId + ':id' ? 'ID copied!' : 'Copy project ID'}
        <span className="ph-menu-hint mono">{shortId(item.indexDocId)}</span>
      </button>
      <button className="ph-menu-item" onClick={() => startRename(item)}>Rename…</button>
      <div className="ph-menu-divider" />
      <button className="ph-menu-item danger" onClick={() => handleRemove(item)}>
        Remove from this device
        <span className="ph-menu-subtext">Doesn't delete the project for others</span>
      </button>
    </div>
  );

  const renderPeek = (item: ProjectItem) => (
    <div className="qh-peek ph-peek" onMouseDown={(e) => e.stopPropagation()}>
      <div className="ph-peek-header">
        ADDED {formatOpened(item.addedAt).toUpperCase()} · OPENED {formatOpened(item.lastAccessed).toUpperCase()}
      </div>
      <div className="ph-peek-row"><span className="mono">{serverHost(item.syncServer)}</span></div>
      <div className="ph-peek-row"><span className="mono">{shortId(item.indexDocId)}</span></div>
      <div className="ph-peek-note">
        File-list preview isn't wired up in this exploration yet — it needs a
        lightweight index-doc connection.
      </div>
      <div className="ph-peek-divider" />
      <div className="ph-peek-actions">
        <button className="ph-btn primary" onClick={() => startRename(item)}>Rename…</button>
        <button className="ph-btn outline" onClick={() => { closeAllMenus(); handleOpen(item); }}>Open</button>
        <button className="ph-link danger" onClick={() => handleRemove(item)}>Remove…</button>
      </div>
      <div className="ph-peek-footnote">Peeking doesn't count as opening the project.</div>
    </div>
  );

  const renderCard = (item: ProjectItem) => (
    <div
      key={item.indexDocId}
      className={`ph-card qh-menu-anchor ${draggingId === item.indexDocId ? 'dragging' : ''}`}
      draggable
      onDragStart={handleDragStart(item)}
      onDragEnd={handleDragEnd}
    >
      <button className="ph-card-body" onClick={() => handleOpen(item)} title={item.description}>
        <span className={`ph-card-name ${isUnnamed(item.description) ? 'unnamed' : ''}`}>
          {item.description}
        </span>
        <span className="ph-card-meta">opened {formatOpened(item.lastAccessed)}</span>
      </button>
      <button
        className="ph-card-menu-btn"
        title="Project actions"
        onClick={(e) => {
          e.stopPropagation();
          setMoveSubmenuOpen(false);
          setOpenMenu(openMenu === item.indexDocId ? null : item.indexDocId);
        }}
      >
        ⋯
      </button>
      {openMenu === item.indexDocId && renderProjectMenu(item)}
    </div>
  );

  const renderShelf = (shelf: Shelf) => {
    const shelfItems = shelf.projectIds
      .map((id) => byId.get(id))
      .filter((it): it is ProjectItem => !!it)
      .filter(matches);
    if (query && shelfItems.length === 0) return null;
    const pageCount = Math.max(1, Math.ceil(shelfItems.length / SHELF_PAGE_SIZE));
    const page = Math.min(shelfPages[shelf.id] ?? 0, pageCount - 1);
    const pageItems = shelfItems.slice(page * SHELF_PAGE_SIZE, (page + 1) * SHELF_PAGE_SIZE);
    const menuKey = `shelf:${shelf.id}`;
    return (
      <section
        key={shelf.id}
        className={`ph-shelf ${dropTarget === shelf.id ? 'drop-target' : ''}`}
        {...dropZoneProps(shelf.id)}
      >
        <div className="ph-shelf-header qh-menu-anchor">
          <span className="ph-shelf-name">{shelf.name}</span>
          <span className="ph-shelf-count">{shelfItems.length}</span>
          <span className="ph-flex-spacer" />
          <button
            className="ph-icon-btn"
            title="Shelf actions"
            onClick={(e) => {
              e.stopPropagation();
              setOpenMenu(openMenu === menuKey ? null : menuKey);
            }}
          >
            ⋯
          </button>
          {openMenu === menuKey && (
            <div className="ph-menu ph-menu-right" role="menu">
              <button
                className="ph-menu-item"
                onClick={() => {
                  const name = prompt('Rename shelf', shelf.name);
                  if (name?.trim()) renameShelf(shelf.id, name.trim());
                  closeAllMenus();
                }}
              >
                Rename shelf…
              </button>
              <button
                className="ph-menu-item danger"
                onClick={() => {
                  if (confirm(`Delete shelf "${shelf.name}"?\n\nProjects return to Everything else — nothing is deleted.`)) {
                    deleteShelf(shelf.id);
                  }
                  closeAllMenus();
                }}
              >
                Delete shelf
                <span className="ph-menu-subtext">Projects return to Everything else</span>
              </button>
            </div>
          )}
        </div>
        {shelfItems.length === 0 ? (
          <div className="ph-shelf-empty">Empty shelf — use a project's ⋯ menu to move it here.</div>
        ) : (
          <div className="ph-shelf-row">
            {page > 0 && (
              <button
                className="ph-pager"
                title="Newer projects"
                onClick={() => setShelfPages((p) => ({ ...p, [shelf.id]: page - 1 }))}
              >
                ‹
              </button>
            )}
            <div className="ph-card-grid">{pageItems.map(renderCard)}</div>
            {page < pageCount - 1 ? (
              <button
                className="ph-pager"
                title="Older projects"
                onClick={() => setShelfPages((p) => ({ ...p, [shelf.id]: page + 1 }))}
              >
                ›
                <span className="ph-pager-pos mono">{page + 1}/{pageCount}</span>
              </button>
            ) : pageCount > 1 ? (
              <div className="ph-pager ph-pager-idle">
                <span className="ph-pager-pos mono">{page + 1}/{pageCount}</span>
              </div>
            ) : null}
          </div>
        )}
      </section>
    );
  };

  return (
    <div className="projects-home">
      <header className="ph-header">
        <div className="ph-logo">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <rect x="3" y="4" width="18" height="3.5" rx="1" stroke="var(--posit-teal)" strokeWidth="1.8" />
            <rect x="3" y="10.2" width="18" height="3.5" rx="1" stroke="var(--posit-teal)" strokeWidth="1.8" />
            <rect x="3" y="16.5" width="11" height="3.5" rx="1" stroke="var(--posit-teal)" strokeWidth="1.8" />
          </svg>
          <span>Quarto Hub</span>
        </div>
        <div className="ph-search">
          <input
            ref={searchRef}
            type="text"
            placeholder="Search projects…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <span className="ph-search-kbd mono">⌘K</span>
        </div>
        <div className="ph-flex-spacer" />
        <div className="ph-header-actions">
          <button className="ph-btn outline" onClick={() => { setAddDialogOpen(true); setAddTab('connect'); setFormError(null); }}>
            Connect / Import ▾
          </button>
          <div className="qh-menu-anchor ph-new-anchor">
            <button className="ph-btn primary" onClick={() => setNewMenuOpen((v) => !v)}>
              ＋ New ▾
            </button>
            {newMenuOpen && (
              <div className="ph-menu ph-menu-right" role="menu">
                <div className="ph-menu-label">START FROM — QUARTO PROJECT TYPES</div>
                {(projectChoices.length > 0
                  ? projectChoices
                  : [{ id: 'default', name: 'Default', description: 'A minimal Quarto project' }]
                ).map((choice) => (
                  <button key={choice.id} className="ph-menu-item two-line" onClick={() => openNewDialog(choice)}>
                    <span className="strong">{choice.name}</span>
                    <span className="ph-menu-subtext">{choice.description}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
          <div className="qh-menu-anchor ph-avatar-anchor">
            <button
              className="ph-avatar"
              style={{ backgroundColor: userSettings?.userColor ?? 'var(--posit-blue)' }}
              onClick={() => setAvatarMenuOpen((v) => !v)}
              title={userSettings?.userName}
            >
              {initialsFor(userSettings?.userName ?? '')}
            </button>
            {avatarMenuOpen && userSettings && (
              <div className="ph-menu ph-menu-right ph-avatar-menu">
                <div className="ph-avatar-menu-id">
                  <span className="ph-avatar big" style={{ backgroundColor: userSettings.userColor }}>
                    {initialsFor(userSettings.userName)}
                  </span>
                  <div className="ph-avatar-menu-who">
                    {editingName ? (
                      <input
                        className="ph-name-input"
                        value={editNameValue}
                        onChange={(e) => setEditNameValue(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === 'Enter') handleSaveName();
                          if (e.key === 'Escape') setEditingName(false);
                        }}
                        onBlur={handleSaveName}
                        autoFocus
                      />
                    ) : (
                      <div className="ph-avatar-menu-name">
                        <strong>{userSettings.userName}</strong>
                        <button
                          className="ph-link"
                          onClick={() => { setEditNameValue(userSettings.userName); setEditingName(true); }}
                        >
                          edit
                        </button>
                      </div>
                    )}
                    <div className="ph-avatar-menu-mail">
                      {authEmail ?? 'Not signed in'}
                      {onSignOut && <> · <button className="ph-link" onClick={onSignOut}>Sign out</button></>}
                    </div>
                  </div>
                </div>
                <div className="ph-menu-label">CURSOR COLOR</div>
                <div className="ph-swatches">
                  {COLOR_PALETTE.map((color) => (
                    <button
                      key={color}
                      className={`ph-swatch ${userSettings.userColor === color ? 'selected' : ''}`}
                      style={{ backgroundColor: color }}
                      onClick={() => handleColorChange(color)}
                      title={color}
                    />
                  ))}
                </div>
                <div className="ph-menu-divider" />
                {projectSetLinkUrl && (
                  <button className="ph-menu-item" onClick={() => { setShowLinkDialog(true); setAvatarMenuOpen(false); }}>
                    Link another browser…
                  </button>
                )}
                <button className="ph-menu-item" onClick={handleExportJson}>Back up list (JSON)…</button>
                <button className="ph-menu-item" onClick={handleImportJson}>Restore list (JSON)…</button>
                <button className="ph-menu-item with-hint" onClick={cycleColorScheme}>
                  Theme
                  <span className="ph-menu-hint">
                    {colorScheme === 'auto' ? 'Auto' : colorScheme === 'dark' ? 'Dark' : 'Light'} ▾
                  </span>
                </button>
                {onSwitchToClassicUi && (
                  <>
                    <div className="ph-menu-divider" />
                    <button className="ph-menu-item" onClick={onSwitchToClassicUi}>
                      Switch to classic UI
                      <span className="ph-menu-subtext">Back to the current shipping project list</span>
                    </button>
                  </>
                )}
              </div>
            )}
          </div>
        </div>
      </header>

      {(connectionError || formError) && !addDialogOpen && !newDialogChoice && (
        <div className="ph-error">{connectionError || formError}</div>
      )}
      {isConnecting && <div className="ph-connecting">Connecting to sync server…</div>}

      <main className="ph-main">
        {items.length === 0 ? (
          <div className="ph-empty-state">
            <h2>No projects yet</h2>
            <p>Create your first Quarto project, or connect to one a collaborator shared.</p>
            <div className="ph-empty-actions">
              <button className="ph-btn primary" onClick={() => setNewMenuOpen(true)}>＋ New project</button>
              <button className="ph-btn outline" onClick={() => { setAddDialogOpen(true); setAddTab('connect'); }}>
                Connect / Import
              </button>
            </div>
          </div>
        ) : (
          <>
            {shelves.map(renderShelf)}

            <div className="ph-new-shelf-row">
              <button className="ph-btn ghost-accent" onClick={handleNewShelf}>＋ New shelf</button>
            </div>

            <section
              className={`ph-rest ${dropTarget === 'unshelved' ? 'drop-target' : ''}`}
              {...dropZoneProps('unshelved')}
            >
              <div className="ph-rest-header qh-menu-anchor">
                <span className="ph-rest-title">Everything else</span>
                <span className="ph-rest-count">{everythingElse.length} · {sortLabel}</span>
                <span className="ph-flex-spacer" />
                <button className="ph-btn small outline" onClick={(e) => { e.stopPropagation(); setSortMenuOpen((v) => !v); }}>
                  Sort <span className="ph-caret">▾</span>
                </button>
                {sortMenuOpen && (
                  <div className="ph-menu ph-menu-right">
                    {(['newest', 'oldest', 'name'] as SortOrder[]).map((o) => (
                      <button
                        key={o}
                        className={`ph-menu-item ${sortOrder === o ? 'strong' : ''}`}
                        onClick={() => { setSortOrder(o); setSortMenuOpen(false); }}
                      >
                        {o === 'newest' ? 'Newest first' : o === 'oldest' ? 'Oldest first' : 'A to Z'}
                      </button>
                    ))}
                  </div>
                )}
              </div>
              {everythingElse.length === 0 ? (
                <div className="ph-rest-empty">
                  {query ? 'No projects match your search.' : 'Everything is on a shelf.'}
                </div>
              ) : (
                <div className="ph-rest-list">
                  {everythingElse.map((item) => (
                    <div
                      key={item.indexDocId}
                      className={`ph-row qh-menu-anchor ${draggingId === item.indexDocId ? 'dragging' : ''}`}
                      draggable
                      onDragStart={handleDragStart(item)}
                      onDragEnd={handleDragEnd}
                    >
                      <button
                        className={`ph-row-name ${isUnnamed(item.description) ? 'unnamed' : ''}`}
                        onClick={() => handleOpen(item)}
                        title={item.description}
                      >
                        {item.description}
                      </button>
                      {isUnnamed(item.description) && (
                        <>
                          <button className="ph-link" onClick={() => startRename(item)}>Rename</button>
                          <button
                            className="ph-link muted"
                            onClick={(e) => {
                              e.stopPropagation();
                              setPeekFor(peekFor === item.indexDocId ? null : item.indexDocId);
                            }}
                          >
                            Peek
                          </button>
                        </>
                      )}
                      <span className="ph-row-meta">opened {formatOpened(item.lastAccessed)}</span>
                      <button
                        className="ph-icon-btn ph-row-menu-btn"
                        title="Project actions"
                        onClick={(e) => {
                          e.stopPropagation();
                          setMoveSubmenuOpen(false);
                          setOpenMenu(openMenu === item.indexDocId ? null : item.indexDocId);
                        }}
                      >
                        ⋯
                      </button>
                      {openMenu === item.indexDocId && renderProjectMenu(item)}
                      {peekFor === item.indexDocId && renderPeek(item)}
                    </div>
                  ))}
                </div>
              )}
            </section>
          </>
        )}
      </main>

      {/* Rename dialog */}
      {renameFor && (
        <div className="ph-dialog-backdrop" onMouseDown={() => setRenameFor(null)}>
          <div className="ph-dialog" onMouseDown={(e) => e.stopPropagation()}>
            <h2>Rename project</h2>
            <form onSubmit={(e) => { e.preventDefault(); commitRename(); }}>
              <label className="ph-field-label" htmlFor="ph-rename">Name</label>
              <input
                id="ph-rename"
                className="ph-input focus-accent"
                value={renameValue}
                onChange={(e) => setRenameValue(e.target.value)}
                placeholder="e.g. Lab retreat agenda"
                autoFocus
              />
              <div className="ph-dialog-actions">
                <button type="button" className="ph-btn outline" onClick={() => setRenameFor(null)}>Cancel</button>
                <button type="submit" className="ph-btn primary" disabled={!renameValue.trim()}>Rename</button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* New project dialog */}
      {newDialogChoice && (
        <div className="ph-dialog-backdrop" onMouseDown={() => setNewDialogChoice(null)}>
          <div className="ph-dialog" onMouseDown={(e) => e.stopPropagation()}>
            <h2>New {newDialogChoice.name.toLowerCase()}</h2>
            <p className="ph-dialog-hint">Starter files will be created for you</p>
            {formError && <div className="ph-error inline">{formError}</div>}
            <form onSubmit={handleCreate}>
              <label className="ph-field-label" htmlFor="ph-new-name">Name</label>
              <input
                id="ph-new-name"
                className="ph-input focus-accent"
                value={newTitle}
                onChange={(e) => setNewTitle(e.target.value)}
                placeholder="Q3 all-hands deck"
                autoFocus
              />
              <label className="ph-field-label" htmlFor="ph-new-shelf">Add to shelf (optional)</label>
              <select
                id="ph-new-shelf"
                className="ph-input"
                value={newShelfId}
                onChange={(e) => setNewShelfId(e.target.value)}
              >
                <option value="">No shelf</option>
                {shelves.map((s) => (
                  <option key={s.id} value={s.id}>{s.name}</option>
                ))}
              </select>
              {showServerField ? (
                <>
                  <label className="ph-field-label" htmlFor="ph-new-server">Sync server</label>
                  <input
                    id="ph-new-server"
                    className="ph-input mono"
                    value={newServer}
                    onChange={(e) => setNewServer(e.target.value)}
                  />
                </>
              ) : (
                <div className="ph-server-line">
                  Syncs to {serverHost(newServer)}{' '}
                  <button type="button" className="ph-link" onClick={() => setShowServerField(true)}>Change…</button>
                </div>
              )}
              <div className="ph-dialog-actions">
                <button type="button" className="ph-btn outline" onClick={() => setNewDialogChoice(null)}>Cancel</button>
                <button type="submit" className="ph-btn primary" disabled={isCreating || !newTitle.trim()}>
                  {isCreating ? 'Creating…' : 'Create'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Connect / Import dialog */}
      {addDialogOpen && (
        <div className="ph-dialog-backdrop" onMouseDown={() => setAddDialogOpen(false)}>
          <div className="ph-dialog wide" onMouseDown={(e) => e.stopPropagation()}>
            <h2>Add an existing project</h2>
            <div className="ph-tabs">
              <button
                className={`ph-tab ${addTab === 'connect' ? 'active' : ''}`}
                onClick={() => { setAddTab('connect'); setFormError(null); }}
              >
                Connect by link or ID
              </button>
              <button
                className={`ph-tab ${addTab === 'import' ? 'active' : ''}`}
                onClick={() => { setAddTab('import'); setFormError(null); }}
              >
                Import from ZIP
              </button>
            </div>
            {formError && <div className="ph-error inline">{formError}</div>}
            {addTab === 'connect' ? (
              <form onSubmit={handleConnect}>
                <label className="ph-field-label" htmlFor="ph-connect-input">Paste a share link or project ID</label>
                <input
                  id="ph-connect-input"
                  className="ph-input mono focus-accent"
                  value={connectInput}
                  onChange={(e) => setConnectInput(e.target.value)}
                  placeholder="https://quarto-hub.com/#/share/… or bs58 ID"
                  autoFocus
                />
                {showConnectServer ? (
                  <>
                    <label className="ph-field-label" htmlFor="ph-connect-server">Sync server</label>
                    <input
                      id="ph-connect-server"
                      className="ph-input mono"
                      value={connectServer}
                      onChange={(e) => setConnectServer(e.target.value)}
                    />
                  </>
                ) : (
                  <div className="ph-server-line">
                    Server is read from the link · advanced:{' '}
                    <button type="button" className="ph-link" onClick={() => setShowConnectServer(true)}>
                      set server manually
                    </button>
                  </div>
                )}
                <label className="ph-field-label" htmlFor="ph-connect-name">Name it for your list (optional)</label>
                <input
                  id="ph-connect-name"
                  className="ph-input"
                  value={connectName}
                  onChange={(e) => setConnectName(e.target.value)}
                  placeholder="e.g. Lab retreat agenda"
                />
                <div className="ph-dialog-actions">
                  <button type="button" className="ph-btn outline" onClick={() => setAddDialogOpen(false)}>Cancel</button>
                  <button type="submit" className="ph-btn primary" disabled={!connectInput.trim()}>Connect</button>
                </div>
              </form>
            ) : (
              <form onSubmit={handleImportZip}>
                <label className="ph-field-label" htmlFor="ph-import-file">ZIP file</label>
                <input
                  id="ph-import-file"
                  className="ph-input"
                  type="file"
                  accept=".zip,application/zip"
                  onChange={(e) => {
                    const file = e.target.files?.[0] ?? null;
                    setImportFile(file);
                    if (file && !importTitle.trim()) {
                      setImportTitle(file.name.replace(/\.zip$/i, ''));
                    }
                  }}
                />
                <label className="ph-field-label" htmlFor="ph-import-title">Name</label>
                <input
                  id="ph-import-title"
                  className="ph-input"
                  value={importTitle}
                  onChange={(e) => setImportTitle(e.target.value)}
                  placeholder="My imported project"
                />
                <div className="ph-dialog-actions">
                  <button type="button" className="ph-btn outline" onClick={() => setAddDialogOpen(false)}>Cancel</button>
                  <button type="submit" className="ph-btn primary" disabled={isImporting || !importFile}>
                    {isImporting ? 'Importing…' : 'Import'}
                  </button>
                </div>
              </form>
            )}
          </div>
        </div>
      )}

      <ShareDialog
        isOpen={showLinkDialog}
        shareableUrl={projectSetLinkUrl}
        onClose={() => setShowLinkDialog(false)}
      />

      <footer className="ph-footer">
        <span className="mono" title={`Built: ${__BUILD_TIME__}\nCommit date: ${__GIT_COMMIT_DATE__}`}>
          {__GIT_COMMIT_HASH__}
        </span>
        <span className="ph-footer-note">shelves UI exploration</span>
      </footer>
    </div>
  );
}
