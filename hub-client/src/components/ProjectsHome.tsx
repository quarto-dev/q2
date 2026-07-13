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
import type { ProjectSetStatus } from '../hooks/useProjectSet';
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
import { mockCollaborators, unionCollaborators, type MockUser } from '../utils/mockCollaborators';
import { useCollections, setPendingCollectionAssignment, type Collection, type CollectionMember } from '../hooks/useCollections';
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
  /** Replace a project's cached peek summary (used by Peek's refresh). */
  onUpdateProjectSummary?: (indexDocId: string, summary: ProjectSetEntrySummary) => void;
  onSwitchToClassicUi?: () => void;
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
  onUpdateProjectSummary,
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
  const [moveSubmenuOpen, setMoveSubmenuOpen] = useState(false);
  const [newMenuOpen, setNewMenuOpen] = useState(false);
  const [avatarMenuOpen, setAvatarMenuOpen] = useState(false);
  const [peekFor, setPeekFor] = useState<string | null>(null);
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
  const { collections, createCollection, renameCollection, deleteCollection, moveProject, reconcilePending, shareCollection, removeMember } = useCollections();
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
  const [renameCollectionTarget, setRenameCollectionTarget] = useState<Collection | null>(null);
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

  // Moving a project out of a shared collection changes what its other
  // members see. Warn once (suppressible) before such moves; private
  // collections and moves within the same collection pass straight through.
  const collectionOf = useCallback((indexDocId: string): Collection | undefined => {
    const short = indexDocId.replace(/^automerge:/, '');
    return collections.find((c) =>
      c.projectIds.some((id) => id === indexDocId || id === short || `automerge:${id}` === indexDocId),
    );
  }, [collections]);

  const requestMove = useCallback((indexDocId: string, target: string | null) => {
    const from = collectionOf(indexDocId);
    const othersCount = from?.shared?.members.filter((m) => !m.isYou).length ?? 0;
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
  }, [collectionOf, byId, moveProject, closeAllMenus]);

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

  /**
   * Duplicate a project: background-connect to the source, read every file
   * (text and binary), and feed them through the same creation path new
   * projects use. The copy keeps the source's collection placement via the
   * pending-assignment mechanism (the new doc id isn't known until the
   * parent finishes creating it).
   */
  const handleDuplicate = useCallback(async (item: ProjectItem) => {
    if (duplicatingId || exportingId) return;
    setDuplicatingId(item.indexDocId);
    setFormError(null);
    const copyTitle = `${item.description} (copy)`;
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
      const sourceCollection = collectionOf(item.indexDocId);
      if (sourceCollection) {
        setPendingCollectionAssignment(copyTitle, sourceCollection.id);
      }
      onProjectCreated?.(projectFiles, copyTitle, 'duplicate', item.syncServer);
    } catch (err) {
      console.error('Duplicate failed:', err);
      setFormError(err instanceof Error ? `Duplicate failed: ${err.message}` : 'Duplicate failed.');
      try { await disconnect(); } catch { /* connection already down */ }
    } finally {
      setDuplicatingId(null);
      closeAllMenus();
    }
  }, [duplicatingId, exportingId, collectionOf, onProjectCreated, closeAllMenus]);

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

  const commitNewCollection = useCallback(() => {
    if (!newCollectionDialog || !newCollectionName.trim()) return;
    const id = createCollection(newCollectionName.trim());
    if (newCollectionDialog.forProject) {
      requestMove(newCollectionDialog.forProject, id);
    }
    setNewCollectionDialog(null);
    setNewCollectionName('');
  }, [newCollectionDialog, newCollectionName, createCollection, requestMove]);

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

  // Facepiles are mock data for the exploration (see utils/mockCollaborators).
  // The real user is always the first face, in their cursor color.
  const selfUser: MockUser | undefined = userSettings
    ? { name: `${userSettings.userName} (you)`, initials: initialsFor(userSettings.userName), color: userSettings.userColor }
    : undefined;
  const collaboratorsFor = useCallback(
    (indexDocId: string) => mockCollaborators(indexDocId, selfUser),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [userSettings?.userName, userSettings?.userColor],
  );

  const renderFacepile = (users: MockUser[], size: 'sm' | 'md' | 'lg', max = 3, mock = true) => {
    const shown = users.slice(0, max);
    const extra = users.length - shown.length;
    return (
      <span className={`ph-facepile ${size}`}>
        {shown.map((u, i) => (
          <span key={`${u.initials}-${i}`} className="ph-face" style={{ backgroundColor: u.color }} title={mock ? `${u.name} (mock)` : u.name}>
            {u.initials}
          </span>
        ))}
        {extra > 0 && <span className="ph-face more" title={`${extra} more (mock)`}>+{extra}</span>}
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
          Move to collection <span className="ph-submenu-arrow">▸</span>
        </button>
        {moveSubmenuOpen && (
          <div className="ph-menu ph-submenu">
            {collections.map((collection) => (
              <button
                key={collection.id}
                className="ph-menu-item"
                onClick={() => { requestMove(item.indexDocId, collection.id); closeAllMenus(); }}
              >
                {collection.name}
              </button>
            ))}
            {shelvedIds.has(item.indexDocId) && (
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
      <button
        className="ph-menu-item"
        onClick={(e) => {
          e.stopPropagation();
          setOpenMenu(null);
          setMoveSubmenuOpen(false);
          setPeekFor(item.indexDocId);
        }}
      >
        Peek
        <span className="ph-menu-subtext">See what's inside without opening it</span>
      </button>
      <button
        className="ph-menu-item"
        disabled={!!duplicatingId}
        onClick={(e) => { e.stopPropagation(); handleDuplicate(item); }}
      >
        {duplicatingId === item.indexDocId ? 'Duplicating…' : 'Duplicate'}
        <span className="ph-menu-subtext">New copy named "{item.description} (copy)"</span>
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
                  'lg', 3, false,
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
          <button className="ph-btn primary" onClick={() => startRename(item)}>Rename…</button>
          <button className="ph-btn outline" onClick={() => { closeAllMenus(); handleOpen(item); }}>Open</button>
          <button className="ph-link" onClick={() => refreshPeek(item)}>
            {peekRefreshing ? 'Refreshing…' : s ? 'Refresh' : 'Load details'}
          </button>
          <button className="ph-link danger" onClick={() => handleRemove(item)}>Remove…</button>
        </div>
        <div className="ph-peek-footnote">Peeking doesn't count as opening the project.</div>
      </div>
    );
  };

  // ---- collection sharing (membership is mock; see utils/mockCollaborators) ----

  const collectionItemsOf = (collection: Collection): ProjectItem[] =>
    collection.projectIds
      .map((id) => byId.get(id) ?? byId.get(`automerge:${id}`))
      .filter((it): it is ProjectItem => !!it);

  const buildInviteUrl = (collection: Collection): string =>
    buildFullUrl({
      type: 'join-collection',
      collectionId: collection.id,
      collectionName: collection.name,
      inviter: userSettings?.userName ?? 'A collaborator',
      entries: collectionItemsOf(collection).map((it) => ({
        indexDocId: it.indexDocId.replace(/^automerge:/, ''),
        syncServer: it.syncServer,
        description: it.description,
      })),
    });

  const handleShareCollection = (collection: Collection) => {
    if (!collection.shared) {
      const now = new Date().toISOString();
      const seeded: CollectionMember[] = selfUser
        ? [{ ...selfUser, name: selfUser.name.replace(/ \(you\)$/, ''), joinedAt: now, isOwner: true, isYou: true }]
        : [];
      // Seed with the mock collaborators already shown on this collection's
      // project cards — the "people with access" story stays consistent.
      const others = unionCollaborators(collectionItemsOf(collection).map((it) => collaboratorsFor(it.indexDocId)))
        .filter((u) => !seeded.some((m) => m.initials === u.initials))
        .map((u) => ({ ...u, joinedAt: now }));
      shareCollection(collection.id, [...seeded, ...others]);
    }
    setOpenMenu(null);
    setMembersFor(collection.id);
  };

  const renderMembersPopover = (collection: Collection) => {
    // A private collection shows the same popover with just you in it, plus
    // the way to share — copying the link is what turns sharing on.
    const members: CollectionMember[] = collection.shared?.members
      ?? (selfUser
        ? [{ ...selfUser, name: selfUser.name.replace(/ \(you\)$/, ''), joinedAt: '', isOwner: true, isYou: true }]
        : []);
    const you = members.find((m) => m.isYou);
    const inviteUrl = buildInviteUrl(collection);
    const copyKey = `collection:${collection.id}:invite`;
    return (
      <div className="ph-menu ph-members" role="dialog" aria-label={`Members of ${collection.name}`}>
        <div className="ph-menu-label">
          {collection.shared
            ? `SHARED WITH ${members.length} ${members.length === 1 ? 'PERSON' : 'PEOPLE'}`
            : 'PRIVATE — ONLY YOU'}
        </div>
        <div className="ph-members-list">
          {members.map((m, i) => (
            <div key={`${m.initials}-${i}`} className="ph-member-row">
              <span className="ph-face lg" style={{ backgroundColor: m.color }}>{m.initials}</span>
              <span className="ph-member-name">
                {m.name}
                {m.isYou && <span className="ph-member-you"> (you)</span>}
              </span>
              {m.isOwner ? (
                <span className="ph-member-badge">Owner</span>
              ) : !m.isYou ? (
                <button
                  className="ph-link danger ph-member-remove"
                  onClick={() => removeMember(collection.id, m.initials)}
                >
                  Remove
                </button>
              ) : null}
            </div>
          ))}
        </div>
        {you && !you.isOwner && (
          <button
            className="ph-menu-item danger"
            onClick={() => {
              closeAllMenus();
              setConfirmState({
                title: `Leave "${collection.name}"?`,
                body: "Removes it from your list only — other members keep it. Projects you've opened stay in your list.",
                confirmLabel: 'Leave collection',
                action: () => deleteCollection(collection.id),
              });
            }}
          >
            Leave collection
          </button>
        )}
        <div className="ph-menu-divider" />
        <div className="ph-menu-label">{collection.shared ? 'INVITE BY LINK' : 'SHARE BY LINK'}</div>
        <div className="ph-members-invite">
          <span className="ph-invite-url mono" title={inviteUrl}>{inviteUrl.replace(/^https?:\/\//, '').slice(0, 34)}…</span>
          <button
            className="ph-btn primary small-invite"
            onClick={() => {
              // Copying the link is the moment a private collection becomes
              // shared — the link leaving your hands is the share.
              if (!collection.shared) handleShareCollection(collection);
              copyToClipboard(inviteUrl, copyKey);
            }}
          >
            {copied === copyKey ? 'Copied!' : 'Copy link'}
          </button>
        </div>
        <div className="ph-invite-note">
          {collection.shared
            ? 'Anyone with this link can join this collection and add or remove projects.'
            : 'Copying turns on sharing — anyone with the link can join and add or remove projects.'}
        </div>
      </div>
    );
  };

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
        <span className="ph-card-footer">
          <span className="ph-card-meta">
            {item.summary ? `${item.summary.fileCount} ${item.summary.fileCount === 1 ? 'file' : 'files'} · ` : ''}
            opened {formatOpened(item.lastAccessed)}
          </span>
          {item.summary?.contributors.length
            ? renderFacepile(
                item.summary.contributors.map((c) => ({ name: c.name, color: c.color, initials: initialsFor(c.name) })),
                'sm', 3, false,
              )
            : renderFacepile(collaboratorsFor(item.indexDocId), 'sm')}
        </span>
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
      {peekFor === item.indexDocId && renderPeek(item)}
    </div>
  );

  const renderCollection = (collection: Collection) => {
    // Collection order is by recency (lastAccessed, newest first), not by stored
    // position — paging walks toward older projects, per the design. True
    // "recent edits" ordering needs automerge-history attribution (Future
    // phase); last-opened is the closest signal in today's metadata.
    const collectionItems = collectionItemsOf(collection)
      .filter(matches)
      .sort((a, b) => (a.lastAccessed < b.lastAccessed ? 1 : -1));
    if (query && collectionItems.length === 0) return null;
    const pageCount = Math.max(1, Math.ceil(collectionItems.length / COLLECTION_PAGE_SIZE));
    const page = Math.min(collectionPages[collection.id] ?? 0, pageCount - 1);
    const pageItems = collectionItems.slice(page * COLLECTION_PAGE_SIZE, (page + 1) * COLLECTION_PAGE_SIZE);
    const menuKey = `collection:${collection.id}`;
    return (
      <section
        key={collection.id}
        className={`ph-collection ${dropTarget === collection.id ? 'drop-target' : ''}`}
        {...dropZoneProps(collection.id)}
      >
        <div className="ph-collection-header qh-menu-anchor">
          <span className="ph-collection-name">{collection.name}</span>
          <span className="ph-collection-count">{collectionItems.length}</span>
          <button
            className={`ph-collection-people ${collection.shared ? '' : 'private'}`}
            title={collection.shared
              ? `Shared with ${collection.shared.members.length} people — members & invite`
              : 'Private — only you. Click to share this collection.'}
            onClick={(e) => {
              e.stopPropagation();
              setOpenMenu(null);
              setMembersFor(membersFor === collection.id ? null : collection.id);
            }}
          >
            {collection.shared && (
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                <circle cx="9" cy="8" r="3.4" stroke="currentColor" strokeWidth="2" />
                <path d="M3 19c0-3 2.7-4.8 6-4.8s6 1.8 6 4.8" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
                <circle cx="17" cy="9" r="2.6" stroke="currentColor" strokeWidth="2" />
                <path d="M16.5 14.4c2.6.3 4.5 1.9 4.5 4.1" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
              </svg>
            )}
            {renderFacepile(
              collection.shared?.members ?? (selfUser ? [selfUser] : []),
              'md',
            )}
          </button>
          <span className="ph-flex-spacer" />
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
            <div className="ph-menu ph-menu-right" role="menu">
              {collection.shared ? (
                <button className="ph-menu-item" onClick={() => handleShareCollection(collection)}>
                  Members &amp; invite…
                </button>
              ) : (
                <button className="ph-menu-item" onClick={() => handleShareCollection(collection)}>
                  Share collection…
                  <span className="ph-menu-subtext">Invite others to this collection</span>
                </button>
              )}
              <button
                className="ph-menu-item"
                onClick={() => {
                  setRenameCollectionTarget(collection);
                  setRenameCollectionValue(collection.name);
                  closeAllMenus();
                }}
              >
                Rename collection…
                {collection.shared && <span className="ph-menu-subtext">Renames it for everyone</span>}
              </button>
              {collection.shared ? (
                <>
                  <div className="ph-menu-divider" />
                  <button
                    className="ph-menu-item danger"
                    onClick={() => {
                      closeAllMenus();
                      setConfirmState({
                        title: `Leave "${collection.name}"?`,
                        body: "Removes it from your list only — other members keep it. Projects you've opened stay in your list.",
                        confirmLabel: 'Leave collection',
                        action: () => deleteCollection(collection.id),
                      });
                    }}
                  >
                    Leave collection
                    <span className="ph-menu-subtext">Removes it from your list only</span>
                  </button>
                  <button
                    className="ph-menu-item danger"
                    onClick={() => {
                      closeAllMenus();
                      setConfirmState({
                        title: `Delete "${collection.name}" for everyone?`,
                        body: "Projects are never deleted — they return to each person's list.",
                        confirmLabel: 'Delete for everyone',
                        action: () => deleteCollection(collection.id),
                      });
                    }}
                  >
                    Delete collection for everyone…
                    <span className="ph-menu-subtext">Projects are never deleted</span>
                  </button>
                </>
              ) : (
                <button
                  className="ph-menu-item danger"
                  onClick={() => {
                    closeAllMenus();
                    setConfirmState({
                      title: `Delete collection "${collection.name}"?`,
                      body: 'Projects return to Everything else — nothing is deleted.',
                      confirmLabel: 'Delete collection',
                      action: () => deleteCollection(collection.id),
                    });
                  }}
                >
                  Delete collection
                  <span className="ph-menu-subtext">Projects return to Everything else</span>
                </button>
              )}
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
                title="Newer projects"
                onClick={() => setCollectionPages((p) => ({ ...p, [collection.id]: page - 1 }))}
              >
                ‹
              </button>
            )}
            <div className="ph-card-grid">{pageItems.map(renderCard)}</div>
            {page < pageCount - 1 ? (
              <button
                className="ph-pager"
                title="Older projects"
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
                  {query ? 'No projects match your search.' : 'Everything is on a collection.'}
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
                      <span className="ph-row-meta">
                        {item.summary ? `${item.summary.fileCount} ${item.summary.fileCount === 1 ? 'file' : 'files'} · ` : ''}
                        opened {formatOpened(item.lastAccessed)}
                      </span>
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

      {/* New collection */}
      {newCollectionDialog && (
        <div className="ph-dialog-backdrop" onMouseDown={() => setNewCollectionDialog(null)}>
          <div className="ph-dialog" onMouseDown={(e) => e.stopPropagation()}>
            <h2>New collection</h2>
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
          <div className="ph-dialog" onMouseDown={(e) => e.stopPropagation()}>
            <h2>Rename collection</h2>
            {renameCollectionTarget.shared && (
              <p className="ph-dialog-hint">Renames it for everyone it's shared with.</p>
            )}
            <form
              onSubmit={(e) => {
                e.preventDefault();
                if (renameCollectionValue.trim()) {
                  renameCollection(renameCollectionTarget.id, renameCollectionValue.trim());
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
          <div className="ph-dialog" onMouseDown={(e) => e.stopPropagation()}>
            <h2>{confirmState.title}</h2>
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
          <div className="ph-dialog" onMouseDown={(e) => e.stopPropagation()}>
            <h2>Move "{pendingMove.name}" out of {pendingMove.fromName}?</h2>
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
        <span className="ph-footer-note">collections UI exploration</span>
      </footer>
    </div>
  );
}
