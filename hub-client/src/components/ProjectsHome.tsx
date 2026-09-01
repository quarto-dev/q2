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
import { FilePlusIcon, ForkIcon, PeekIcon, PeopleIcon, SortIcon } from './icons';
import { Menu, MenuItem, MenuDivider, MenuLabel, MenuSubmenu } from './Menu';
import Tooltip from './Tooltip';
import ModalDialog from './ModalDialog';
import { common } from '../strings';
import { sortProjectItems, sortOrderLabel, type SortOrder } from '../utils/projectSort';
import { buildProjectListExport, parseProjectListImport } from '../services/projectListExport';
import type { Face } from '../utils/facepile';
import type { CollectionSnapshot } from '../services/projectSetService';
import './ProjectsHome.css';

interface Props {
  onSelectProject: (project: ProjectEntry, filePathOverride?: string) => void;
  error?: string | null;
  /** Re-attempt a failed project open; renders a "Try again" recovery
   *  action on the connection error banner. */
  onRetry?: () => void;
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
  /** Import a project-list JSON export into the root set (import + reconcile,
   * so the projects appear without a reload). Absent in legacy mode. */
  onImportProjects?: (json: string) => Promise<{ imported: number; reconciled: number; connected: boolean }>;
  /** Subscribe to an existing collection document (used by import to restore
   * collection subscriptions from a v5 export's pointers). */
  onSubscribeCollection?: (projectSetDocId: string, syncServer: string) => Promise<void>;
  onRenameProject?: (indexDocId: string, description: string) => void;
  /** Replace a project's cached peek summary (used by Peek's refresh). */
  onUpdateProjectSummary?: (indexDocId: string, summary: ProjectSetEntrySummary) => void;
  /** All connected collections, root first (from useCollectionSets). */
  collections?: CollectionSnapshot[];
  /**
   * Sort this collection's section to the top (bd-fxdcxbpq): set after
   * joining a collection via an invite so the invitee lands looking at
   * what they just accepted.
   */
  promoteCollectionId?: string;
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

/** Fork glyph for the duplicate affordance. */
const forkIcon = <ForkIcon />;

/** Magnifying glass for the hover-to-peek affordance. */
const peekIcon = <PeekIcon />;

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
  error: connectionError,
  onRetry,
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
  onImportProjects,
  onSubscribeCollection,
  onRenameProject,
  onUpdateProjectSummary,
  collections: collectionsProp,
  promoteCollectionId,
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
  // A failed legacy load surfaces as an error state with retry (Phase 3)
  // instead of silently falling through to the "No projects yet" empty
  // copy. loadAttempt re-triggers the load effect.
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loadAttempt, setLoadAttempt] = useState(0);

  const [search, setSearch] = useState('');
  const searchRef = useRef<HTMLInputElement>(null);

  // Menus / popovers. openMenu identifies the ⋯ menu by project id or
  // `collection:<id>`; submenus and the peek popover are tracked separately.
  const [openMenu, setOpenMenu] = useState<string | null>(null);

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
  const collectionViews: CollectionView[] = useMemo(() => {
    const views = (collectionsProp ?? [])
      .filter((c) => !c.isRoot)
      .map((c) => ({
        id: c.docId,
        name: c.name ?? 'Untitled collection',
        syncServer: c.syncServer,
        entries: c.entries,
        projectIds: c.entries.map((e) => e.indexDocId.replace(/^automerge:/, '')),
      }));
    // A just-joined collection sorts to the top (stable otherwise) so the
    // invitee lands looking at what they accepted (bd-fxdcxbpq).
    if (promoteCollectionId) {
      const bare = promoteCollectionId.replace(/^automerge:/, '');
      views.sort((a, b) =>
        Number(b.id.replace(/^automerge:/, '') === bare) -
        Number(a.id.replace(/^automerge:/, '') === bare));
    }
    return views;
  }, [collectionsProp, promoteCollectionId]);
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
        if (!cancelled) {
          setLegacyProjects(entries);
          setLoadError(null);
        }
      } catch (err) {
        console.error('Failed to load projects:', err);
        if (!cancelled) {
          setLoadError(err instanceof Error ? err.message : String(err));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => { cancelled = true; };
  }, [useProjectSet, loadAttempt]);

  const handleRetryLoad = () => {
    setLoading(true);
    setLoadError(null);
    setLoadAttempt((n) => n + 1);
  };

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
    // Carry the cached peek summary along with the identity fields:
    // dropping it here left collection copies with no file counts until
    // the project's next open, so invite previews showed "0 files" for
    // populated projects (bd-fxdcxbpq follow-up).
    const item = byId.get(indexDocId)
      ?? byId.get(indexDocId.replace(/^automerge:/, ''))
      ?? byId.get(`automerge:${indexDocId}`);
    if (item) {
      return {
        indexDocId: item.indexDocId,
        syncServer: item.syncServer,
        description: item.description,
        ...(item.summary && { summary: item.summary }),
      };
    }
    for (const c of collections) {
      const e = c.entries.find((en) => en.indexDocId.replace(/^automerge:/, '') === indexDocId.replace(/^automerge:/, ''));
      if (e) {
        return {
          indexDocId: e.indexDocId,
          syncServer: e.syncServer,
          description: e.description,
          ...(e.summary && { summary: e.summary }),
        };
      }
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

  // The item being opened, shown as an italic "opening..." beside its
  // name (replaces the old global "Connecting to sync server…" banner).
  // Success unmounts this view; failure surfaces connectionError, which
  // clears the marker below.
  const [openingId, setOpeningId] = useState<string | null>(null);
  useEffect(() => {
    if (connectionError) setOpeningId(null);
  }, [connectionError]);

  const handleOpen = useCallback(async (item: ProjectItem) => {
    setOpeningId(item.indexDocId);
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
    // Collections are synced docs; the export records their pointers so a
    // restoring browser can re-subscribe (root included for completeness).
    const json = buildProjectListExport(items, collectionsProp ?? []);
    const blob = new Blob([json], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'quarto-hub-projects.json';
    a.click();
    URL.revokeObjectURL(url);
  }, [items, collectionsProp]);

  const handleImportJson = useCallback(() => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = 'application/json';
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0];
      if (!file) return;
      try {
        const json = await file.text();
        if (onImportProjects) {
          // Set mode: import + reconcile so the projects appear immediately.
          const parsed = parseProjectListImport(json);
          const { imported, reconciled, connected } = await onImportProjects(json);

          // Restore collection subscriptions from the export's pointers (v5+).
          // Never the root (this browser has its own), never ones already
          // subscribed; membership arrives with sync, so nothing is written.
          let joined = 0;
          let failedCollections = 0;
          if (onSubscribeCollection) {
            const have = new Set((collectionsProp ?? []).map((c) => c.docId));
            for (const c of parsed.collections) {
              if (c.isRoot || have.has(c.projectSetDocId)) continue;
              try {
                await onSubscribeCollection(c.projectSetDocId, c.syncServer);
                joined++;
              } catch (err) {
                console.error(`Failed to subscribe to collection ${c.projectSetDocId}:`, err);
                failedCollections++;
              }
            }
          }

          let msg: string;
          if (!connected) {
            msg = `Saved ${imported} project(s) — they'll appear when the sync connection is restored`;
          } else if (reconciled > 0) {
            msg = `Imported ${reconciled} project(s)`;
          } else {
            msg = 'All projects were already in your list';
          }
          if (joined > 0) msg += `, joined ${joined} collection(s)`;
          if (failedCollections > 0) msg += `; ${failedCollections} collection(s) could not be joined`;
          alert(msg);
        } else {
          const count = await projectStorage.importProjects(json);
          alert(`Imported ${count} project(s)`);
        }
      } catch (err) {
        console.error('Failed to import:', err);
        alert('Failed to import projects. Invalid JSON format.');
      }
    };
    input.click();
  }, [onImportProjects, onSubscribeCollection, collectionsProp]);

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
      <span className={`qh-facepile ${size}`}>
        {shown.map((u, i) => (
          <Tooltip key={`${u.initials}-${i}`} content={u.name}>
            <span className="qh-face" style={{ backgroundColor: u.color }}>
              {u.initials}
            </span>
          </Tooltip>
        ))}
        {extra > 0 && (
          <Tooltip content={`${extra} more`}>
            <span className="qh-face more">+{extra}</span>
          </Tooltip>
        )}
      </span>
    );
  };

  // ---- rendering ----

  if (loading || projectSetConnecting) {
    // Skeleton grid shaped like the loaded page (Phase 5); role="status"
    // keeps the Phase 3 polite loading announcement for screen readers.
    return (
      <div className="projects-home">
        <main
          className="qh-main qh-skeleton-page"
          role="status"
          aria-label={projectSetConnecting ? 'Connecting to project set…' : 'Loading projects…'}
        >
          <div className="qh-skeleton qh-skeleton-heading" aria-hidden="true" />
          <div className="qh-card-grid" aria-hidden="true">
            {Array.from({ length: 8 }, (_, i) => (
              <div key={i} className="qh-card qh-skeleton-card">
                <div className="qh-skeleton qh-skeleton-line" style={{ width: '60%' }} />
                <div className="qh-skeleton qh-skeleton-line-sm" style={{ width: '40%' }} />
              </div>
            ))}
          </div>
        </main>
      </div>
    );
  }

  // The shared APG menu primitive (components/Menu.tsx): arrow-key nav,
  // type-ahead, Escape with focus return, submenus.
  const renderProjectMenu = (item: ProjectItem) => (
    <Menu onClose={() => closeAllMenus()} ignoreOutsideSelector=".qh-menu-anchor, .qh-peek" aria-label={`Actions for ${item.description}`}>
      <MenuItem strong onSelect={() => { closeAllMenus(); handleOpen(item); }}>
        Open
      </MenuItem>
      <MenuSubmenu label="Move to collection">
        {collections
          .filter((c) => !c.projectIds.includes(item.indexDocId.replace(/^automerge:/, '')))
          .map((collection) => (
            <MenuItem
              key={collection.id}
              onSelect={() => { requestMove(item.indexDocId, collection.id); closeAllMenus(); }}
            >
              {collection.name}
            </MenuItem>
          ))}
        {collectionOf(item.indexDocId) && (
          <MenuItem onSelect={() => { requestMove(item.indexDocId, null); closeAllMenus(); }}>
            No collection
          </MenuItem>
        )}
        <MenuItem accent onSelect={() => openNewCollection(item.indexDocId)}>
          ＋ New collection…
        </MenuItem>
      </MenuSubmenu>
      <MenuSubmenu label="Add to collection">
        {collections
          .filter((c) => !c.projectIds.includes(item.indexDocId.replace(/^automerge:/, '')))
          .map((collection) => (
            <MenuItem
              key={collection.id}
              onSelect={() => { addToCollection(item.indexDocId, collection.id); closeAllMenus(); }}
            >
              {collection.name}
            </MenuItem>
          ))}
        {collections.every((c) => c.projectIds.includes(item.indexDocId.replace(/^automerge:/, ''))) && (
          <div className="qh-menu-item qh-menu-subtext" role="presentation" style={{ cursor: 'default' }}>
            Already in every collection
          </div>
        )}
      </MenuSubmenu>
      <MenuItem
        disabled={!!duplicatingId}
        subtext="Fork a fresh copy — no history carried over"
        onSelect={() => openDuplicateDialog(item)}
      >
        Duplicate
      </MenuItem>
      <MenuItem
        keepOpen
        onSelect={() => copyToClipboard(
          buildShareableUrl(item.indexDocId, item.syncServer, item.description, 'index.qmd', {
            from: userSettings?.userName,
            preview: item.summary
              ? {
                  kind: 'document',
                  fileName: 'index.qmd',
                  topFiles: item.summary.topFiles.filter((f) => f !== 'index.qmd').slice(0, 2),
                  fileCount: item.summary.fileCount,
                  contributorInitials: item.summary.contributors
                    .map((c) => initialsFor(c.name))
                    .slice(0, 4),
                }
              : undefined,
          }),
          item.indexDocId + ':share',
        )}
      >
        {copied === item.indexDocId + ':share' ? 'Link copied!' : 'Share link…'}
      </MenuItem>
      <MenuItem
        keepOpen
        hint={<span className="mono">{shortId(item.indexDocId)}</span>}
        onSelect={() => copyToClipboard(item.indexDocId.replace(/^automerge:/, ''), item.indexDocId + ':id')}
      >
        {copied === item.indexDocId + ':id' ? 'ID copied!' : 'Copy project ID'}
      </MenuItem>
      <MenuItem
        disabled={!!exportingId}
        onSelect={() => handleDownloadZip(item)}
      >
        {exportingId === item.indexDocId ? 'Preparing ZIP…' : 'Download as ZIP'}
      </MenuItem>
      <MenuItem onSelect={() => startRename(item)}>Rename…</MenuItem>
      <MenuDivider />
      <MenuItem
        danger
        subtext="Doesn't delete the project for others"
        onSelect={() => handleRemove(item)}
      >
        Remove from this device
      </MenuItem>
    </Menu>
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
      <div className="qh-peek qh-peek" onMouseDown={(e) => e.stopPropagation()}>
        {s ? (
          <>
            <div className="qh-peek-header">
              {s.fileCount} {s.fileCount === 1 ? 'FILE' : 'FILES'} · AS OF {formatOpened(s.asOf).toUpperCase()}
            </div>
            {s.contributors.length > 0 && (
              <div className="qh-peek-people">
                {renderFacepile(
                  s.contributors.map((c) => ({ name: c.name, color: c.color, initials: initialsFor(c.name) })),
                  'lg', 3,
                )}
                <span className="qh-peek-people-label">
                  {s.contributors.length === 1
                    ? `${s.contributors[0].name} has joined`
                    : `${s.contributors.map((c) => c.name.split(' ')[0]).join(', ')} have joined`}
                </span>
              </div>
            )}
            <div className="qh-peek-files">
              {s.topFiles.map((f) => (
                <div key={f} className="qh-peek-file mono qh-truncate">{f}</div>
              ))}
              {s.fileCount > s.topFiles.length && (
                <div className="qh-peek-file more">and {s.fileCount - s.topFiles.length} more…</div>
              )}
            </div>
          </>
        ) : (
          <>
            <div className="qh-peek-header">NOT OPENED ON THIS DEVICE YET</div>
            <div className="qh-peek-note">
              Details are cached when you open a project — or load them now without opening it.
            </div>
          </>
        )}
        <div className="qh-peek-row">
          <span className="mono">{serverHost(item.syncServer)} · {shortId(item.indexDocId)}</span>
        </div>
        <div className="qh-peek-divider" />
        <div className="qh-peek-actions">
          <button className="qh-link" onClick={() => refreshPeek(item)}>
            {peekRefreshing ? 'Refreshing…' : s ? 'Refresh' : 'Load details'}
          </button>
        </div>
        <div className="qh-peek-footnote">Peeking is read-only — use the ⋯ menu to act on the project.</div>
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

  /**
   * Invite = the collection document's id + server, plus (bd-fxdcxbpq) a
   * display-only preview= built from the cached peek summaries. Joining
   * lands on the home screen with the collection promoted to the top —
   * the invite is to the collection, so no start= document is emitted
   * (the parser still tolerates start= on links already in the wild).
   * Nothing that grants access beyond the collection id itself travels.
   */
  const buildInviteUrl = (collection: CollectionView): string => {
    const items = collectionItemsOf(collection);
    return buildFullUrl({
      type: 'join-collection',
      collectionId: collection.id,
      collectionName: collection.name,
      inviter: userSettings?.userName ?? 'A collaborator',
      syncServer: collection.syncServer,
      entries: [],
      preview: {
        kind: 'collection',
        projects: items.slice(0, 3).map((it) => ({
          name: it.description || 'Untitled project',
          topFiles: (it.summary?.topFiles ?? []).slice(0, 1),
          fileCount: it.summary?.fileCount ?? 0,
          contributorInitials: (it.summary?.contributors ?? [])
            .map((c) => initialsFor(c.name))
            .slice(0, 4),
        })),
        totalProjects: items.length,
        memberFirstNames: peopleOn(collection)
          .map((p) => p.name.split(/\s+/)[0])
          .slice(0, 3),
      },
    });
  };

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
      <div className="qh-menu qh-members" role="dialog" aria-label={`People on ${collection.name}`}>
        <div className="qh-menu-label">
          {people.length <= 1
            ? 'ONLY YOU SO FAR'
            : `${people.length} PEOPLE SEEN ON THESE PROJECTS`}
        </div>
        <div className="qh-members-list">
          {people.map((m, i) => (
            <div key={`${m.initials}-${i}`} className="qh-member-row">
              <span className="qh-face lg" style={{ backgroundColor: m.color }}>{m.initials}</span>
              <span className="qh-member-name qh-truncate">
                {m.name}
                {i === 0 && selfUser && <span className="qh-member-you"> (you)</span>}
              </span>
            </div>
          ))}
        </div>
        <div className="qh-menu-divider" />
        <div className="qh-menu-label">INVITE BY LINK</div>
        <div className="qh-members-invite">
          <Tooltip block content={inviteUrl}>
            <span className="qh-invite-url mono qh-truncate">{inviteUrl.replace(/^https?:\/\//, '').slice(0, 34)}…</span>
          </Tooltip>
          <button
            className="qh-btn primary small-invite"
            onClick={() => copyToClipboard(inviteUrl, copyKey)}
          >
            {copied === copyKey ? 'Copied!' : 'Copy link'}
          </button>
        </div>
        <div className="qh-invite-note">
          Anyone with this link can join this collection and add or remove projects.
          Its contents sync to them for real.
        </div>
      </div>
    );
  };

  const renderCard = (item: ProjectItem) => (
    <div
      key={item.indexDocId}
      className={`qh-card qh-menu-anchor ${draggingId === item.indexDocId ? 'dragging' : ''} ${peekFor === item.indexDocId ? 'peek-open' : ''}`}
      draggable
      onDragStart={handleDragStart(item)}
      onDragEnd={handleDragEnd}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        setOpenMenu(openMenu === item.indexDocId ? null : item.indexDocId);
      }}
    >
      <button className="qh-card-body" onClick={() => handleOpen(item)}>
        <span style={{ display: 'flex', alignItems: 'baseline', minWidth: 0 }}>
          <span className={`qh-card-name qh-truncate ${isUnnamed(item.description) ? 'unnamed' : ''}`}>
            {item.description}
          </span>
          {openingId === item.indexDocId && <em className="qh-opening">opening...</em>}
        </span>
        <span className="qh-card-footer">
          <span className="qh-card-meta">
            {item.summary ? `${item.summary.fileCount} ${item.summary.fileCount === 1 ? 'file' : 'files'} · ` : ''}
            opened {formatOpened(item.lastAccessed)}
          </span>
          {renderFacepile(contributorsFor(item), 'sm')}
        </span>
      </button>
      <span className="qh-card-actions">
        <span
          className="qh-peek-anchor"
          onMouseOver={() => openPeekHover(item.indexDocId)}
          onMouseOut={closePeekHoverSoon}
        >
          <button
            className="qh-peek-btn"
            aria-label={`Peek — see what's inside ${item.description}`}
            onClick={(e) => { e.stopPropagation(); openPeekHover(item.indexDocId); }}
          >
            {peekIcon}
          </button>
          {peekFor === item.indexDocId && renderPeek(item)}
        </span>
        <Tooltip content={`Duplicate "${item.description}" (fork a fresh copy)`}>
          <button
            className="qh-fork-btn"
            aria-label={`Duplicate "${item.description}"`}
            disabled={!!duplicatingId}
            onClick={(e) => { e.stopPropagation(); openDuplicateDialog(item); }}
          >
            {forkIcon}
          </button>
        </Tooltip>
        <button
          className="qh-card-menu-btn"
          aria-label={`Actions for ${item.description}`}
          aria-haspopup="menu"
          onClick={(e) => {
            e.stopPropagation();
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
        className={`qh-collection ${dropTarget === collection.id ? 'drop-target' : ''}`}
        {...dropZoneProps(collection.id)}
      >
        <div className="qh-collection-header qh-menu-anchor">
          <span className="qh-collection-name">{collection.name}</span>
          <span className="qh-collection-count">{collectionItems.length}</span>
          {(() => {
            const people = peopleOn(collection);
            const hasOthers = people.length > 1;
            return (
              <Tooltip
                content={hasOthers
                  ? `${people.length} people seen on these projects — people & invite`
                  : 'Only you so far. Click to invite others.'}
              >
              <button
                className={`qh-collection-people ${hasOthers ? '' : 'private'}`}
                onClick={(e) => {
                  e.stopPropagation();
                  setOpenMenu(null);
                  setMembersFor(membersFor === collection.id ? null : collection.id);
                }}
              >
                {hasOthers && <PeopleIcon />}
                {renderFacepile(people, 'md', 3)}
              </button>
              </Tooltip>
            );
          })()}
          <span className="qh-flex-spacer" />
          <span className="qh-collection-sort-anchor">
            <Tooltip content={`Sort collection (${sortOrderLabel(collectionSort)})`}>
              <button
                className={`qh-icon-btn qh-collection-sort-btn ${collectionSort !== 'newest' ? 'active' : ''}`}
                aria-label={`Sort collection (${sortOrderLabel(collectionSort)})`}
                aria-haspopup="menu"
                onClick={(e) => {
                  e.stopPropagation();
                  setMembersFor(null);
                  setOpenMenu(openMenu === sortMenuKey ? null : sortMenuKey);
                }}
              >
                <SortIcon />
              </button>
            </Tooltip>
            {openMenu === sortMenuKey && (
              <Menu className="qh-menu-right" onClose={() => closeAllMenus()} ignoreOutsideSelector=".qh-menu-anchor, .qh-peek" aria-label="Sort collection">
                {(['newest', 'oldest', 'name'] as SortOrder[]).map((o) => (
                  <MenuItem
                    key={o}
                    strong={collectionSort === o}
                    onSelect={() => {
                      setCollectionSorts((s) => ({ ...s, [collection.id]: o }));
                      setCollectionPages((p) => ({ ...p, [collection.id]: 0 }));
                      setOpenMenu(null);
                    }}
                  >
                    {o === 'newest' ? 'Newest first' : o === 'oldest' ? 'Oldest first' : 'A to Z'}
                  </MenuItem>
                ))}
              </Menu>
            )}
          </span>
          <Tooltip content="Collection actions">
            <button
              className="qh-icon-btn"
              aria-label={`Actions for ${collection.name}`}
              aria-haspopup="menu"
              onClick={(e) => {
                e.stopPropagation();
                setMembersFor(null);
                setOpenMenu(openMenu === menuKey ? null : menuKey);
              }}
            >
              ⋯
            </button>
          </Tooltip>
          {membersFor === collection.id && renderMembersPopover(collection)}
          {openMenu === menuKey && (
            <Menu className="qh-menu-right" onClose={() => closeAllMenus()} ignoreOutsideSelector=".qh-menu-anchor, .qh-peek" aria-label={`Actions for ${collection.name}`}>
              <MenuItem onSelect={() => { setOpenMenu(null); setMembersFor(collection.id); }}>
                People &amp; invite…
              </MenuItem>
              <MenuItem
                subtext="Renames it for everyone subscribed"
                onSelect={() => {
                  setRenameCollectionTarget(collection);
                  setRenameCollectionValue(collection.name);
                  closeAllMenus();
                }}
              >
                Rename collection…
              </MenuItem>
              <MenuDivider />
              <MenuItem
                danger
                subtext="Removes it from your view only"
                onSelect={() => {
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
              </MenuItem>
            </Menu>
          )}
        </div>
        {collectionItems.length === 0 ? (
          <div className="qh-collection-empty">Empty collection — drag a project here, or use its ⋯ menu.</div>
        ) : (
          <div className="qh-collection-row">
            {page > 0 && (
              <Tooltip content={collectionSort === 'newest' ? 'Newer projects' : collectionSort === 'oldest' ? 'Older projects' : 'Previous page'}>
                <button
                  className="qh-pager"
                  aria-label="Previous page"
                  onClick={() => setCollectionPages((p) => ({ ...p, [collection.id]: page - 1 }))}
                >
                  ‹
                </button>
              </Tooltip>
            )}
            <div className="qh-card-grid">{pageItems.map(renderCard)}</div>
            {page < pageCount - 1 ? (
              <Tooltip content={collectionSort === 'newest' ? 'Older projects' : collectionSort === 'oldest' ? 'Newer projects' : 'Next page'}>
                <button
                  className="qh-pager"
                  aria-label="Next page"
                  onClick={() => setCollectionPages((p) => ({ ...p, [collection.id]: page + 1 }))}
                >
                  ›
                  <span className="qh-pager-pos mono">{page + 1}/{pageCount}</span>
                </button>
              </Tooltip>
            ) : pageCount > 1 ? (
              <div className="qh-pager qh-pager-idle">
                <span className="qh-pager-pos mono">{page + 1}/{pageCount}</span>
              </div>
            ) : null}
          </div>
        )}
      </section>
    );
  };

  return (
    <div className="projects-home">
      <header className="qh-header">
        <div className="qh-logo">
          <img
            src="/quarto-icon.svg"
            alt=""
            width="20"
            height="20"
            style={{ filter: 'var(--logo-filter)' }}
            aria-hidden="true"
          />
          <span>Quarto Hub</span>
        </div>
        <div className="qh-search">
          <input
            ref={searchRef}
            type="text"
            placeholder="Search projects…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <span className="qh-search-kbd mono">⌘K</span>
        </div>
        <div className="qh-flex-spacer" />
        <div className="qh-header-actions">
          <button className="qh-btn outline" onClick={() => { setAddDialogOpen(true); setAddTab('connect'); setFormError(null); }}>
            Connect / Import ▾
          </button>
          <div className="qh-menu-anchor qh-new-anchor">
            <button className="qh-btn primary" onClick={() => setNewMenuOpen((v) => !v)}>
              ＋ New ▾
            </button>
            {newMenuOpen && (
              <Menu className="qh-menu-right" onClose={() => setNewMenuOpen(false)} ignoreOutsideSelector=".qh-menu-anchor" aria-label="New project">
                <MenuLabel>START FROM — QUARTO PROJECT TYPES</MenuLabel>
                {(projectChoices.length > 0
                  ? projectChoices
                  : [{ id: 'default', name: 'Default', description: 'A minimal Quarto project' }]
                ).map((choice) => (
                  <MenuItem key={choice.id} strong subtext={choice.description} onSelect={() => openNewDialog(choice)}>
                    {choice.name}
                  </MenuItem>
                ))}
              </Menu>
            )}
          </div>
          <div className="qh-menu-anchor qh-avatar-anchor">
            <button
              className="qh-avatar"
              style={authPicture ? undefined : { backgroundColor: userSettings?.userColor ?? 'var(--posit-blue)' }}
              onClick={() => setAvatarMenuOpen((v) => !v)}
              aria-label={userSettings?.userName ? `Account: ${userSettings.userName}` : 'Account'}
            >
              {authPicture ? (
                <img src={authPicture} alt="" className="qh-avatar-img" referrerPolicy="no-referrer" />
              ) : (
                initialsFor(userSettings?.userName ?? '')
              )}
            </button>
            {avatarMenuOpen && userSettings && (
              <div className="qh-menu qh-menu-right qh-avatar-menu">
                <div className="qh-avatar-menu-id">
                  <span className="qh-avatar big" style={authPicture ? undefined : { backgroundColor: userSettings.userColor }}>
                    {authPicture ? (
                      <img src={authPicture} alt="" className="qh-avatar-img" referrerPolicy="no-referrer" />
                    ) : (
                      initialsFor(userSettings.userName)
                    )}
                  </span>
                  <div className="qh-avatar-menu-who">
                    {editingName ? (
                      <input
                        className="qh-name-input"
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
                      <div className="qh-avatar-menu-name">
                        <strong>{userSettings.userName}</strong>
                        <button
                          className="qh-link"
                          onClick={() => { setEditNameValue(userSettings.userName); setEditingName(true); }}
                        >
                          edit
                        </button>
                      </div>
                    )}
                    <div className="qh-avatar-menu-mail">
                      {authEmail ?? 'Not signed in'}
                      {onSignOut && <> · <button className="qh-link" onClick={onSignOut}>Sign out</button></>}
                    </div>
                  </div>
                </div>
                <div className="qh-menu-label">CURSOR COLOR</div>
                <div className="qh-swatches">
                  {COLOR_PALETTE.map((color) => (
                    <Tooltip key={color} content={color}>
                      <button
                        className={`qh-swatch ${userSettings.userColor === color ? 'selected' : ''}`}
                        style={{ backgroundColor: color }}
                        onClick={() => handleColorChange(color)}
                        aria-label={`Cursor color ${color}`}
                      />
                    </Tooltip>
                  ))}
                </div>
                <div className="qh-menu-divider" />
                {projectSetLinkUrl && (
                  <button className="qh-menu-item" onClick={() => { setShowLinkDialog(true); setAvatarMenuOpen(false); }}>
                    Link another browser…
                  </button>
                )}
                <button className="qh-menu-item" onClick={handleExportJson}>Export project list (JSON)…</button>
                <button className="qh-menu-item" onClick={handleImportJson}>Import project list (JSON)…</button>
                <button className="qh-menu-item with-hint" onClick={cycleColorScheme}>
                  Theme
                  <span className="qh-menu-hint">
                    {colorScheme === 'auto' ? 'Auto' : colorScheme === 'dark' ? 'Dark' : 'Light'} ▾
                  </span>
                </button>
                {onSwitchToClassicUi && (
                  <>
                    <div className="qh-menu-divider" />
                    <button className="qh-menu-item" onClick={onSwitchToClassicUi}>
                      Switch to classic UI
                      <span className="qh-menu-subtext">Back to the current shipping project list</span>
                    </button>
                  </>
                )}
              </div>
            )}
          </div>
        </div>
      </header>

      {(connectionError || formError) && !addDialogOpen && !newDialogChoice && (
        <div className="qh-error">
          {connectionError || formError}
          {connectionError && onRetry && (
            <button type="button" className="qh-error-action" onClick={onRetry}>
              {common.retry}
            </button>
          )}
        </div>
      )}

      <main id="main-content" tabIndex={-1} className="qh-main">
        {loadError ? (
          <div className="qh-empty-state">
            <h2>Couldn't load your projects</h2>
            <p>{loadError}</p>
            <div className="qh-empty-actions">
              <button className="qh-btn outline" onClick={handleRetryLoad}>
                {common.retry}
              </button>
            </div>
          </div>
        ) : items.length === 0 && collections.every((c) => c.entries.length === 0) ? (
          <div className="qh-empty-state">
            <div className="qh-empty-icon" aria-hidden="true"><FilePlusIcon size={24} /></div>
            <h2>No projects yet</h2>
            <p>Create your first Quarto project, or connect to one a collaborator shared.</p>
            <div className="qh-empty-actions">
              <button className="qh-btn primary" onClick={() => setNewMenuOpen(true)}>＋ New project</button>
              <button className="qh-btn outline" onClick={() => { setAddDialogOpen(true); setAddTab('connect'); }}>
                Connect / Import
              </button>
            </div>
          </div>
        ) : (
          <>
            {collections.map(renderCollection)}

            <div className="qh-new-collection-row">
              <button className="qh-btn ghost-accent" onClick={() => openNewCollection()}>＋ New collection</button>
            </div>

            <section
              className={`qh-rest ${dropTarget === 'unshelved' ? 'drop-target' : ''}`}
              {...dropZoneProps('unshelved')}
            >
              <div className="qh-rest-header qh-menu-anchor">
                <span className="qh-rest-title">Everything else</span>
                <span className="qh-rest-count">{everythingElse.length} · {sortLabel}</span>
                <span className="qh-flex-spacer" />
                <button className="qh-btn small outline" onClick={(e) => { e.stopPropagation(); setSortMenuOpen((v) => !v); }}>
                  Sort <span className="qh-caret">▾</span>
                </button>
                {sortMenuOpen && (
                  <Menu className="qh-menu-right" onClose={() => setSortMenuOpen(false)} ignoreOutsideSelector=".qh-menu-anchor" aria-label="Sort projects">
                    {(['newest', 'oldest', 'name'] as SortOrder[]).map((o) => (
                      <MenuItem
                        key={o}
                        strong={sortOrder === o}
                        onSelect={() => { setSortOrder(o); setSortMenuOpen(false); }}
                      >
                        {o === 'newest' ? 'Newest first' : o === 'oldest' ? 'Oldest first' : 'A to Z'}
                      </MenuItem>
                    ))}
                  </Menu>
                )}
              </div>
              {everythingElse.length === 0 ? (
                <div className="qh-rest-empty">
                  {query ? 'No projects match your search.' : 'Everything is in a collection.'}
                </div>
              ) : (
                <div className="qh-rest-list">
                  {everythingElse.map((item) => (
                    <div
                      key={item.indexDocId}
                      className={`qh-row qh-menu-anchor ${draggingId === item.indexDocId ? 'dragging' : ''}`}
                      draggable
                      onDragStart={handleDragStart(item)}
                      onDragEnd={handleDragEnd}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        e.stopPropagation();
                        setOpenMenu(openMenu === item.indexDocId ? null : item.indexDocId);
                      }}
                    >
                      <button
                        className={`qh-row-name qh-truncate ${isUnnamed(item.description) ? 'unnamed' : ''}`}
                        onClick={() => handleOpen(item)}
                      >
                        {item.description}
                      </button>
                      {openingId === item.indexDocId && <em className="qh-opening">opening...</em>}
                      {isUnnamed(item.description) && (
                        <button className="qh-link" onClick={() => startRename(item)}>Rename</button>
                      )}
                      <span className="qh-row-meta">
                        {item.summary ? `${item.summary.fileCount} ${item.summary.fileCount === 1 ? 'file' : 'files'} · ` : ''}
                        opened {formatOpened(item.lastAccessed)}
                      </span>
                      <span
                        className="qh-peek-anchor"
                        onMouseOver={() => openPeekHover(item.indexDocId)}
                        onMouseOut={closePeekHoverSoon}
                      >
                        <button
                          className="qh-icon-btn qh-peek-btn"
                          aria-label={`Peek — see what's inside ${item.description}`}
                          onClick={(e) => { e.stopPropagation(); openPeekHover(item.indexDocId); }}
                        >
                          {peekIcon}
                        </button>
                        {peekFor === item.indexDocId && renderPeek(item)}
                      </span>
                      <Tooltip content={`Duplicate "${item.description}" (fork a fresh copy)`}>
                        <button
                          className="qh-icon-btn qh-fork-btn"
                          aria-label={`Duplicate "${item.description}"`}
                          disabled={!!duplicatingId}
                          onClick={(e) => { e.stopPropagation(); openDuplicateDialog(item); }}
                        >
                          {forkIcon}
                        </button>
                      </Tooltip>
                      <button
                        className="qh-icon-btn qh-row-menu-btn"
                        aria-label={`Actions for ${item.description}`}
                        aria-haspopup="menu"
                        onClick={(e) => {
                          e.stopPropagation();
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
        <ModalDialog
          title={`Duplicate "${duplicateFor.description}"`}
          onClose={() => { if (!duplicatingId) setDuplicateFor(null); }}
          className="qh-form-dialog"
        >
          <form onSubmit={(e) => { e.preventDefault(); handleDuplicate(); }}>
            <div className="dialog-content">
              <p className="qh-dialog-hint">
                A fresh copy of all {duplicateFor.summary ? `${duplicateFor.summary.fileCount} ` : ''}files — no
                edit history carries over.
              </p>
              {formError && <div className="qh-error inline">{formError}</div>}
              <label className="qh-field-label" htmlFor="qh-dup-name">Name</label>
              <input
                id="qh-dup-name"
                className="qh-input focus-accent"
                value={duplicateName}
                onChange={(e) => setDuplicateName(e.target.value)}
                autoFocus
                onFocus={(e) => e.target.select()}
              />
              <label className="qh-field-label" htmlFor="qh-dup-collection">Add to collection</label>
              <select
                id="qh-dup-collection"
                className="qh-input"
                value={duplicateCollectionId}
                onChange={(e) => setDuplicateCollectionId(e.target.value)}
              >
                <option value="">No collection</option>
                {collections.map((c) => (
                  <option key={c.id} value={c.id}>{c.name}</option>
                ))}
              </select>
            </div>
            <div className="dialog-actions">
              <button type="button" className="qh-btn outline" disabled={!!duplicatingId} onClick={() => setDuplicateFor(null)}>
                Cancel
              </button>
              <button type="submit" className="qh-btn primary" disabled={!!duplicatingId || !duplicateName.trim()}>
                {duplicatingId ? 'Duplicating…' : 'Duplicate'}
              </button>
            </div>
          </form>
        </ModalDialog>
      )}

      {/* New collection */}
      {newCollectionDialog && (
        <ModalDialog
          title="New collection"
          onClose={() => setNewCollectionDialog(null)}
          className="qh-form-dialog"
        >
          <form onSubmit={(e) => { e.preventDefault(); commitNewCollection(); }}>
            <div className="dialog-content">
              {newCollectionDialog.forProject && (
                <p className="qh-dialog-hint">The project will be moved onto it.</p>
              )}
              <label className="qh-field-label" htmlFor="qh-new-collection-name">Name</label>
              <input
                id="qh-new-collection-name"
                className="qh-input focus-accent"
                value={newCollectionName}
                onChange={(e) => setNewCollectionName(e.target.value)}
                placeholder="e.g. Board prep"
                autoFocus
              />
            </div>
            <div className="dialog-actions">
              <button type="button" className="qh-btn outline" onClick={() => setNewCollectionDialog(null)}>Cancel</button>
              <button type="submit" className="qh-btn primary" disabled={!newCollectionName.trim()}>Create</button>
            </div>
          </form>
        </ModalDialog>
      )}

      {/* Rename collection */}
      {renameCollectionTarget && (
        <ModalDialog
          title="Rename collection"
          onClose={() => setRenameCollectionTarget(null)}
          className="qh-form-dialog"
        >
          <form
            onSubmit={(e) => {
              e.preventDefault();
              if (renameCollectionValue.trim()) {
                onRenameCollection?.(renameCollectionTarget.id, renameCollectionValue.trim());
              }
              setRenameCollectionTarget(null);
            }}
          >
            <div className="dialog-content">
              <p className="qh-dialog-hint">Renames it for everyone subscribed to it.</p>
              <label className="qh-field-label" htmlFor="qh-rename-collection">Name</label>
              <input
                id="qh-rename-collection"
                className="qh-input focus-accent"
                value={renameCollectionValue}
                onChange={(e) => setRenameCollectionValue(e.target.value)}
                autoFocus
              />
            </div>
            <div className="dialog-actions">
              <button type="button" className="qh-btn outline" onClick={() => setRenameCollectionTarget(null)}>Cancel</button>
              <button type="submit" className="qh-btn primary" disabled={!renameCollectionValue.trim()}>Rename</button>
            </div>
          </form>
        </ModalDialog>
      )}

      {/* Generic destructive confirmation */}
      {confirmState && (
        <ModalDialog
          title={confirmState.title}
          onClose={() => setConfirmState(null)}
          className="qh-form-dialog"
        >
          <div className="dialog-content">
            <p className="qh-dialog-hint">{confirmState.body}</p>
          </div>
          <div className="dialog-actions">
            <button type="button" className="qh-btn outline" onClick={() => setConfirmState(null)} autoFocus>
              Cancel
            </button>
            <button
              type="button"
              className="qh-btn danger"
              onClick={() => { confirmState.action(); setConfirmState(null); }}
            >
              {confirmState.confirmLabel}
            </button>
          </div>
        </ModalDialog>
      )}

      {/* Shared-collection move warning */}
      {pendingMove && (
        <ModalDialog
          title={`Move "${pendingMove.name}" out of ${pendingMove.fromName}?`}
          onClose={() => setPendingMove(null)}
          className="qh-form-dialog"
        >
          <div className="dialog-content">
            <p className="qh-dialog-hint">
              Please note you're changing {pendingMove.othersCount === 1
                ? "another person's"
                : `${pendingMove.othersCount} other people's`} view of this collection — it will
              no longer appear in {pendingMove.fromName} for them. The project itself isn't
              deleted or changed.
            </p>
            <label className="qh-checkbox-row">
              <input
                type="checkbox"
                checked={moveWarnChecked}
                onChange={(e) => setMoveWarnChecked(e.target.checked)}
              />
              Don't show this again
            </label>
          </div>
          <div className="dialog-actions">
            <button type="button" className="qh-btn outline" onClick={() => setPendingMove(null)}>
              Cancel
            </button>
            <button
              type="button"
              className="qh-btn primary"
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
        </ModalDialog>
      )}

      {/* Rename dialog */}
      {renameFor && (
        <ModalDialog
          title="Rename project"
          onClose={() => setRenameFor(null)}
          className="qh-form-dialog"
        >
          <form onSubmit={(e) => { e.preventDefault(); commitRename(); }}>
            <div className="dialog-content">
              <label className="qh-field-label" htmlFor="qh-rename">Name</label>
              <input
                id="qh-rename"
                className="qh-input focus-accent"
                value={renameValue}
                onChange={(e) => setRenameValue(e.target.value)}
                placeholder="e.g. Lab retreat agenda"
                autoFocus
              />
            </div>
            <div className="dialog-actions">
              <button type="button" className="qh-btn outline" onClick={() => setRenameFor(null)}>Cancel</button>
              <button type="submit" className="qh-btn primary" disabled={!renameValue.trim()}>Rename</button>
            </div>
          </form>
        </ModalDialog>
      )}

      {/* New project dialog */}
      {newDialogChoice && (
        <ModalDialog
          title={`New ${newDialogChoice.name.toLowerCase()}`}
          onClose={() => setNewDialogChoice(null)}
          className="qh-form-dialog"
        >
          <form onSubmit={handleCreate}>
            <div className="dialog-content">
              <p className="qh-dialog-hint">Starter files will be created for you</p>
              {formError && <div className="qh-error inline">{formError}</div>}
              <label className="qh-field-label" htmlFor="qh-new-name">Name</label>
              <input
                id="qh-new-name"
                className="qh-input focus-accent"
                value={newTitle}
                onChange={(e) => setNewTitle(e.target.value)}
                placeholder="Q3 all-hands deck"
                autoFocus
              />
              <label className="qh-field-label" htmlFor="qh-new-collection">Add to collection (optional)</label>
              <select
                id="qh-new-collection"
                className="qh-input"
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
                  <label className="qh-field-label" htmlFor="qh-new-server">Sync server</label>
                  <input
                    id="qh-new-server"
                    className="qh-input mono"
                    value={newServer}
                    onChange={(e) => setNewServer(e.target.value)}
                  />
                </>
              ) : (
                <div className="qh-server-line">
                  Syncs to {serverHost(newServer)}{' '}
                  <button type="button" className="qh-link" onClick={() => setShowServerField(true)}>Change…</button>
                </div>
              )}
            </div>
            <div className="dialog-actions">
              <button type="button" className="qh-btn outline" onClick={() => setNewDialogChoice(null)}>Cancel</button>
              <button type="submit" className="qh-btn primary" disabled={isCreating || !newTitle.trim()}>
                {isCreating ? 'Creating…' : 'Create'}
              </button>
            </div>
          </form>
        </ModalDialog>
      )}

      {/* Connect / Import dialog */}
      {addDialogOpen && (
        <ModalDialog
          title="Add an existing project"
          onClose={() => setAddDialogOpen(false)}
          className="qh-form-dialog wide"
        >
          <div className="dialog-content">
            <div className="qh-tabs">
              <button
                className={`qh-tab ${addTab === 'connect' ? 'active' : ''}`}
                onClick={() => { setAddTab('connect'); setFormError(null); }}
              >
                Connect by link or ID
              </button>
              <button
                className={`qh-tab ${addTab === 'import' ? 'active' : ''}`}
                onClick={() => { setAddTab('import'); setFormError(null); }}
              >
                Import from ZIP
              </button>
            </div>
            {formError && <div className="qh-error inline">{formError}</div>}
            {addTab === 'connect' ? (
              <form onSubmit={handleConnect}>
                <label className="qh-field-label" htmlFor="qh-connect-input">Paste a share link or project ID</label>
                <input
                  id="qh-connect-input"
                  className="qh-input mono focus-accent"
                  value={connectInput}
                  onChange={(e) => setConnectInput(e.target.value)}
                  placeholder="https://quarto-hub.com/#/share/… or bs58 ID"
                  autoFocus
                />
                {showConnectServer ? (
                  <>
                    <label className="qh-field-label" htmlFor="qh-connect-server">Sync server</label>
                    <input
                      id="qh-connect-server"
                      className="qh-input mono"
                      value={connectServer}
                      onChange={(e) => setConnectServer(e.target.value)}
                    />
                  </>
                ) : (
                  <div className="qh-server-line">
                    Server is read from the link · advanced:{' '}
                    <button type="button" className="qh-link" onClick={() => setShowConnectServer(true)}>
                      set server manually
                    </button>
                  </div>
                )}
                <label className="qh-field-label" htmlFor="qh-connect-name">Name it for your list (optional)</label>
                <input
                  id="qh-connect-name"
                  className="qh-input"
                  value={connectName}
                  onChange={(e) => setConnectName(e.target.value)}
                  placeholder="e.g. Lab retreat agenda"
                />
                <div className="dialog-actions">
                  <button type="button" className="qh-btn outline" onClick={() => setAddDialogOpen(false)}>Cancel</button>
                  <button type="submit" className="qh-btn primary" disabled={!connectInput.trim()}>Connect</button>
                </div>
              </form>
            ) : (
              <form onSubmit={handleImportZip}>
                <label className="qh-field-label" htmlFor="qh-import-file">ZIP file</label>
                <input
                  id="qh-import-file"
                  className="qh-input"
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
                <label className="qh-field-label" htmlFor="qh-import-title">Name</label>
                <input
                  id="qh-import-title"
                  className="qh-input"
                  value={importTitle}
                  onChange={(e) => setImportTitle(e.target.value)}
                  placeholder="My imported project"
                />
                <div className="dialog-actions">
                  <button type="button" className="qh-btn outline" onClick={() => setAddDialogOpen(false)}>Cancel</button>
                  <button type="submit" className="qh-btn primary" disabled={isImporting || !importFile}>
                    {isImporting ? 'Importing…' : 'Import'}
                  </button>
                </div>
              </form>
            )}
          </div>
        </ModalDialog>
      )}

      <ShareDialog
        isOpen={showLinkDialog}
        shareableUrl={projectSetLinkUrl}
        onClose={() => setShowLinkDialog(false)}
      />

      <footer className="qh-footer">
        <Tooltip content={`Built: ${__BUILD_TIME__} · Commit date: ${__GIT_COMMIT_DATE__}`}>
          <span className="mono" tabIndex={0}>
            {__GIT_COMMIT_HASH__}
          </span>
        </Tooltip>
        <span className="qh-footer-note">collections UI exploration</span>
      </footer>
    </div>
  );
}
