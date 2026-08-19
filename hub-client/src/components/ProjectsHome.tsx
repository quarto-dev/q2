/**
 * ProjectsHome — full-page projects view (explore/projects-collections-ui).
 *
 * Implements the "Short term" design from QH-ProjectManagement-July26.fig:
 * collections + streamlined entry, buildable on today's metadata. Replaces the
 * ProjectSelector modal on this exploration branch:
 *   - header bar: logo, search (⌘K), Connect/Import ▾, ＋ New ▾, avatar menu
 *   - personal collections with project cards (paged at 6+)
 *   - "Everything else" list with per-project ⋯ menu, Rename and Peek for
 *     unnamed projects
 *   - identity / cursor color / device linking / JSON backup relocated into
 *     the avatar menu; plumbing (doc IDs, wss) behind ⋯ → Copy
 */

import { useState, useEffect, useCallback, useMemo, useRef } from 'react';
import { useTheme } from './ThemeContext';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';
import type { ProjectSetEntry, ProjectSetEntrySummary } from '@quarto/quarto-automerge-schema';
import type { CollectionsStatus as ProjectSetStatus } from '../hooks/useCollectionSets';
import type { UserSettings } from '../services/storage/types';
import * as projectStorage from '../services/projectStorage';
import * as userSettingsService from '../services/userSettings';
import {
  getProjectChoices,
  createProject as wasmCreateProject,
  importProjectFromZip,
  exportProjectAsZip,
  connect,
  disconnect,
  getFileContent,
  getBinaryFileContent,
  isFileBinary,
  type ProjectChoice,
  type ProjectFile,
} from '@quarto/preview-runtime';
import {
  DEFAULT_SYNC_SERVER,
  buildProjectSetLinkUrl,
  buildShareableUrl,
  buildFullUrl,
  resolveSyncServerUrl,
} from '../utils/routing';
import ShareDialog from './ShareDialog';
import { sortProjectItems, sortOrderLabel, type SortOrder } from '../utils/projectSort';
import type { Face } from '../utils/facepile';
import type { CollectionSnapshot } from '../services/projectSetService';
import './ProjectsHome.css';

interface Props {
  onSelectProject: (project: ProjectEntry, filePathOverride?: string) => void;
  isConnecting?: boolean;
  error?: string | null;
  onProjectCreated?: (files: ProjectFile[], title: string, projectType: string, syncServer: string) => void;
  onSignOut?: () => void;
  authEmail?: string;
  authPicture?: string | null;
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
  /** Replace a project's cached peek summary (used by Peek's refresh). */
  onUpdateProjectSummary?: (indexDocId: string, summary: ProjectSetEntrySummary) => void;
  /** All connected collections, root first (from useCollectionSets). */
  collections?: CollectionSnapshot[];
  onCreateCollection?: (name: string) => Promise<string>;
  onUnsubscribeCollection?: (collectionDocId: string) => Promise<void>;
  onRenameCollection?: (collectionDocId: string, name: string) => void;
  onAddProjectToCollection?: (collectionDocId: string, entry: Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'>) => void;
  onRemoveProjectFromCollection?: (collectionDocId: string, indexDocId: string) => void;
  onMoveProjectBetweenCollections?: (fromDocId: string, toDocId: string, indexDocId: string) => void;
  onSwitchToClassicUi?: () => void;
}

/** In-component view of a non-root collection (adapted from a snapshot). */
interface CollectionView {
  /** The collection's ProjectSetDocument id. */
  id: string;
  name: string;
  syncServer: string;
  entries: ProjectSetEntry[];
  /** Entry keys (indexDocId without prefix), for membership checks. */
  projectIds: string[];
}

/** Unified view of a project regardless of source (synced set vs legacy IDB). */
interface ProjectItem {
  indexDocId: string;
  syncServer: string;
  description: string;
  addedAt: string;
  lastAccessed: string;
  summary?: ProjectSetEntrySummary;
}

const COLOR_PALETTE = [
  '#E91E63', '#9C27B0', '#3F51B5', '#2196F3',
  '#00BCD4', '#009688', '#4CAF50', '#FF9800',
  '#FF5722', '#795548',
];

const COLLECTION_PAGE_SIZE = 8; // two rows of four cards

const UNNAMED_RE = /^Project \d{4}-\d{2}-\d{2}T/;

/** Set to '1' once the user opts out of the shared-collection move warning. */
const MOVE_WARNING_KEY = 'qh-collection-move-warning-dismissed';

/**
 * Pending "add to collection on create": new-project flows don't know the
 * project's doc id until the parent app finishes creating it, so the
 * assignment is recorded by title and reconciled when the entry appears.
 */
const PENDING_ASSIGNMENT_KEY = 'qh-collection-pending-v1';

function setPendingCollectionAssignment(title: string, collectionId: string): void {
  localStorage.setItem(
    PENDING_ASSIGNMENT_KEY,
    JSON.stringify({ title, collectionId, ts: Date.now() }),
  );
}

/** Fork glyph for the duplicate affordance (three nodes, branch lines). */
const forkIcon = (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <circle cx="6" cy="5" r="2.2" stroke="currentColor" strokeWidth="2" />
    <circle cx="18" cy="5" r="2.2" stroke="currentColor" strokeWidth="2" />
    <circle cx="12" cy="19" r="2.2" stroke="currentColor" strokeWidth="2" />
    <path d="M6 7.5v1.5c0 1.7 1.3 3 3 3h6c1.7 0 3-1.3 3-3V7.5M12 12v4.5" stroke="currentColor" strokeWidth="2" />
  </svg>
);

/** Magnifying glass for the hover-to-peek affordance. */
const peekIcon = (
  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <circle cx="10.5" cy="10.5" r="6.5" stroke="currentColor" strokeWidth="2" />
    <path d="M15.5 15.5L21 21" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
  </svg>
);

/** Base64-encode without blowing the arg-spread limit on large files. */
function toBase64(bytes: Uint8Array): string {
  let binary = '';
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(binary);
}

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

export default function ProjectsHome({
  onSelectProject,
  isConnecting,
  error: connectionError,
  onProjectCreated,
  onSignOut,
  authEmail,
  authPicture,
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
  onUpdateProjectSummary,
  collections: collectionsProp,
  onCreateCollection,
  onUnsubscribeCollection,
  onRenameCollection,
  onAddProjectToCollection,
  onRemoveProjectFromCollection,
  onMoveProjectBetweenCollections,
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
  // `collection:<id>`; submenus and the peek popover are tracked separately.
  const [openMenu, setOpenMenu] = useState<string | null>(null);
  const [moveSubmenuOpen, setMoveSubmenuOpen] = useState<false | 'move' | 'add'>(false);
  const [newMenuOpen, setNewMenuOpen] = useState(false);
  const [avatarMenuOpen, setAvatarMenuOpen] = useState(false);
  const [peekFor, setPeekFor] = useState<string | null>(null);
  // Peek is a hover card: open on hover of the magnifying-glass icon, and
  // stay open while the pointer is over the icon or the popover. A short
  // close delay (cancelled by re-entering) bridges the gap between them and
  // lets the pointer reach the popover's action buttons.
  const peekTimerRef = useRef<number | null>(null);
  const openPeekHover = useCallback((indexDocId: string) => {
    if (peekTimerRef.current) { window.clearTimeout(peekTimerRef.current); peekTimerRef.current = null; }
    setPeekFor(indexDocId);
  }, []);
  const closePeekHoverSoon = useCallback(() => {
    if (peekTimerRef.current) window.clearTimeout(peekTimerRef.current);
    peekTimerRef.current = window.setTimeout(() => setPeekFor(null), 180);
  }, []);
  const [peekRefreshing, setPeekRefreshing] = useState(false);
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
  const [newCollectionId, setNewCollectionId] = useState<string>('');
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

  // ---- collections adapter ----
  // Collections are real ProjectSetDocuments delivered via props (root
  // first). The root is the personal superset; the sections on this page
  // render the non-root collections, and "Everything else" is computed as
  // root entries not present in any of them.
  const collectionViews: CollectionView[] = useMemo(
    () =>
      (collectionsProp ?? [])
        .filter((c) => !c.isRoot)
        .map((c) => ({
          id: c.docId,
          name: c.name ?? 'Untitled collection',
          syncServer: c.syncServer,
          entries: c.entries,
          projectIds: c.entries.map((e) => e.indexDocId.replace(/^automerge:/, '')),
        })),
    [collectionsProp],
  );
  const collections = collectionViews;
  // Which collection's members-and-invite popover is open
  const [membersFor, setMembersFor] = useState<string | null>(null);
  // Pending move out of a shared collection, awaiting the user's OK
  const [pendingMove, setPendingMove] = useState<{
    indexDocId: string;
    name: string;
    fromName: string;
    othersCount: number;
    target: string | null;
  } | null>(null);
  const [moveWarnChecked, setMoveWarnChecked] = useState(false);
  // In-app replacements for native prompt()/confirm(): embedded browsers can
  // throw on prompt() and auto-accept confirm(), so collection naming and
  // destructive confirmations get real dialogs.
  const [newCollectionDialog, setNewCollectionDialog] = useState<null | { forProject?: string }>(null);
  const [newCollectionName, setNewCollectionName] = useState('');
  const [renameCollectionTarget, setRenameCollectionTarget] = useState<CollectionView | null>(null);
  const [renameCollectionValue, setRenameCollectionValue] = useState('');
  const [confirmState, setConfirmState] = useState<null | {
    title: string;
    body: string;
    confirmLabel: string;
    action: () => void;
  }>(null);
  const [collectionPages, setCollectionPages] = useState<Record<string, number>>({});
  // Drag-and-drop between collections and the unshelved list. dropTarget is a
  // collection id or 'unshelved'.
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const [sortOrder, setSortOrder] = useState<SortOrder>('newest');
  const [sortMenuOpen, setSortMenuOpen] = useState(false);
  // Per-collection sort choice (view preference, not synced). Missing entry
  // means the default, newest first. The open sort menu is tracked in
  // openMenu under `sort:<collection id>` so the usual close-on-outside-click
  // and one-menu-at-a-time behavior applies.
  const [collectionSorts, setCollectionSorts] = useState<Record<string, SortOrder>>({});

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
        summary: e.summary,
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

  // Apply any pending "add to collection on create" once the new entry appears.
  useEffect(() => {
    const raw = localStorage.getItem(PENDING_ASSIGNMENT_KEY);
    if (!raw) return;
    try {
      const pending: { title: string; collectionId: string; ts: number } = JSON.parse(raw);
      if (Date.now() - pending.ts > 24 * 3600 * 1000) {
        localStorage.removeItem(PENDING_ASSIGNMENT_KEY);
        return;
      }
      const match = items.find(
        (e) => e.description === pending.title && new Date(e.addedAt).getTime() >= pending.ts - 60_000,
      );
      if (match) {
        onAddProjectToCollection?.(pending.collectionId, {
          indexDocId: match.indexDocId,
          syncServer: match.syncServer,
          description: match.description,
        });
        localStorage.removeItem(PENDING_ASSIGNMENT_KEY);
      }
    } catch {
      localStorage.removeItem(PENDING_ASSIGNMENT_KEY);
    }
  }, [items, onAddProjectToCollection]);

  // ---- global listeners ----

  const closeAllMenus = useCallback(() => {
    setOpenMenu(null);
    setMoveSubmenuOpen(false);
    setNewMenuOpen(false);
    setAvatarMenuOpen(false);
    setPeekFor(null);
    setSortMenuOpen(false);
    setMembersFor(null);
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

  const collectionOf = useCallback((indexDocId: string): CollectionView | undefined => {
    const short = indexDocId.replace(/^automerge:/, '');
    return collections.find((c) =>
      c.projectIds.some((id) => id === indexDocId || id === short || `automerge:${id}` === indexDocId),
    );
  }, [collections]);

  const entryFor = useCallback((indexDocId: string): Omit<ProjectSetEntry, 'addedAt' | 'lastAccessed'> | null => {
    const item = byId.get(indexDocId)
      ?? byId.get(indexDocId.replace(/^automerge:/, ''))
      ?? byId.get(`automerge:${indexDocId}`);
    if (item) return { indexDocId: item.indexDocId, syncServer: item.syncServer, description: item.description };
    for (const c of collections) {
      const e = c.entries.find((en) => en.indexDocId.replace(/^automerge:/, '') === indexDocId.replace(/^automerge:/, ''));
      if (e) return { indexDocId: e.indexDocId, syncServer: e.syncServer, description: e.description };
    }
    return null;
  }, [byId, collections]);

  /**
   * Apply a move: target collection gets the entry; a non-root source
   * collection loses it; target null means "no collection" (the entry
   * remains in the personal root superset either way).
   */
  const moveProject = useCallback((indexDocId: string, target: string | null) => {
    const from = collectionOf(indexDocId);
    if (target) {
      if (from && from.id !== target && onMoveProjectBetweenCollections) {
        onMoveProjectBetweenCollections(from.id, target, indexDocId);
      } else if (!from || from.id !== target) {
        const entry = entryFor(indexDocId);
        if (entry) onAddProjectToCollection?.(target, entry);
      }
    } else if (from) {
      onRemoveProjectFromCollection?.(from.id, indexDocId);
    }
  }, [collectionOf, entryFor, onMoveProjectBetweenCollections, onAddProjectToCollection, onRemoveProjectFromCollection]);

  /** Add without removing — a project can sit in several collections. */
  const addToCollection = useCallback((indexDocId: string, target: string) => {
    const entry = entryFor(indexDocId);
    if (entry) onAddProjectToCollection?.(target, entry);
  }, [entryFor, onAddProjectToCollection]);

  /** People other than you seen on a collection's projects (contributor
   * union from cached summaries). We can't know access for sure — bearer
   * links — so this is the acceptable-risk heuristic for the move warning. */
  const otherPeopleOn = useCallback((collection: CollectionView): number => {
    const names = new Set<string>();
    for (const e of collection.entries) {
      for (const c of e.summary?.contributors ?? []) names.add(c.name);
    }
    if (userSettings?.userName) names.delete(userSettings.userName);
    return names.size;
  }, [userSettings]);

  const requestMove = useCallback((indexDocId: string, target: string | null) => {
    const from = collectionOf(indexDocId);
    const othersCount = from ? otherPeopleOn(from) : 0;
    const suppressed = localStorage.getItem(MOVE_WARNING_KEY) === '1';
    if (from && from.id !== target && othersCount > 0 && !suppressed) {
      const item = byId.get(indexDocId) ?? byId.get(indexDocId.replace(/^automerge:/, '')) ?? byId.get(`automerge:${indexDocId}`);
      setMoveWarnChecked(false);
      setPendingMove({
        indexDocId,
        name: item?.description ?? 'this project',
        fromName: from.name,
        othersCount,
        target,
      });
      closeAllMenus();
      return;
    }
    moveProject(indexDocId, target);
  }, [collectionOf, otherPeopleOn, byId, moveProject, closeAllMenus]);

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

  /** Drop-zone props for a collection section (or 'unshelved' for the bottom list). */
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
      if (docId) requestMove(docId, target === 'unshelved' ? null : target);
      handleDragEnd();
    },
  }), [draggingId, dropTarget, requestMove, handleDragEnd]);

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

  // Which project is currently being exported as a ZIP (background connect)
  const [exportingId, setExportingId] = useState<string | null>(null);

  /**
   * Download a project's files as a ZIP without opening it: connect in the
   * background, pull every file's content (exportProjectAsZip reads only
   * what's loaded), zip, then tear the connection down. Safe from the home
   * page because nothing else holds the singleton sync client here.
   */
  const handleDownloadZip = useCallback(async (item: ProjectItem) => {
    if (exportingId) return;
    setExportingId(item.indexDocId);
    setFormError(null);
    try {
      await connect(resolveSyncServerUrl(item.syncServer), item.indexDocId);
      const bytes = exportProjectAsZip(item.description);
      const blob = new Blob([bytes as BlobPart], { type: 'application/zip' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `${item.description.replace(/[^\w.-]+/g, '-').replace(/^-+|-+$/g, '') || 'project'}.zip`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (err) {
      console.error('ZIP export failed:', err);
      setFormError(err instanceof Error ? `Export failed: ${err.message}` : 'Export failed.');
    } finally {
      try { await disconnect(); } catch { /* connection already down */ }
      setExportingId(null);
      closeAllMenus();
    }
  }, [exportingId, closeAllMenus]);

  // Which project is being duplicated (background connect + re-create)
  const [duplicatingId, setDuplicatingId] = useState<string | null>(null);
  // Duplicate dialog state: the fork source plus editable name/destination
  const [duplicateFor, setDuplicateFor] = useState<ProjectItem | null>(null);
  const [duplicateName, setDuplicateName] = useState('');
  const [duplicateCollectionId, setDuplicateCollectionId] = useState('');

  /** Open the duplicate (fork) dialog: name prefilled with "(copy)",
   * destination defaulting to the source's collection. */
  const openDuplicateDialog = useCallback((item: ProjectItem) => {
    setDuplicateFor(item);
    setDuplicateName(`${item.description} (copy)`);
    setDuplicateCollectionId(collectionOf(item.indexDocId)?.id ?? '');
    setFormError(null);
    closeAllMenus();
  }, [collectionOf, closeAllMenus]);

  /**
   * Duplicate (fork) a project: background-connect to the source, read every
   * file (text and binary), and feed them through the same creation path new
   * projects use — fresh documents, no history carried over. The chosen
   * collection is applied via the pending-assignment mechanism (the new doc
   * id isn't known until the parent finishes creating it).
   */
  const handleDuplicate = useCallback(async () => {
    if (!duplicateFor || duplicatingId || exportingId || !duplicateName.trim()) return;
    const item = duplicateFor;
    const title = duplicateName.trim();
    setDuplicatingId(item.indexDocId);
    setFormError(null);
    try {
      const files = await connect(resolveSyncServerUrl(item.syncServer), item.indexDocId);
      const projectFiles: ProjectFile[] = [];
      for (const f of files) {
        if (isFileBinary(f.path)) {
          const bin = getBinaryFileContent(f.path);
          if (bin) {
            projectFiles.push({ path: f.path, content_type: 'binary', content: toBase64(bin.content), mime_type: bin.mimeType });
          }
        } else {
          const text = getFileContent(f.path);
          if (text !== null) {
            projectFiles.push({ path: f.path, content_type: 'text', content: text });
          }
        }
      }
      if (projectFiles.length === 0) {
        setFormError('Nothing to duplicate — the project has no readable files.');
        return;
      }
      await disconnect();
      if (duplicateCollectionId) {
        setPendingCollectionAssignment(title, duplicateCollectionId);
      }
      setDuplicateFor(null);
      onProjectCreated?.(projectFiles, title, 'duplicate', item.syncServer);
    } catch (err) {
      console.error('Duplicate failed:', err);
      setFormError(err instanceof Error ? `Duplicate failed: ${err.message}` : 'Duplicate failed.');
      try { await disconnect(); } catch { /* connection already down */ }
    } finally {
      setDuplicatingId(null);
    }
  }, [duplicateFor, duplicateName, duplicateCollectionId, duplicatingId, exportingId, onProjectCreated]);

  const handleRemove = useCallback((item: ProjectItem) => {
    closeAllMenus();
    setConfirmState({
      title: `Remove "${item.description}" from this device?`,
      body: "This doesn't delete the project for others — anyone who has it keeps it.",
      confirmLabel: 'Remove',
      action: () => {
        moveProject(item.indexDocId, null);
        onRemoveProjectFromSet?.(item.indexDocId);
      },
    });
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

  /** Open the new-collection dialog; when a project id is given, the
   * project moves onto the collection once it's created. */
  const openNewCollection = useCallback((forProject?: string) => {
    setNewCollectionName('');
    setNewCollectionDialog({ forProject });
    closeAllMenus();
  }, [closeAllMenus]);

  const commitNewCollection = useCallback(async () => {
    if (!newCollectionDialog || !newCollectionName.trim() || !onCreateCollection) return;
    try {
      const id = await onCreateCollection(newCollectionName.trim());
      if (newCollectionDialog.forProject) {
        requestMove(newCollectionDialog.forProject, id);
      }
      setNewCollectionDialog(null);
      setNewCollectionName('');
    } catch (err) {
      setFormError(err instanceof Error ? `Could not create collection: ${err.message}` : 'Could not create collection.');
    }
  }, [newCollectionDialog, newCollectionName, onCreateCollection, requestMove]);

  const openNewDialog = useCallback((choice: ProjectChoice) => {
    setNewDialogChoice(choice);
    setNewTitle('');
    setNewCollectionId('');
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
      if (newCollectionId) {
        setPendingCollectionAssignment(newTitle.trim(), newCollectionId);
      }
      onProjectCreated?.(result.files, newTitle.trim(), newDialogChoice.id, newServer.trim());
      setNewDialogChoice(null);
    } catch (err) {
      setFormError(err instanceof Error ? err.message : 'Failed to create project');
    } finally {
      setIsCreating(false);
    }
  }, [newDialogChoice, newTitle, newServer, newCollectionId, onProjectCreated]);

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
    // Collection project ids may be stored with or without the 'automerge:'
    // prefix (invite links carry the short form); index both.
    const s = new Set<string>();
    for (const collection of collections) {
      for (const id of collection.projectIds) {
        s.add(id);
        s.add(id.startsWith('automerge:') ? id.replace(/^automerge:/, '') : `automerge:${id}`);
      }
    }
    return s;
  }, [collections]);

  const everythingElse = useMemo(
    () => sortProjectItems(items.filter((it) => !shelvedIds.has(it.indexDocId)).filter(matches), sortOrder),
    [items, shelvedIds, matches, sortOrder],
  );

  const sortLabel = sortOrderLabel(sortOrder);

  // The current user, as a face (real identity from user settings).
  const selfUser: Face | undefined = userSettings
    ? { name: `${userSettings.userName} (you)`, initials: initialsFor(userSettings.userName), color: userSettings.userColor }
    : undefined;

  // Real contributors for a project: the identities cached on its summary
  // (populated from the index doc when anyone opens it). A project nobody
  // else has touched shows just you — never fabricated authors.
  const contributorsFor = useCallback(
    (item: ProjectItem): Face[] => {
      const real = (item.summary?.contributors ?? []).map((c) => ({
        name: c.name,
        color: c.color,
        initials: initialsFor(c.name),
      }));
      if (real.length > 0) return real;
      return selfUser ? [{ ...selfUser, name: selfUser.name.replace(/ \(you\)$/, '') }] : [];
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [userSettings?.userName, userSettings?.userColor],
  );

  const renderFacepile = (users: Face[], size: 'sm' | 'md' | 'lg', max = 3) => {
    const shown = users.slice(0, max);
    const extra = users.length - shown.length;
    return (
      <span className={`ph-facepile ${size}`}>
        {shown.map((u, i) => (
          <span key={`${u.initials}-${i}`} className="ph-face" style={{ backgroundColor: u.color }} title={u.name}>
            {u.initials}
          </span>
        ))}
        {extra > 0 && <span className="ph-face more" title={`${extra} more`}>+{extra}</span>}
      </span>
    );
  };

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

  // Menus are plain groups of buttons, not ARIA menus: role="menu"
  // would require menuitem children and the full menu keyboard pattern,
  // which these action lists don't implement (WCAG 4.1.2).
  const renderProjectMenu = (item: ProjectItem) => (
    <div className="ph-menu">
      <button className="ph-menu-item strong" onClick={() => { closeAllMenus(); handleOpen(item); }}>
        Open
      </button>
      <div className="ph-menu-item ph-submenu-parent">
        <button
          className="ph-menu-item-inner"
          onClick={(e) => { e.stopPropagation(); setMoveSubmenuOpen((v) => v === 'move' ? false : 'move'); }}
        >
          Move to collection <span className="ph-submenu-arrow">▸</span>
        </button>
        {moveSubmenuOpen === 'move' && (
          <div className="ph-menu ph-submenu">
            {collections
              .filter((c) => !c.projectIds.includes(item.indexDocId.replace(/^automerge:/, '')))
              .map((collection) => (
                <button
                  key={collection.id}
                  className="ph-menu-item"
                  onClick={() => { requestMove(item.indexDocId, collection.id); closeAllMenus(); }}
                >
                  {collection.name}
                </button>
              ))}
            {collectionOf(item.indexDocId) && (
              <button
                className="ph-menu-item"
                onClick={() => { requestMove(item.indexDocId, null); closeAllMenus(); }}
              >
                No collection
              </button>
            )}
            <button
              className="ph-menu-item accent"
              onClick={() => openNewCollection(item.indexDocId)}
            >
              ＋ New collection…
            </button>
          </div>
        )}
      </div>
      <div className="ph-menu-item ph-submenu-parent">
        <button
          className="ph-menu-item-inner"
          onClick={(e) => { e.stopPropagation(); setMoveSubmenuOpen((v) => v === 'add' ? false : 'add'); }}
        >
          Add to collection <span className="ph-submenu-arrow">▸</span>
        </button>
        {moveSubmenuOpen === 'add' && (
          <div className="ph-menu ph-submenu">
            {collections
              .filter((c) => !c.projectIds.includes(item.indexDocId.replace(/^automerge:/, '')))
              .map((collection) => (
                <button
                  key={collection.id}
                  className="ph-menu-item"
                  onClick={() => { addToCollection(item.indexDocId, collection.id); closeAllMenus(); }}
                >
                  {collection.name}
                </button>
              ))}
            {collections.every((c) => c.projectIds.includes(item.indexDocId.replace(/^automerge:/, ''))) && (
              <div className="ph-menu-item ph-menu-subtext" style={{ cursor: 'default' }}>
                Already in every collection
              </div>
            )}
          </div>
        )}
      </div>
      <button
        className="ph-menu-item"
        disabled={!!duplicatingId}
        onClick={(e) => { e.stopPropagation(); openDuplicateDialog(item); }}
      >
        Duplicate
        <span className="ph-menu-subtext">Fork a fresh copy — no history carried over</span>
      </button>
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
      <button
        className="ph-menu-item"
        disabled={!!exportingId}
        onClick={(e) => { e.stopPropagation(); handleDownloadZip(item); }}
      >
        {exportingId === item.indexDocId ? 'Preparing ZIP…' : 'Download as ZIP'}
      </button>
      <button className="ph-menu-item" onClick={() => startRename(item)}>Rename…</button>
      <div className="ph-menu-divider" />
      <button className="ph-menu-item danger" onClick={() => handleRemove(item)}>
        Remove from this device
        <span className="ph-menu-subtext">Doesn't delete the project for others</span>
      </button>
    </div>
  );

  /** Refresh a peek summary via a short background connection. Contributors
   * are carried over: they only update on a real open, when presence data
   * is flowing. */
  const refreshPeek = async (item: ProjectItem) => {
    if (peekRefreshing || exportingId || duplicatingId) return;
    setPeekRefreshing(true);
    try {
      const files = await connect(resolveSyncServerUrl(item.syncServer), item.indexDocId);
      onUpdateProjectSummary?.(item.indexDocId, {
        fileCount: files.length,
        topFiles: files.slice(0, 5).map((f) => f.path),
        contributors: item.summary?.contributors ?? [],
        asOf: new Date().toISOString(),
      });
    } catch (err) {
      console.error('Peek refresh failed:', err);
    } finally {
      try { await disconnect(); } catch { /* connection already down */ }
      setPeekRefreshing(false);
    }
  };

  const renderPeek = (item: ProjectItem) => {
    const s = item.summary;
    return (
      <div className="qh-peek ph-peek" onMouseDown={(e) => e.stopPropagation()}>
        {s ? (
          <>
            <div className="ph-peek-header">
              {s.fileCount} {s.fileCount === 1 ? 'FILE' : 'FILES'} · AS OF {formatOpened(s.asOf).toUpperCase()}
            </div>
            {s.contributors.length > 0 && (
              <div className="ph-peek-people">
                {renderFacepile(
                  s.contributors.map((c) => ({ name: c.name, color: c.color, initials: initialsFor(c.name) })),
                  'lg', 3,
                )}
                <span className="ph-peek-people-label">
                  {s.contributors.length === 1
                    ? `${s.contributors[0].name} has joined`
                    : `${s.contributors.map((c) => c.name.split(' ')[0]).join(', ')} have joined`}
                </span>
              </div>
            )}
            <div className="ph-peek-files">
              {s.topFiles.map((f) => (
                <div key={f} className="ph-peek-file mono">{f}</div>
              ))}
              {s.fileCount > s.topFiles.length && (
                <div className="ph-peek-file more">and {s.fileCount - s.topFiles.length} more…</div>
              )}
            </div>
          </>
        ) : (
          <>
            <div className="ph-peek-header">NOT OPENED ON THIS DEVICE YET</div>
            <div className="ph-peek-note">
              Details are cached when you open a project — or load them now without opening it.
            </div>
          </>
        )}
        <div className="ph-peek-row">
          <span className="mono">{serverHost(item.syncServer)} · {shortId(item.indexDocId)}</span>
        </div>
        <div className="ph-peek-divider" />
        <div className="ph-peek-actions">
          <button className="ph-link" onClick={() => refreshPeek(item)}>
            {peekRefreshing ? 'Refreshing…' : s ? 'Refresh' : 'Load details'}
          </button>
        </div>
        <div className="ph-peek-footnote">Peeking is read-only — use the ⋯ menu to act on the project.</div>
      </div>
    );
  };

  // ---- collection sharing (real synced documents) ----

  /** Items on a collection come from its own document's entries — a shared
   * collection can hold projects this user has never opened. */
  const collectionItemsOf = (collection: CollectionView): ProjectItem[] =>
    collection.entries.map((e) => ({
      indexDocId: e.indexDocId,
      syncServer: e.syncServer,
      description: e.description,
      addedAt: e.addedAt,
      lastAccessed: e.lastAccessed,
      summary: e.summary,
    }));

  /** Invite = the collection document's id + server. Nothing else travels. */
  const buildInviteUrl = (collection: CollectionView): string =>
    buildFullUrl({
      type: 'join-collection',
      collectionId: collection.id,
      collectionName: collection.name,
      inviter: userSettings?.userName ?? 'A collaborator',
      syncServer: collection.syncServer,
      entries: [],
    });

  /** People seen on a collection: you plus the contributor union from the
   * projects' cached summaries. Derived, not a stored member list. */
  const peopleOn = (collection: CollectionView): Face[] => {
    const people: Face[] = selfUser
      ? [{ ...selfUser, name: selfUser.name.replace(/ \(you\)$/, '') }]
      : [];
    for (const e of collection.entries) {
      for (const c of e.summary?.contributors ?? []) {
        if (!people.some((p) => p.name === c.name)) {
          people.push({ name: c.name, color: c.color, initials: initialsFor(c.name) });
        }
      }
    }
    return people;
  };

  const renderMembersPopover = (collection: CollectionView) => {
    const people = peopleOn(collection);
    const inviteUrl = buildInviteUrl(collection);
    const copyKey = `collection:${collection.id}:invite`;
    return (
      <div className="ph-menu ph-members" role="dialog" aria-label={`People on ${collection.name}`}>
        <div className="ph-menu-label">
          {people.length <= 1
            ? 'ONLY YOU SO FAR'
            : `${people.length} PEOPLE SEEN ON THESE PROJECTS`}
        </div>
        <div className="ph-members-list">
          {people.map((m, i) => (
            <div key={`${m.initials}-${i}`} className="ph-member-row">
              <span className="ph-face lg" style={{ backgroundColor: m.color }}>{m.initials}</span>
              <span className="ph-member-name">
                {m.name}
                {i === 0 && selfUser && <span className="ph-member-you"> (you)</span>}
              </span>
            </div>
          ))}
        </div>
        <div className="ph-menu-divider" />
        <div className="ph-menu-label">INVITE BY LINK</div>
        <div className="ph-members-invite">
          <span className="ph-invite-url mono" title={inviteUrl}>{inviteUrl.replace(/^https?:\/\//, '').slice(0, 34)}…</span>
          <button
            className="ph-btn primary small-invite"
            onClick={() => copyToClipboard(inviteUrl, copyKey)}
          >
            {copied === copyKey ? 'Copied!' : 'Copy link'}
          </button>
        </div>
        <div className="ph-invite-note">
          Anyone with this link can join this collection and add or remove projects.
          Its contents sync to them for real.
        </div>
      </div>
    );
  };

  const renderCard = (item: ProjectItem) => (
    <div
      key={item.indexDocId}
      className={`ph-card qh-menu-anchor ${draggingId === item.indexDocId ? 'dragging' : ''} ${peekFor === item.indexDocId ? 'peek-open' : ''}`}
      draggable
      onDragStart={handleDragStart(item)}
      onDragEnd={handleDragEnd}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        setMoveSubmenuOpen(false);
        setOpenMenu(openMenu === item.indexDocId ? null : item.indexDocId);
      }}
    >
      <button className="ph-card-body" onClick={() => handleOpen(item)} title={item.description}>
        <span className={`ph-card-name ${isUnnamed(item.description) ? 'unnamed' : ''}`}>
          {item.description}
        </span>
        <span className="ph-card-footer">
          <span className="ph-card-meta">
            {item.summary ? `${item.summary.fileCount} ${item.summary.fileCount === 1 ? 'file' : 'files'} · ` : ''}
            opened {formatOpened(item.lastAccessed)}
          </span>
          {renderFacepile(contributorsFor(item), 'sm')}
        </span>
      </button>
      <span className="ph-card-actions">
        <span
          className="ph-peek-anchor"
          onMouseOver={() => openPeekHover(item.indexDocId)}
          onMouseOut={closePeekHoverSoon}
        >
          <button
            className="ph-peek-btn"
            title="Peek — see what's inside"
            onClick={(e) => { e.stopPropagation(); openPeekHover(item.indexDocId); }}
          >
            {peekIcon}
          </button>
          {peekFor === item.indexDocId && renderPeek(item)}
        </span>
        <button
          className="ph-fork-btn"
          title={`Duplicate "${item.description}" (fork a fresh copy)`}
          disabled={!!duplicatingId}
          onClick={(e) => { e.stopPropagation(); openDuplicateDialog(item); }}
        >
          {forkIcon}
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
      </span>
      {openMenu === item.indexDocId && renderProjectMenu(item)}
    </div>
  );

  const renderCollection = (collection: CollectionView) => {
    // Default collection order is by recency (lastAccessed, newest first), not
    // by stored position — paging walks toward older projects, per the design;
    // the header's sort button switches to oldest-first or by name. True
    // "recent edits" ordering needs automerge-history attribution (Future
    // phase); last-opened is the closest signal in today's metadata.
    const collectionSort = collectionSorts[collection.id] ?? 'newest';
    const collectionItems = sortProjectItems(collectionItemsOf(collection).filter(matches), collectionSort);
    if (query && collectionItems.length === 0) return null;
    const pageCount = Math.max(1, Math.ceil(collectionItems.length / COLLECTION_PAGE_SIZE));
    const page = Math.min(collectionPages[collection.id] ?? 0, pageCount - 1);
    const pageItems = collectionItems.slice(page * COLLECTION_PAGE_SIZE, (page + 1) * COLLECTION_PAGE_SIZE);
    const menuKey = `collection:${collection.id}`;
    const sortMenuKey = `sort:${collection.id}`;
    return (
      <section
        key={collection.id}
        className={`ph-collection ${dropTarget === collection.id ? 'drop-target' : ''}`}
        {...dropZoneProps(collection.id)}
      >
        <div className="ph-collection-header qh-menu-anchor">
          <span className="ph-collection-name">{collection.name}</span>
          <span className="ph-collection-count">{collectionItems.length}</span>
          {(() => {
            const people = peopleOn(collection);
            const hasOthers = people.length > 1;
            return (
              <button
                className={`ph-collection-people ${hasOthers ? '' : 'private'}`}
                title={hasOthers
                  ? `${people.length} people seen on these projects — people & invite`
                  : 'Only you so far. Click to invite others.'}
                onClick={(e) => {
                  e.stopPropagation();
                  setOpenMenu(null);
                  setMembersFor(membersFor === collection.id ? null : collection.id);
                }}
              >
                {hasOthers && (
                  <svg width="12" height="12" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                    <circle cx="9" cy="8" r="3.4" stroke="currentColor" strokeWidth="2" />
                    <path d="M3 19c0-3 2.7-4.8 6-4.8s6 1.8 6 4.8" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
                    <circle cx="17" cy="9" r="2.6" stroke="currentColor" strokeWidth="2" />
                    <path d="M16.5 14.4c2.6.3 4.5 1.9 4.5 4.1" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
                  </svg>
                )}
                {renderFacepile(people, 'md', 3)}
              </button>
            );
          })()}
          <span className="ph-flex-spacer" />
          <span className="ph-collection-sort-anchor">
            <button
              className={`ph-icon-btn ph-collection-sort-btn ${collectionSort !== 'newest' ? 'active' : ''}`}
              title={`Sort collection (${sortOrderLabel(collectionSort)})`}
              onClick={(e) => {
                e.stopPropagation();
                setMembersFor(null);
                setOpenMenu(openMenu === sortMenuKey ? null : sortMenuKey);
              }}
            >
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <path d="M7 4v14M7 18l-3.5-3.5M7 18l3.5-3.5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
                <path d="M17 20V6M17 6l-3.5 3.5M17 6l3.5 3.5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
            {openMenu === sortMenuKey && (
              <div className="ph-menu ph-menu-right">
                {(['newest', 'oldest', 'name'] as SortOrder[]).map((o) => (
                  <button
                    key={o}
                    className={`ph-menu-item ${collectionSort === o ? 'strong' : ''}`}
                    onClick={() => {
                      setCollectionSorts((s) => ({ ...s, [collection.id]: o }));
                      setCollectionPages((p) => ({ ...p, [collection.id]: 0 }));
                      setOpenMenu(null);
                    }}
                  >
                    {o === 'newest' ? 'Newest first' : o === 'oldest' ? 'Oldest first' : 'A to Z'}
                  </button>
                ))}
              </div>
            )}
          </span>
          <button
            className="ph-icon-btn"
            title="Collection actions"
            onClick={(e) => {
              e.stopPropagation();
              setMembersFor(null);
              setOpenMenu(openMenu === menuKey ? null : menuKey);
            }}
          >
            ⋯
          </button>
          {membersFor === collection.id && renderMembersPopover(collection)}
          {openMenu === menuKey && (
            <div className="ph-menu ph-menu-right">
              <button
                className="ph-menu-item"
                onClick={() => { setOpenMenu(null); setMembersFor(collection.id); }}
              >
                People &amp; invite…
              </button>
              <button
                className="ph-menu-item"
                onClick={() => {
                  setRenameCollectionTarget(collection);
                  setRenameCollectionValue(collection.name);
                  closeAllMenus();
                }}
              >
                Rename collection…
                <span className="ph-menu-subtext">Renames it for everyone subscribed</span>
              </button>
              <div className="ph-menu-divider" />
              <button
                className="ph-menu-item danger"
                onClick={() => {
                  closeAllMenus();
                  setConfirmState({
                    title: `Leave "${collection.name}"?`,
                    body: "Removes it from your view only — anyone else subscribed keeps it, and projects you've opened stay in your list.",
                    confirmLabel: 'Leave collection',
                    action: () => { onUnsubscribeCollection?.(collection.id); },
                  });
                }}
              >
                Leave collection
                <span className="ph-menu-subtext">Removes it from your view only</span>
              </button>
            </div>
          )}
        </div>
        {collectionItems.length === 0 ? (
          <div className="ph-collection-empty">Empty collection — drag a project here, or use its ⋯ menu.</div>
        ) : (
          <div className="ph-collection-row">
            {page > 0 && (
              <button
                className="ph-pager"
                title={collectionSort === 'newest' ? 'Newer projects' : collectionSort === 'oldest' ? 'Older projects' : 'Previous page'}
                onClick={() => setCollectionPages((p) => ({ ...p, [collection.id]: page - 1 }))}
              >
                ‹
              </button>
            )}
            <div className="ph-card-grid">{pageItems.map(renderCard)}</div>
            {page < pageCount - 1 ? (
              <button
                className="ph-pager"
                title={collectionSort === 'newest' ? 'Older projects' : collectionSort === 'oldest' ? 'Newer projects' : 'Next page'}
                onClick={() => setCollectionPages((p) => ({ ...p, [collection.id]: page + 1 }))}
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
              <div className="ph-menu ph-menu-right">
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
              style={authPicture ? undefined : { backgroundColor: userSettings?.userColor ?? 'var(--posit-blue)' }}
              onClick={() => setAvatarMenuOpen((v) => !v)}
              title={userSettings?.userName}
            >
              {authPicture ? (
                <img src={authPicture} alt="" className="ph-avatar-img" referrerPolicy="no-referrer" />
              ) : (
                initialsFor(userSettings?.userName ?? '')
              )}
            </button>
            {avatarMenuOpen && userSettings && (
              <div className="ph-menu ph-menu-right ph-avatar-menu">
                <div className="ph-avatar-menu-id">
                  <span className="ph-avatar big" style={authPicture ? undefined : { backgroundColor: userSettings.userColor }}>
                    {authPicture ? (
                      <img src={authPicture} alt="" className="ph-avatar-img" referrerPolicy="no-referrer" />
                    ) : (
                      initialsFor(userSettings.userName)
                    )}
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
                <button className="ph-menu-item" onClick={handleExportJson}>Export project list (JSON)…</button>
                <button className="ph-menu-item" onClick={handleImportJson}>Import project list (JSON)…</button>
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

      <main id="main-content" tabIndex={-1} className="ph-main">
        {items.length === 0 && collections.every((c) => c.entries.length === 0) ? (
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
            {collections.map(renderCollection)}

            <div className="ph-new-collection-row">
              <button className="ph-btn ghost-accent" onClick={() => openNewCollection()}>＋ New collection</button>
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
                  {query ? 'No projects match your search.' : 'Everything is in a collection.'}
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
                      onContextMenu={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        setMoveSubmenuOpen(false);
                        setOpenMenu(openMenu === item.indexDocId ? null : item.indexDocId);
                      }}
                    >
                      <button
                        className={`ph-row-name ${isUnnamed(item.description) ? 'unnamed' : ''}`}
                        onClick={() => handleOpen(item)}
                        title={item.description}
                      >
                        {item.description}
                      </button>
                      {isUnnamed(item.description) && (
                        <button className="ph-link" onClick={() => startRename(item)}>Rename</button>
                      )}
                      <span className="ph-row-meta">
                        {item.summary ? `${item.summary.fileCount} ${item.summary.fileCount === 1 ? 'file' : 'files'} · ` : ''}
                        opened {formatOpened(item.lastAccessed)}
                      </span>
                      <span
                        className="ph-peek-anchor"
                        onMouseOver={() => openPeekHover(item.indexDocId)}
                        onMouseOut={closePeekHoverSoon}
                      >
                        <button
                          className="ph-icon-btn ph-peek-btn"
                          title="Peek — see what's inside"
                          onClick={(e) => { e.stopPropagation(); openPeekHover(item.indexDocId); }}
                        >
                          {peekIcon}
                        </button>
                        {peekFor === item.indexDocId && renderPeek(item)}
                      </span>
                      <button
                        className="ph-icon-btn ph-fork-btn"
                        title={`Duplicate "${item.description}" (fork a fresh copy)`}
                        disabled={!!duplicatingId}
                        onClick={(e) => { e.stopPropagation(); openDuplicateDialog(item); }}
                      >
                        {forkIcon}
                      </button>
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
                    </div>
                  ))}
                </div>
              )}
            </section>
          </>
        )}
      </main>

      {/* Duplicate (fork) */}
      {duplicateFor && (
        <div className="ph-dialog-backdrop" onMouseDown={() => { if (!duplicatingId) setDuplicateFor(null); }}>
          <div className="ph-dialog" role="dialog" aria-modal="true" aria-labelledby="ph-dialog-title-duplicate" onMouseDown={(e) => e.stopPropagation()}>
            <h2 id="ph-dialog-title-duplicate">Duplicate "{duplicateFor.description}"</h2>
            <p className="ph-dialog-hint">
              A fresh copy of all {duplicateFor.summary ? `${duplicateFor.summary.fileCount} ` : ''}files — no
              edit history carries over.
            </p>
            {formError && <div className="ph-error inline">{formError}</div>}
            <form onSubmit={(e) => { e.preventDefault(); handleDuplicate(); }}>
              <label className="ph-field-label" htmlFor="ph-dup-name">Name</label>
              <input
                id="ph-dup-name"
                className="ph-input focus-accent"
                value={duplicateName}
                onChange={(e) => setDuplicateName(e.target.value)}
                autoFocus
                onFocus={(e) => e.target.select()}
              />
              <label className="ph-field-label" htmlFor="ph-dup-collection">Add to collection</label>
              <select
                id="ph-dup-collection"
                className="ph-input"
                value={duplicateCollectionId}
                onChange={(e) => setDuplicateCollectionId(e.target.value)}
              >
                <option value="">No collection</option>
                {collections.map((c) => (
                  <option key={c.id} value={c.id}>{c.name}</option>
                ))}
              </select>
              <div className="ph-dialog-actions">
                <button type="button" className="ph-btn outline" disabled={!!duplicatingId} onClick={() => setDuplicateFor(null)}>
                  Cancel
                </button>
                <button type="submit" className="ph-btn primary" disabled={!!duplicatingId || !duplicateName.trim()}>
                  {duplicatingId ? 'Duplicating…' : 'Duplicate'}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* New collection */}
      {newCollectionDialog && (
        <div className="ph-dialog-backdrop" onMouseDown={() => setNewCollectionDialog(null)}>
          <div className="ph-dialog" role="dialog" aria-modal="true" aria-labelledby="ph-dialog-title-new-collection" onMouseDown={(e) => e.stopPropagation()}>
            <h2 id="ph-dialog-title-new-collection">New collection</h2>
            {newCollectionDialog.forProject && (
              <p className="ph-dialog-hint">The project will be moved onto it.</p>
            )}
            <form onSubmit={(e) => { e.preventDefault(); commitNewCollection(); }}>
              <label className="ph-field-label" htmlFor="ph-new-collection-name">Name</label>
              <input
                id="ph-new-collection-name"
                className="ph-input focus-accent"
                value={newCollectionName}
                onChange={(e) => setNewCollectionName(e.target.value)}
                placeholder="e.g. Board prep"
                autoFocus
              />
              <div className="ph-dialog-actions">
                <button type="button" className="ph-btn outline" onClick={() => setNewCollectionDialog(null)}>Cancel</button>
                <button type="submit" className="ph-btn primary" disabled={!newCollectionName.trim()}>Create</button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Rename collection */}
      {renameCollectionTarget && (
        <div className="ph-dialog-backdrop" onMouseDown={() => setRenameCollectionTarget(null)}>
          <div className="ph-dialog" role="dialog" aria-modal="true" aria-labelledby="ph-dialog-title-rename-collection" onMouseDown={(e) => e.stopPropagation()}>
            <h2 id="ph-dialog-title-rename-collection">Rename collection</h2>
            <p className="ph-dialog-hint">Renames it for everyone subscribed to it.</p>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                if (renameCollectionValue.trim()) {
                  onRenameCollection?.(renameCollectionTarget.id, renameCollectionValue.trim());
                }
                setRenameCollectionTarget(null);
              }}
            >
              <label className="ph-field-label" htmlFor="ph-rename-collection">Name</label>
              <input
                id="ph-rename-collection"
                className="ph-input focus-accent"
                value={renameCollectionValue}
                onChange={(e) => setRenameCollectionValue(e.target.value)}
                autoFocus
              />
              <div className="ph-dialog-actions">
                <button type="button" className="ph-btn outline" onClick={() => setRenameCollectionTarget(null)}>Cancel</button>
                <button type="submit" className="ph-btn primary" disabled={!renameCollectionValue.trim()}>Rename</button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* Generic destructive confirmation */}
      {confirmState && (
        <div className="ph-dialog-backdrop" onMouseDown={() => setConfirmState(null)}>
          <div className="ph-dialog" role="dialog" aria-modal="true" aria-labelledby="ph-dialog-title-confirm" onMouseDown={(e) => e.stopPropagation()}>
            <h2 id="ph-dialog-title-confirm">{confirmState.title}</h2>
            <p className="ph-dialog-hint">{confirmState.body}</p>
            <div className="ph-dialog-actions">
              <button type="button" className="ph-btn outline" onClick={() => setConfirmState(null)} autoFocus>
                Cancel
              </button>
              <button
                type="button"
                className="ph-btn danger"
                onClick={() => { confirmState.action(); setConfirmState(null); }}
              >
                {confirmState.confirmLabel}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Shared-collection move warning */}
      {pendingMove && (
        <div className="ph-dialog-backdrop" onMouseDown={() => setPendingMove(null)}>
          <div className="ph-dialog" role="dialog" aria-modal="true" aria-labelledby="ph-dialog-title-move" onMouseDown={(e) => e.stopPropagation()}>
            <h2 id="ph-dialog-title-move">Move "{pendingMove.name}" out of {pendingMove.fromName}?</h2>
            <p className="ph-dialog-hint">
              Please note you're changing {pendingMove.othersCount === 1
                ? "another person's"
                : `${pendingMove.othersCount} other people's`} view of this collection — it will
              no longer appear in {pendingMove.fromName} for them. The project itself isn't
              deleted or changed.
            </p>
            <label className="ph-checkbox-row">
              <input
                type="checkbox"
                checked={moveWarnChecked}
                onChange={(e) => setMoveWarnChecked(e.target.checked)}
              />
              Don't show this again
            </label>
            <div className="ph-dialog-actions">
              <button type="button" className="ph-btn outline" onClick={() => setPendingMove(null)}>
                Cancel
              </button>
              <button
                type="button"
                className="ph-btn primary"
                onClick={() => {
                  if (moveWarnChecked) localStorage.setItem(MOVE_WARNING_KEY, '1');
                  moveProject(pendingMove.indexDocId, pendingMove.target);
                  setPendingMove(null);
                }}
                autoFocus
              >
                Move it
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Rename dialog */}
      {renameFor && (
        <div className="ph-dialog-backdrop" onMouseDown={() => setRenameFor(null)}>
          <div className="ph-dialog" role="dialog" aria-modal="true" aria-labelledby="ph-dialog-title-rename-project" onMouseDown={(e) => e.stopPropagation()}>
            <h2 id="ph-dialog-title-rename-project">Rename project</h2>
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
          <div className="ph-dialog" role="dialog" aria-modal="true" aria-labelledby="ph-dialog-title-new-choice" onMouseDown={(e) => e.stopPropagation()}>
            <h2 id="ph-dialog-title-new-choice">New {newDialogChoice.name.toLowerCase()}</h2>
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
              <label className="ph-field-label" htmlFor="ph-new-collection">Add to collection (optional)</label>
              <select
                id="ph-new-collection"
                className="ph-input"
                value={newCollectionId}
                onChange={(e) => setNewCollectionId(e.target.value)}
              >
                <option value="">No collection</option>
                {collections.map((s) => (
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
          <div className="ph-dialog wide" role="dialog" aria-modal="true" aria-labelledby="ph-dialog-title-add-existing" onMouseDown={(e) => e.stopPropagation()}>
            <h2 id="ph-dialog-title-add-existing">Add an existing project</h2>
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
        <span className="ph-footer-note">collections UI exploration</span>
      </footer>
    </div>
  );
}
