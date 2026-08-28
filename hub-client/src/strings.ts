/**
 * User-facing strings — the single source of truth for hub-client copy.
 *
 * Not full i18n, but one module that enforces terminology by structure:
 * each concept is named exactly once, and components reference it instead
 * of re-typing (and re-inventing) the words. When you add user-facing
 * copy, add it here in the same commit.
 *
 * Copy conventions (from the Phase 2 copy audit):
 * - The ellipsis is the single Unicode character …, never three dots.
 * - Buttons, labels, and menu items use sentence case ("Copy link",
 *   not "Copy Link").
 * - A menu item that opens a dialog ends with …; an immediate action
 *   ("Delete", "Copy link") does not.
 * - One verb per concept: projects are "switched" (header), files are
 *   "opened", assets are "added", items are "renamed". "Remove" is
 *   reserved for sync-aware removal (a project leaves this device but
 *   isn't deleted for others); "Delete" destroys a file.
 *
 * Scope: the app chrome (header, sidebar, outline, dialogs,
 * notifications, tabs) is migrated. ProjectsHome, the classic
 * ProjectSelector, and ProjectSetSetup keep local strings for now —
 * they share the common vocabulary below where trivially substitutable.
 *
 * Phase 2 deliverable of the UI/UX modernization plan.
 */

/** Shared vocabulary — actions and states used across components. */
export const common = {
  cancel: 'Cancel',
  create: 'Create',
  rename: 'Rename',
  delete: 'Delete',
  close: 'Close',
  dismiss: 'Dismiss',
  save: 'Save',
  back: 'Back',
  loading: 'Loading…',
  /** Recovery action on an error surface that can be re-attempted. */
  retry: 'Try again',
  /** Recovery action when the only retry is a full page reload. */
  reload: 'Reload',
} as const;

/** Top bars (ProjectTopBar + DocumentTopBar, the editor shell top row). */
export const header = {
  switchProject: 'Switch project',
  shareProject: 'Share this project',
  fullscreenPreview: 'Fullscreen preview',
  toggleSidebar: 'Toggle sidebar',
  sidebarDrawerLabel: 'Sidebar',
  noFileSelected: 'No file selected',
} as const;

/** SyncStatusBadge (project + document bottom bars). */
export const syncStatus = {
  /** Prefix when disconnected (browser offline / socket down / no peer). */
  savingLocally: 'Offline',
  syncedAgo: (ago: string) => `synced ${ago}`,
  neverSynced: 'not synced yet',
  synced: 'Synced',
  justNow: 'just now',
  underMinuteAgo: '<1 minute ago',
  tooltip: 'Sync status — click for connection details',
} as const;

/** FileSidebar — file tree, search, and the row actions menu. */
export const fileSidebar = {
  treeLabel: 'Files',
  newFile: 'New file',
  addAsset: 'Add asset',
  printableTooltip:
    "Open a printable version in a new tab (use the browser's Print to save as PDF)",
  printableLabel: 'Open printable version in a new tab',
  searchPlaceholder: 'Search files…',
  searchLabel: 'Search files',
  clearSearch: 'Clear search',
  resultsLabel: 'Search results',
  noMatches: 'No matches',
  emptyTitle: 'No files yet',
  emptyHint: 'Drop files here or click + to create',
  dropOverlay: 'Drop files to upload',
  actionsFor: (name: string) => `Actions for ${name}`,
  rowTooltip: (path: string, canOpenInNewTab: boolean) =>
    canOpenInNewTab ? `${path} — Ctrl/Cmd+click to open in new tab` : path,
  confirmDelete: (path: string) => `Delete ${path}?`,
  menuOpenInNewTab: 'Open in new tab',
  menuCopyLink: 'Copy link',
  menuRename: common.rename,
  menuDelete: common.delete,
} as const;

/** OutlinePanel. */
export const outline = {
  expand: (name: string) => `Expand ${name}`,
  collapse: (name: string) => `Collapse ${name}`,
  goTo: (name: string) => `Go to ${name}`,
  thumbnailAlt: (name: string) => `Thumbnail for ${name}`,
  loading: 'Loading outline',
  empty: 'No outline available',
} as const;

/** Sidebar section titles (rendered uppercase by the section header). */
export const sections = {
  files: 'FILES',
  outline: 'OUTLINE',
  project: 'PROJECT',
  status: 'STATUS',
  settings: 'SETTINGS',
  about: 'ABOUT',
} as const;

/** The three notification tiers (see components/notifications.css). */
export const notifications = {
  autoSaved: 'Auto-saved',
  updateAvailable: 'A new version is available.',
  ephemeralBanner: "Ephemeral session — edits won't be saved to disk",
  ephemeralTooltip:
    "Started without --allow-edit: edits sync live to everyone connected but are never written to the project's files. Restart the preview with --allow-edit to persist them.",
} as const;

/** Editor-shell dialogs. */
export const dialogs = {
  newFile: {
    title: 'New file',
    templateLabel: 'Template:',
    blank: 'Blank file',
    filenameLabel: 'Filename:',
    filenamePlaceholder: 'e.g., chapter1.qmd',
    errorRequired: 'Filename is required',
    errorInvalidChars: 'Filename contains invalid characters',
    errorExists: 'A file with this name already exists',
  },
  connectionStatus: {
    title: 'Connection status',
    browserNetwork: 'Browser network',
    browserOnline: 'Online',
    browserOffline: 'Offline',
    webSocket: 'WebSocket',
    noSocket: 'No socket',
    peerHandshake: 'Peer handshake',
    peerEstablished: 'Established',
    peerNone: 'Not established',
    connectionLog: 'Connection log',
    lastEphemeralMessage: 'Last ephemeral message received',
    lastRemoteChange: 'Last remote change',
    thisFile: 'This file',
    project: 'Project',
    morePatches: (n: number) => `… and ${n} more`,
    never: 'Never',
  },
  share: {
    title: 'Share project',
    warning: 'Anyone with this link can access and edit this project permanently.',
    warningDetail: 'Only share with people you trust. This link cannot be revoked.',
    linkLabel: 'Shareable link:',
    copyLink: 'Copy link',
    copied: 'Copied!',
  },
  newAsset: {
    title: 'Add asset to project',
    destinationLabel: 'Destination folder:',
    destinationPlaceholder: '(project root)',
    dropZone: 'Drag & drop files here',
    dropZoneOr: 'or',
    browse: 'Browse files',
    maxSize: (mb: number) => `Max file size: ${mb}MB`,
    remove: (name: string) => `Remove ${name}`,
    addMore: '+ Add more',
    upload: 'Upload',
    uploading: 'Uploading…',
    errorExists: (path: string) => `"${path}" already exists in the project`,
    errorDuplicateInBatch: 'Duplicate path with another file in this batch',
  },
} as const;

/** Sidebar tabs (PROJECT / STATUS / SETTINGS / ABOUT sections). */
export const tabs = {
  project: {
    nameLabel: 'Project Name',
    docIdLabel: 'Index Document ID',
    copyDocIdTooltip: (docId: string) => `Click to copy: ${docId}`,
    copy: 'Copy',
    copied: 'Copied!',
    syncServerLabel: 'Sync Server',
    exportZip: 'Export ZIP',
    exportingZip: 'Exporting…',
    screenshot: '📸 Screenshot Preview',
    capturingScreenshot: 'Capturing…',
    errorExport: 'Export failed',
    errorNoPreview: 'Preview pane not found',
    errorScreenshot: 'Failed to capture screenshot. Please try again.',
  },
  status: {
    rendererLabel: 'Renderer',
    loadingWasm: 'Loading WASM…',
    ready: 'Ready',
    error: 'Error',
    collaboratorsLabel: 'Collaborators',
    noOthers: 'No other users connected',
    othersHere: (n: number) => `${n} other${n === 1 ? '' : 's'} here`,
  },
  settings: {
    scrollSync: 'Scroll sync',
    scrollSyncDescription: 'Sync scroll position between editor and preview',
    collapseErrorOverlay: 'Collapse error overlay',
    collapseErrorOverlayDescription:
      'Show errors as a small indicator instead of expanded panel',
    nestingCursor: 'Nesting cursor',
    nestingCursorDescription:
      'Descend into nested list/quote blocks; edit each level cleanly.',
    richText: 'Rich-text editor',
    richTextDescription:
      'Edit paragraphs and headings as formatted text (WYSIWYG) instead of raw markdown. Other blocks still use the plain text editor.',
  },
  about: {
    tagline: 'A collaborative editor for Quarto projects.',
    linksLabel: 'Links',
    github: 'GitHub Repository',
    moreInfo: 'More Information',
    viewChangelog: 'View Changelog',
    unavailable: '(unavailable)',
    shortcutsLabel: 'Keyboard Shortcuts',
    buildInfoLabel: 'Build Info',
    commitLabel: 'commit',
    builtTooltip: (time: string, date: string) => `Built: ${time} · Commit date: ${date}`,
  },
} as const;

/** ViewToggleControl (markup / split / preview segmented control). */
export const viewToggle = {
  expandMarkup: 'Expand markup',
  markupView: 'Markup view',
  splitEqually: 'Split equally',
  splitView: 'Split view',
  expandPreview: 'Expand preview',
  previewView: 'Preview view',
} as const;

/** ReplayDrawer transport and overlays. */
export const replay = {
  title: 'Replay',
  collapse: 'Collapse replay',
  close: 'Close replay',
  skipToStart: 'Skip to start',
  stepBackward: 'Step backward',
  pause: 'Pause',
  play: 'Play',
  stepForward: 'Step forward',
  skipToEnd: 'Skip to end',
  playbackSpeed: 'Playback speed',
  position: 'Replay position',
  restore: 'Restore',
  changeTooltip: (n: number) => `Change ${n}`,
} as const;
