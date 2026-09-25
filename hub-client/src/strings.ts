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
 * ProjectSelector, and ProjectSetError keep local strings for now —
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
  printableTooltip:
    "Open a printable version in a new tab (use the browser's Print to save as PDF)",
  printableLabel: 'Open printable version in a new tab',
  toggleSidebar: 'Toggle sidebar',
  sidebarDrawerLabel: 'Sidebar',
  noFileSelected: 'No file selected',
} as const;

/** SyncStatusBadge (FILES section + document bottom bar). */
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
  menuCopyLink: 'Copy share link',
  menuRename: common.rename,
  menuDelete: common.delete,
  newFolder: 'New folder',
  menuNewFileInside: 'New file inside',
  menuNewFolderInside: 'New folder inside',
  menuNewAssetInside: 'New asset inside',
  menuDownloadFolder: 'Download as zip',
  menuMove: 'Move…',
  menuDownload: 'Download',
  menuDeleteFolder: 'Delete folder',
  deleteFolderNotEmpty: 'Only empty folders can be deleted',
  folderEmpty: 'Empty',
  folderActionsFor: (name: string) => `Actions for folder ${name}`,
  confirmDeleteFolder: (path: string) => `Delete folder ${path}?`,
} as const;

/** FolderPicker (New-file dialog's folder dropdown). */
export const folderPicker = {
  label: 'Choose folder',
  root: '(project root)',
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

/** Outbound links to Quarto's own sites. */
export const links = {
  /** The Quarto Hub project site — every "Learn more" leads here. */
  quartoHub: 'https://quarto-dev.github.io/quarto-hub/',
} as const;

/**
 * The landing page at quarto-hub.com — what anyone without a session
 * sees, including someone arriving from an invite link (bd-g0uyp2v1).
 *
 * Wording is borrowed from the project site (links.quartoHub) so the two
 * describe the product the same way. One claim there is deliberately not
 * repeated: that a project link needs no account or Quarto install. That
 * is the product's intent, but this deployment is allowlisted and the
 * reader is looking at a sign-in button, so it would contradict the page
 * it sits on.
 */
export const landing = {
  product: 'Quarto Hub',
  /** Rendered as two lines: the second sentence is the turn. */
  taglineLead: 'Prose and code belong in one place.',
  taglineFollow: 'So do the people.',
  /**
   * Three sentences, each ending on its own new idea (Gopen's stress
   * position): live render, one .qmd, real-time collaboration. The
   * third deliberately does *not* end "…on the same project in Quarto
   * Hub" — the product name is the paragraph's own opening subject and
   * the reader is already on the site, so putting it last spends the
   * emphatic slot on the oldest information in the paragraph.
   */
  what:
    'Quarto Hub is a Quarto editor in the browser that renders while you type. ' +
    'Edit the markdown or the rendered page, and either way it is the same .qmd. ' +
    'Share a link and your team can collaborate on the same project in real time.',
  inviteOnly: 'quarto-hub.com is experimental and currently available by invite only.',
  learnMore: 'Learn more about Quarto Hub',
} as const;

/**
 * The demo disclaimer (bd-m6u9qu3u): what not to enter, and that nothing
 * entered is protected or returned. Rendered by components/DemoDisclaimer
 * on every card reachable without a session — the landing / sign-in
 * screen and the invite landing cards — above their sign-in button.
 *
 * Structured rather than written as markdown: those cards render before
 * the WASM markdown renderer (the About tab's) exists, and the formatting
 * is fixed anyway — a heading, then two paragraphs that each open with a
 * bold single-sentence lead. DemoDisclaimer builds the markup from this
 * shape, so each `lead` must stay one sentence and end with its own
 * period; the component joins `lead` and `body` with a space. The same
 * two paragraphs open resources/more-info.md (the About tab's "More
 * information"); keep the wording in step when editing either.
 */
export const demoDisclaimer = {
  heading: 'Disclaimer',
  useTestData: {
    lead: 'Use test data, not real data.',
    body:
      'This is a live demo of a product still in development. ' +
      'Please don’t enter anything sensitive, confidential, or proprietary — ' +
      'no customer data, credentials, internal business info, or anyone else’s ' +
      'personal information. If you wouldn’t post it on a public forum, ' +
      'don’t put it here.',
  },
  notProtected: {
    lead: 'Anything you enter is not protected, and we won’t return it.',
    body:
      'Data you share during this demo may be logged, cached, or otherwise ' +
      'retained by Posit and any third-party services the demo relies on. ' +
      'We don’t guarantee confidentiality, security, or deletion, and we can’t ' +
      'retrieve or return data you enter once the demo session ends. ' +
      'Enter information only if you’re comfortable with that.',
  },
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
    folderLabel: 'Folder:',
    typeLabel: 'File type:',
    typeOther: 'Other',
    extensionLabel: 'Extension:',
    extensionPlaceholder: 'e.g., css',
    filenameLabel: 'Filename:',
    filenamePlaceholder: 'e.g., chapter1',
    errorRequired: 'Filename is required',
    errorExtensionRequired: 'Enter a file extension',
    errorInvalidChars: 'Filename contains invalid characters',
    errorExists: 'A file with this name already exists',
  },
  moveFile: {
    title: (name: string) => `Move ${name}`,
    folderLabel: 'Folder:',
    move: 'Move',
    errorExists: 'A file with this name already exists in that folder',
  },
  newFolder: {
    title: 'New folder',
    parentLabel: 'Folder:',
    nameLabel: 'Name:',
    namePlaceholder: 'e.g., chapters',
    errorRequired: 'Folder name is required',
    errorInvalidChars: 'Folder name contains invalid characters',
    errorExists: 'A folder with this name already exists',
    errorFileExists: 'A file with this name already exists',
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
    lastReadReceipt: 'Last read receipt',
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
    destinationLabel: 'Folder:',
    dropZone: 'Drag & drop files here',
    dropZoneOr: 'or',
    browse: 'Browse files',
    maxSize: (mb: number) => `Max file size: ${mb}MB`,
    sidebarHint: 'Drag & drop files directly into the sidebar or .qmd text to upload.',
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
    documentBranches: 'Document branches (experimental)',
    documentBranchesDescription:
      'Fork a private local branch of a document, compare it with main, and merge it back. Branches are not shared with collaborators.',
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
