import { useState, useCallback, useEffect, useRef, lazy, Suspense } from 'react';
import type { ProjectEntry, FileEntry } from '@quarto/preview-renderer/types/project';
import ProjectSelector from './components/ProjectSelector';
import ProjectsHome from './components/ProjectsHome';
import JoinCollectionLanding from './components/JoinCollectionLanding';
import ProjectSetSetup from './components/ProjectSetSetup';

// Lazy-loaded dev harness — only fetched when navigating to #/dev/... routes.
// In production builds, the DevRoute type is never parsed, so this code is never reached.
const DevHarnessRaw = lazy(() => import('./components/DevHarness'));
function DevHarnessLazy({ page }: { page: string }) {
  return (
    <Suspense fallback={null}>
      <DevHarnessRaw page={page} />
    </Suspense>
  );
}
import Editor from './components/Editor';
import { ErrorBoundary } from './components/ErrorBoundary';
import Toast from './components/Toast';
import { ViewModeProvider } from './components/ViewModeContext';
import { LoginScreen } from './components/auth/LoginScreen';
import { readAuthErrorReason } from './auth/authError';
import {
  connect,
  disconnect,
  setSyncHandlers,
  getFileContent,
  applyEditorOperations,
  createNewProject,
  type ActorIdentity,
  type CaptureRef,
  type EditorContentChange,
} from '@quarto/preview-runtime';
import type { ProjectFile } from '@quarto/preview-runtime';
import * as projectStorage from './services/projectStorage';
import { installDebugApi } from './services/debugApi';
import { getUserIdentity, updateUserName, actorIdFromUserId } from './services/userSettings';
import { useRouting } from './hooks/useRouting';
import { useCollectionSets } from './hooks/useCollectionSets';
import { useAuth } from './hooks/useAuth';
import { useAuthProbe } from './hooks/useAuthProbe';
import { useSessionKeepAlive } from './hooks/useSessionKeepAlive';
import { useExecutionChannel } from './hooks/useExecutionChannel';
import { usePreviewSession } from './hooks/usePreviewSession';
import { resolveActorId as resolveActorIdRequest } from './services/authService';
import type { Route, ShareRoute, LinkProjectSetRoute } from './utils/routing';
import { resolveSyncServerUrl, DEFAULT_SYNC_SERVER, parseHashRoute, hubPath } from './utils/routing';
import { isEphemeralStorage } from './services/ephemeralStorage';
import { fetchPreviewSessionConfig } from './services/previewConfig';
import type { StorageKind } from '@quarto/quarto-sync-client';
import './App.css';

/**
 * Production budget for the initial peer connect. `waitForPeer` resolves the
 * *instant* the peer connects, so in the common (online) case this adds only
 * the real connect latency and lets `connect()` resolve as Online — the header
 * (which mounts only after connect() resolves) then shows Online right away
 * with the *live* document, instead of the 1 ms probe that always resolved
 * offline-first and made the indicator flash Offline → Online. If the connect
 * is slower than this budget the header mounts Offline and flips to Online when
 * the peer lands — the rare tail, and still an improvement on always-first
 * Offline. A genuinely-offline user waits at most this long before cached
 * content appears (bounded regression of offline-first).
 */
const PRODUCTION_PEER_TIMEOUT_MS = 400;

/**
 * Connect to a sync server and load all file contents into a Map.
 * Shared by every code path that opens a project.
 */
async function connectAndLoadContents(
  syncServer: string,
  indexDocId: string,
  actorId?: string,
  screenName?: string,
  color?: string,
): Promise<{ files: FileEntry[]; contents: Map<string, string> }> {
  // E2E builds: honour __QUARTO_TEST_ACTOR_ID__ so getActorId() returns a
  // stable, known value inside the preview iframe. Tree-shaken in production.
  if (import.meta.env.VITE_E2E === '1') {
    actorId = (window as any).__QUARTO_TEST_ACTOR_ID__ as string | undefined ?? actorId;
  }
  // Production gives the peer a modest budget (PRODUCTION_PEER_TIMEOUT_MS) so
  // the common (online) open resolves as Online with the live document, rather
  // than the 1 ms offline-first probe that made the indicator always flash
  // Offline → Online. waitForPeer resolves the instant the peer connects, so a
  // fast connection pays only its real latency; a genuinely-offline user waits
  // at most the budget before cached content appears.
  //
  // The smoke-all E2E env always starts with EMPTY storage and must sync every
  // doc from the (local) server, so it needs a much longer wait: opening before
  // the socket connects means loadFileDocuments races it — under CI contention
  // the render-target doc loses, is marked unavailable, and the preview fails
  // "Path not found" (stage EDITOR_NO_PREVIEW; sometimes the index loses it too
  // → CONNECT_STALL).
  const peerTimeoutMs = import.meta.env.VITE_E2E === '1' ? 15000 : PRODUCTION_PEER_TIMEOUT_MS;
  // Ephemeral storage mode (bd-sw4xy1vw): the q2 preview embed build
  // keeps the automerge document cache in memory — each preview session
  // is a fresh origin, so a persisted cache could never hit and would
  // just accumulate in IndexedDB.
  const storage: StorageKind = isEphemeralStorage() ? 'memory' : 'indexeddb';
  const files = await connect(resolveSyncServerUrl(syncServer), indexDocId, actorId, screenName, color, { peerTimeoutMs, storage });
  const contents = new Map<string, string>();
  for (const file of files) {
    const content = getFileContent(file.path);
    if (content !== null) contents.set(file.path, content);
  }
  return { files, contents };
}

/** Whether auth is configured (build-time env var). */
const AUTH_ENABLED = !!import.meta.env.VITE_GOOGLE_CLIENT_ID;


function App() {
  const {
    auth,
    loading: authLoading,
    logout,
    sessionExpired,
    expireSession,
    applyAuth,
  } = useAuth();

  const [project, setProject] = useState<ProjectEntry | null>(null);
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [isConnecting, setIsConnecting] = useState(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [fileContents, setFileContents] = useState<Map<string, string>>(new Map());
  const [showSaveToast, setShowSaveToast] = useState(false);
  const [screenName, setScreenName] = useState<string | undefined>();
  const [cursorColor, setCursorColor] = useState<string | undefined>();
  // Local user id → stable Automerge actor when auth is disabled (local-prod).
  const [localActorId, setLocalActorId] = useState<string | undefined>();
  const [identities, setIdentities] = useState<Record<string, ActorIdentity>>({});
  // bd-sfet3264 (Phase 1C): IndexDocument V2 capture sidecar (path → CaptureRef).
  // Populated by the sync client's onCapturesChange; threaded down to the
  // preview so recorded engine output can be spliced into the rendered AST.
  const [captures, setCaptures] = useState<Record<string, CaptureRef>>({});
  const [isOnline, setIsOnline] = useState<boolean>(false);

  // bd-sfet3264 (Phase 2D + Phase 4b): track which q2 executors are online for
  // the connected project (via the index-handle capability beacon) and expose
  // a way to ask one to run a document. Beacons come from a connected
  // `q2 provide-hub` (Phase 4).
  const { executors: liveExecutors, requestExecution } = useExecutionChannel(
    isOnline,
    project?.indexDocId ?? null,
  );

  // While a project's sync is disconnected, check whether the disconnect is
  // actually an auth rejection (browsers hide the WS upgrade status). Only
  // definitive 401/403 evidence ever clears auth — never network errors.
  // Past the token's exp, useAuth's expiry timer logs out on the first 401
  // (preempting this probe's two-strike); the probe governs earlier drops.
  useAuthProbe({
    enabled: AUTH_ENABLED && !!auth && !!project && !isOnline,
    onAuthRejected: expireSession,
  });

  // While sync is online, keep the sliding session alive: WS traffic
  // never slides the server-side idle window (validate-once at
  // upgrade), so a periodic /auth/me is the keep-alive — and its
  // response carries the slid expiry back into useAuth's schedules.
  useSessionKeepAlive({
    enabled: AUTH_ENABLED && !!auth && isOnline,
    onAuthState: applyAuth,
    onAuthRejected: expireSession,
  });

  // Project set management (synced project list)
  const [projectSetState, projectSetActions] = useCollectionSets();

  // Keep a ref so callbacks that intentionally omit projectSetState from their
  // dependency arrays (to avoid re-creation churn) can still read the latest status.
  const projectSetStateRef = useRef(projectSetState);
  projectSetStateRef.current = projectSetState;

  // Resolve the per-project actor ID before opening a document. See
  // `resolveActorIdRequest` for the three-valued contract; callers abandon
  // the open only on `null` (auth failure), proceed on `string`/`undefined`.
  const resolveActorId = useCallback(
    (indexDocId: string) => resolveActorIdRequest(indexDocId, AUTH_ENABLED, expireSession, localActorId),
    [expireSession, localActorId],
  );

  // Capture the auth error reason from the redirect query param (once,
  // before the URL is cleaned). `undefined` means no error; `''` means a
  // bare `/?auth_error` from a hub predating the reason codes, which is
  // still an error — see readAuthErrorReason.
  const [authErrorReason] = useState(() => {
    const reason = readAuthErrorReason(window.location.search);
    if (reason !== undefined) {
      window.history.replaceState(null, '', window.location.pathname + window.location.hash);
    }
    return reason;
  });

  // Capture the ephemeral-hub flag from the boot URL (once, before the
  // share handler below clears the hash from the address bar). Only
  // `q2 preview --ui editor` emits it: the serving hub is a throwaway
  // per-session server, so project-set onboarding is skipped entirely
  // (bd-zf4ryvuq). The preview-embed build (VITE_EPHEMERAL_STORAGE=1,
  // bd-sw4xy1vw) only ever serves such a hub, so the flag holds for the
  // whole artifact — including after a reload, when the boot URL's
  // share hash is gone.
  const [ephemeralHub] = useState(() => {
    if (isEphemeralStorage()) return true;
    const bootRoute = parseHashRoute(window.location.hash);
    return bootRoute.type === 'share' && bootRoute.ephemeral === true;
  });

  // `q2 preview` session config (bd-ov4gqk3m): when the serving server
  // is a preview started without --allow-edit, the editor shows an
  // ephemeral-session banner. Null on a standalone hub (no such
  // endpoint), which never shows the banner. Unlike the boot-URL flag
  // above this survives reloads and works for --join guests, whose
  // proxy splices every connection through to the host.
  const previewSession = usePreviewSession();

  // Load screen name from IndexedDB (for identity mapping in Automerge docs).
  // When auth is enabled, wait for it to resolve so we can upgrade anonymous
  // names to the OIDC display name on first login. Without auth, load immediately.
  useEffect(() => {
    if (AUTH_ENABLED && authLoading) return;
    getUserIdentity().then(async (settings) => {
      setLocalActorId(actorIdFromUserId(settings.userId));
      if (auth?.name && settings.createdAt === settings.updatedAt) {
        const updated = await updateUserName(auth.name);
        setScreenName(updated.userName);
        setCursorColor(updated.userColor);
      } else {
        setScreenName(settings.userName);
        setCursorColor(settings.userColor);
      }
    });
  }, [authLoading]);


  // Track if we've done the initial URL-based navigation
  const initialLoadRef = useRef(false);

  // UI exploration (explore/projects-collections-ui): choose between the new
  // collections-based projects home and the classic modal selector. Persisted so
  // UX testing can flip back and forth across reloads.
  const [uiVariant, setUiVariant] = useState<'collections' | 'classic'>(() =>
    localStorage.getItem('qh-ui-variant') === 'classic' ? 'classic' : 'collections',
  );
  const switchUiVariant = useCallback((variant: 'collections' | 'classic') => {
    localStorage.setItem('qh-ui-variant', variant);
    setUiVariant(variant);
  }, []);

  // URL-based routing
  const {
    route,
    navigateToProjectSelector,
    navigateToProject,
    navigateToFile,
  } = useRouting();

  // Invite-first onboarding: opening a collection invite establishes a
  // personal root behind the landing screen, so the invitee only ever sees
  // "join Team docs" — never the setup or migration prompts. A browser with a
  // stray legacy project (needs-migration) migrates it silently into the new
  // root (non-destructive: the legacy store is retained); a fresh browser
  // (needs-setup) just creates an empty root. The landing shows "Connecting…"
  // until the root is ready, then Join subscribes to the collection.
  const inviteRootInitiatedRef = useRef(false);
  useEffect(() => {
    if (route.type !== 'join-collection' || inviteRootInitiatedRef.current) return;
    if (projectSetState.status === 'needs-setup') {
      inviteRootInitiatedRef.current = true;
      projectSetActions.createProjectSet(DEFAULT_SYNC_SERVER);
    } else if (projectSetState.status === 'needs-migration') {
      // Fire once: migrateProjects resets to needs-migration on failure, so
      // an unguarded effect would retry-loop against an unreachable server.
      inviteRootInitiatedRef.current = true;
      projectSetActions.migrateProjects(DEFAULT_SYNC_SERVER);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [route.type, projectSetState.status]);

  // Ephemeral preview boot (bd-zf4ryvuq): same invite-first pattern as
  // join-collection above. The user asked for a preview, not project
  // management, so establish the personal root silently — create on a
  // fresh browser, migrate when legacy IDB projects exist — and never
  // show the setup/migration screens. DEFAULT_SYNC_SERVER is '/ws' in
  // the preview-embed build, i.e. the ephemeral hub itself; the root
  // doc lives in IndexedDB and re-syncs to whatever ephemeral hub serves
  // this origin next.
  const ephemeralRootInitiatedRef = useRef(false);
  useEffect(() => {
    if (!ephemeralHub || ephemeralRootInitiatedRef.current) return;
    if (projectSetState.status === 'needs-setup') {
      ephemeralRootInitiatedRef.current = true;
      projectSetActions.createProjectSet(DEFAULT_SYNC_SERVER);
    } else if (projectSetState.status === 'needs-migration') {
      // Fire once: migrateProjects resets to needs-migration on failure, so
      // an unguarded effect would retry-loop against an unreachable server.
      ephemeralRootInitiatedRef.current = true;
      projectSetActions.migrateProjects(DEFAULT_SYNC_SERVER);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ephemeralHub, projectSetState.status]);

  // Denormalize a peek summary onto this user's project-set entry while a
  // project is open. Kept current as files and identities change (both are
  // low-frequency: file add/remove/rename, presence join). This is a per-user
  // cache of "the project as I last saw it" — list surfaces (cards, peek)
  // read it so they never need a sync connection per project.
  useEffect(() => {
    if (!project || projectSetState.status !== 'connected' || files.length === 0) return;
    // You are always a contributor; presence identities fill in everyone else
    // (they can lag connection, so self is added explicitly).
    const seen = screenName ? [{ name: screenName, color: cursorColor ?? '#447099' }] : [];
    for (const i of Object.values(identities)) {
      if (!seen.some((s) => s.name === i.name)) seen.push({ name: i.name, color: i.color });
    }
    projectSetActions.updateProjectSummary(project.indexDocId, {
      fileCount: files.length,
      topFiles: files.slice(0, 5).map((f) => f.path),
      contributors: seen.slice(0, 6),
      asOf: new Date().toISOString(),
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project, files, identities, screenName, cursorColor, projectSetState.status]);

  // Live refs for the dev-only console debug API. The API itself
  // is installed once (per the gate below) and reads current state
  // through these refs, so it doesn't churn on every project /
  // route change. See `services/debugApi.ts` and bd-2rv8.
  const projectRef = useRef(project);
  projectRef.current = project;
  const filesRef = useRef<FileEntry[]>(files);
  filesRef.current = files;
  const routeRef = useRef<Route>(route);
  routeRef.current = route;
  const navigateToFileRef = useRef(navigateToFile);
  navigateToFileRef.current = navigateToFile;

  useEffect(() => {
    const enabled =
      import.meta.env.DEV ||
      (typeof localStorage !== 'undefined' &&
        localStorage.getItem('quartoDebug') === '1');
    if (!enabled) return;
    return installDebugApi({
      getProject: () => projectRef.current,
      getFiles: () => filesRef.current,
      getActiveFile: () => {
        const r = routeRef.current;
        return r.type === 'file' ? r.filePath : null;
      },
      setActiveFile: (path: string) => {
        const p = projectRef.current;
        if (!p) {
          throw new Error('quartoDebug.setActiveFile: no project loaded');
        }
        navigateToFileRef.current(p.id, path);
      },
    });
  }, []);

  // Handle browser back/forward navigation
  // We use a separate effect instead of the onRouteChange callback to avoid
  // circular dependencies (the callback would need navigateToProjectSelector
  // which isn't defined until after useRouting returns).
  const prevRouteRef = useRef<Route>(route);
  useEffect(() => {
    const prevRoute = prevRouteRef.current;
    prevRouteRef.current = route;

    // Skip if route hasn't changed (this effect also runs on initial mount)
    if (
      prevRoute.type === route.type &&
      (route.type === 'project-selector' ||
        ((route.type === 'project' || route.type === 'file') &&
          (prevRoute.type === 'project' || prevRoute.type === 'file') &&
          route.projectId === prevRoute.projectId))
    ) {
      return;
    }

    // Handle route change (browser back/forward)
    const handleRouteChange = async () => {
      if (route.type === 'project-selector') {
        // Navigating back to project selector
        await disconnect();
        setProject(null);
        setFiles([]);
        setFileContents(new Map());
        setConnectionError(null);
      } else if (route.type === 'project' || route.type === 'file') {
        // Navigating to a project (possibly different from current)
        const currentProjectId = project?.id;
        if (route.projectId !== currentProjectId) {
          // Different project - need to load it
          const targetProject = await projectStorage.getProject(route.projectId);
          if (targetProject) {
            setIsConnecting(true);
            setConnectionError(null);
            try {
              const newActorId = await resolveActorId(targetProject.indexDocId);
              if (newActorId === null) return;
              const { files: loadedFiles, contents } = await connectAndLoadContents(targetProject.syncServer, targetProject.indexDocId, newActorId, screenName, cursorColor);
              setProject(targetProject);
              setFiles(loadedFiles);
              setFileContents(contents);
            } catch (err) {
              setConnectionError(err instanceof Error ? err.message : String(err));
              navigateToProjectSelector({ replace: true });
            } finally {
              setIsConnecting(false);
            }
          } else {
            // Project not found in IndexedDB
            setConnectionError(`Project not found. It may have been deleted.`);
            navigateToProjectSelector({ replace: true });
          }
        }
        // If same project, file navigation will be handled by Editor (Phase 2)
      }
    };

    handleRouteChange();
  }, [route, project, navigateToProjectSelector]);

  // Handle initial URL-based navigation
  useEffect(() => {
    if (initialLoadRef.current) return;
    initialLoadRef.current = true;

    const loadFromUrl = async () => {
      // Handle link-project-set URLs
      if (route.type === 'link-project-set') {
        // SECURITY: Immediately clear the URL
        navigateToProjectSelector({ replace: true });

        const linkRoute = route as LinkProjectSetRoute;

        if (!linkRoute.syncServer) {
          setConnectionError('This project set link is incomplete.');
          return;
        }

        // Normalize the docId
        const normalizedDocId = linkRoute.projectSetDocId.startsWith('automerge:')
          ? linkRoute.projectSetDocId
          : `automerge:${linkRoute.projectSetDocId}`;

        // Check if we have legacy projects to merge
        const legacy = await projectStorage.listProjects();
        if (legacy.length > 0) {
          await projectSetActions.mergeIntoProjectSet(normalizedDocId, linkRoute.syncServer);
        } else {
          await projectSetActions.linkProjectSet(normalizedDocId, linkRoute.syncServer);
        }
        return;
      }

      // Shared by the share-route branch and the ephemeral
      // reload-recovery branch below: find-or-create the local project
      // entry for a shared document, then connect and open the file.
      const connectToSharedProject = async (share: {
        indexDocId: string;
        syncServer: string;
        name: string;
        filePath: string;
      }): Promise<void> => {
        // Normalize the indexDocId (add 'automerge:' prefix if not present)
        const normalizedIndexDocId = share.indexDocId.startsWith('automerge:')
          ? share.indexDocId
          : `automerge:${share.indexDocId}`;

        // Check if we already have this project locally, or auto-create it
        let targetProject = await projectStorage.getProjectByIndexDocId(normalizedIndexDocId);
        if (!targetProject) {
          targetProject = await projectStorage.addProject(
            normalizedIndexDocId,
            share.syncServer,
            share.name
          );
        }

        // Also add to the synced project set (if connected)
        if (projectSetStateRef.current.status === 'connected') {
          try {
            projectSetActions.addProject({
              indexDocId: normalizedIndexDocId,
              syncServer: share.syncServer,
              description: share.name,
            });
          } catch {
            // Non-fatal: project set update failed, but project is in IDB
          }
        }

        setIsConnecting(true);
        setConnectionError(null);
        try {
          const newActorId = await resolveActorId(targetProject.indexDocId);
          if (newActorId === null) return;
          const { files: loadedFiles, contents } = await connectAndLoadContents(targetProject.syncServer, targetProject.indexDocId, newActorId, screenName, cursorColor);
          setProject(targetProject);
          setFiles(loadedFiles);
          setFileContents(contents);

          navigateToFile(targetProject.id, share.filePath, { replace: true });
        } catch (err) {
          setConnectionError(err instanceof Error ? err.message : String(err));
        } finally {
          setIsConnecting(false);
        }
      };

      // Handle shareable link URLs
      if (route.type === 'share') {
        // SECURITY: Immediately clear the URL to prevent indexDocId from appearing
        // in browser history, bookmarks, or being accidentally shared.
        navigateToProjectSelector({ replace: true });

        const shareRoute = route as ShareRoute;

        // Validate required fields
        if (!shareRoute.syncServer || !shareRoute.filePath || !shareRoute.name) {
          setConnectionError(
            'This share link is incomplete. Please ask the sender to share a new link.'
          );
          return;
        }

        await connectToSharedProject({
          indexDocId: shareRoute.indexDocId,
          syncServer: shareRoute.syncServer,
          name: shareRoute.name,
          filePath: shareRoute.filePath,
        });
        return;
      }

      if (route.type === 'project' || route.type === 'file') {
        // URL specifies a project - try to load it
        const targetProject = await projectStorage.getProject(route.projectId);
        if (targetProject) {
          setIsConnecting(true);
          setConnectionError(null);
          try {
            const newActorId = await resolveActorId(targetProject.indexDocId);
            if (newActorId === null) return;
            const { files: loadedFiles, contents } = await connectAndLoadContents(targetProject.syncServer, targetProject.indexDocId, newActorId, screenName, cursorColor);
            setProject(targetProject);
            setFiles(loadedFiles);
            setFileContents(contents);

          } catch (err) {
            setConnectionError(err instanceof Error ? err.message : String(err));
            navigateToProjectSelector({ replace: true });
          } finally {
            setIsConnecting(false);
          }
        } else if (isEphemeralStorage()) {
          // Ephemeral storage mode (bd-sw4xy1vw) keeps no project
          // records across page reloads. Rebuild the session from the
          // preview server's boot params (every editor-UI session
          // serves them at /api/preview/config, bd-7htq16rx) instead of
          // reporting a missing project. A non-preview server answers
          // without editorBoot and falls through to the error.
          const config = await fetchPreviewSessionConfig();
          if (config?.editorBoot) {
            await connectToSharedProject({
              indexDocId: config.editorBoot.indexDocId,
              syncServer: hubPath('/ws'),
              name: config.editorBoot.name,
              filePath: config.editorBoot.file,
            });
            return;
          }
          setConnectionError(`Project not found. It may have been deleted.`);
          navigateToProjectSelector({ replace: true });
        } else {
          // Project not found - show error and stay on project selector
          setConnectionError(`Project not found. It may have been deleted.`);
          navigateToProjectSelector({ replace: true });
        }
      }
      // If route is 'project-selector', do nothing - we're already there
    };

    loadFromUrl();
  }, [route, navigateToProjectSelector, navigateToProject, navigateToFile]);

  // Disconnect sync when auth is lost (token expired or user logged out).
  // Without this, the WebSocket adapter keeps retrying with an expired cookie
  // and the user sees "Connection lost" instead of the login screen.
  useEffect(() => {
    if (AUTH_ENABLED && !auth && !authLoading && project) {
      disconnect();
      setProject(null);
      setFiles([]);
      setFileContents(new Map());
      setConnectionError(null);

    }
  }, [auth, authLoading, project]);

  // Intercept Ctrl+S / Cmd+S to prevent browser save dialog
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        setShowSaveToast(true);
      }
    };

    // Listen for save events from preview iframe
    const handleMessage = (e: MessageEvent) => {
      if (e.data?.type === 'hub-client-save') {
        setShowSaveToast(true);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('message', handleMessage);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('message', handleMessage);
    };
  }, []);

  // Set up sync handlers
  useEffect(() => {
    setSyncHandlers({
      onFilesChange: (newFiles) => {
        setFiles(newFiles);
      },
      onIdentitiesChange: (newIdentities) => {
        setIdentities(newIdentities);
      },
      onCapturesChange: (newCaptures) => {
        setCaptures(newCaptures);
      },
      onFileContent: (path, content, _patches) => {
        // Note: patches are ignored - we use diff-based sync in Editor.tsx
        setFileContents((prev) => {
          const next = new Map(prev);
          next.set(path, content);
          return next;
        });
      },
      onConnectionChange: (connected) => {
        setIsOnline(connected);
        if (!connected && project) {
          // Connection lost - show error
          setConnectionError('Connection lost - working offline');
        } else if (connected && connectionError === 'Connection lost - working offline') {
          // Connection restored - clear error
          setConnectionError(null);
        }
      },
      onError: (error) => {
        setConnectionError(error.message);
      },
    });
  }, [project]);

  const handleSelectProject = useCallback(async (selectedProject: ProjectEntry, filePathOverride?: string) => {
    setIsConnecting(true);
    setConnectionError(null);

    try {
      const newActorId = await resolveActorId(selectedProject.indexDocId);
      if (newActorId === null) return;
      const { files: loadedFiles, contents } = await connectAndLoadContents(selectedProject.syncServer, selectedProject.indexDocId, newActorId, screenName, cursorColor);
      setProject(selectedProject);
      setFiles(loadedFiles);
      setFileContents(contents);
      

      if (filePathOverride) {
        navigateToFile(selectedProject.id, filePathOverride, { replace: true });
      } else {
        navigateToProject(selectedProject.id, { replace: true });
      }
    } catch (err) {
      setConnectionError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsConnecting(false);
    }
  }, [navigateToProject, navigateToFile, resolveActorId, screenName, cursorColor]);

  const handleDisconnect = useCallback(async () => {
    await disconnect();
    setProject(null);
    setFiles([]);
    setFileContents(new Map());
    setConnectionError(null);
    // Update URL to show project selector
    navigateToProjectSelector({ replace: true });
  }, [navigateToProjectSelector]);

  const handleContentOperations = useCallback((path: string, changes: EditorContentChange[]) => {
    applyEditorOperations(path, changes);
  }, []);

  const handleProjectCreated = useCallback(async (
    scaffoldFiles: ProjectFile[],
    title: string,
    _projectType: string,
    syncServer: string
  ) => {
    setIsConnecting(true);
    setConnectionError(null);

    try {
      // Convert scaffold files to the format expected by createNewProject
      const files = scaffoldFiles.map(f => ({
        path: f.path,
        content: f.content,
        contentType: f.content_type,
        mimeType: f.mime_type,
      }));

      // Create the Automerge documents. The resolveActorId callback is
      // called after the index doc is created (to derive the HMAC actor
      // ID from the indexDocId) but before any file docs are written.
      //
      // Resolve only the runtime connection value (the WS adapter needs an
      // absolute ws(s):// URL); the portable `syncServer` is what we store
      // and share below, so it stays origin-independent under a subpath.
      const result = await createNewProject({
        syncServer: resolveSyncServerUrl(syncServer),
        files,
        // Ephemeral storage mode (bd-sw4xy1vw): no IndexedDB cache.
        storage: isEphemeralStorage() ? 'memory' : 'indexeddb',
      }, undefined, screenName, cursorColor, resolveActorId);

      // Store the project in IndexedDB
      const projectEntry = await projectStorage.addProject(
        result.indexDocId,
        syncServer,
        title
      );

      // Also add to the synced project set
      if (projectSetStateRef.current.status === 'connected') {
        try {
          projectSetActions.addProject({
            indexDocId: result.indexDocId,
            syncServer,
            description: title,
          });
        } catch {
          // Non-fatal
        }
      }

      // Set up the project state
      setProject(projectEntry);
      setFiles(result.files);

      // Initialize file contents from the scaffold
      const contents = new Map<string, string>();
      for (const file of scaffoldFiles) {
        if (file.content_type === 'text') {
          contents.set(file.path, file.content);
        }
      }
      setFileContents(contents);

      // Update URL to reflect the new project
      navigateToProject(projectEntry.id, { replace: true });
    } catch (err) {
      setConnectionError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsConnecting(false);
    }
  }, [navigateToProject, resolveActorId, screenName, cursorColor]);

  // Auth gate: when auth is enabled, require login before showing the app.
  // Show a loading spinner while checking auth status to avoid login flash.
  if (AUTH_ENABLED && authLoading) {
    return (
      <div className="project-selector" style={{ alignItems: 'center', justifyContent: 'center' }}>
        <div style={{ color: 'var(--text-secondary)', fontSize: '14px' }}>Loading...</div>
      </div>
    );
  }

  if (AUTH_ENABLED && !auth) {
    return (
      <LoginScreen
        errorReason={authErrorReason}
        message={sessionExpired ? 'Your session expired — please sign in again.' : undefined}
      />
    );
  }

  // Gate on screen name being loaded (fast IndexedDB read).
  // Prevents connects from firing before the identity can be written.
  if (screenName === undefined) {
    return (
      <div className="project-selector" style={{ alignItems: 'center', justifyContent: 'center' }}>
        <div style={{ color: 'var(--text-secondary)', fontSize: '14px' }}>Loading...</div>
      </div>
    );
  }

  // Dev harness: render components in isolation for visual testing.
  // Only available in development builds; the DevRoute type is never parsed in production.
  if (route.type === 'dev') {
    return <DevHarnessLazy page={route.page} />;
  }

  // Collection invite landing (explore/projects-collections-ui). Always shown
  // for an invite route, ahead of the setup/migration screens: the effect
  // above establishes the personal root (creating or silently migrating) so
  // the invitee only ever sees "join <collection>", never onboarding prompts.
  if (route.type === 'join-collection') {
    return (
      <JoinCollectionLanding
        route={route}
        status={projectSetState.status}
        onSubscribe={projectSetActions.subscribeCollection}
        onDone={() => navigateToProjectSelector({ replace: true })}
      />
    );
  }

  // Show project set setup/migration screen if needed. Ephemeral
  // preview boots skip it: the effect above establishes the root
  // silently while the share handler connects.
  if (
    !ephemeralHub &&
    (projectSetState.status === 'needs-setup' ||
      projectSetState.status === 'needs-migration')
  ) {
    return (
      <ProjectSetSetup
        hasMigration={projectSetState.status === 'needs-migration'}
        legacyProjects={projectSetState.legacyProjects}
        error={projectSetState.error}
        isConnecting={false}
        onCreateProjectSet={projectSetActions.createProjectSet}
        onLinkProjectSet={projectSetActions.linkProjectSet}
        onMigrateProjects={projectSetActions.migrateProjects}
        onMergeIntoProjectSet={projectSetActions.mergeIntoProjectSet}
      />
    );
  }

  // Show error if project set connection failed. Ephemeral preview
  // boots skip it too: the preview works without a project set, so a
  // set failure must not block it.
  if (!ephemeralHub && projectSetState.status === 'error') {
    return (
      <ProjectSetSetup
        hasMigration={false}
        legacyProjects={[]}
        error={projectSetState.error}
        isConnecting={false}
        onCreateProjectSet={projectSetActions.createProjectSet}
        onLinkProjectSet={projectSetActions.linkProjectSet}
        onMigrateProjects={projectSetActions.migrateProjects}
        onMergeIntoProjectSet={projectSetActions.mergeIntoProjectSet}
      />
    );
  }

  return (
    <>
      {!project ? (
        uiVariant === 'collections' ? (
          <ProjectsHome
            onSelectProject={handleSelectProject}
            onProjectCreated={handleProjectCreated}
            isConnecting={isConnecting}
            error={connectionError}
            onSignOut={AUTH_ENABLED ? logout : undefined}
            authEmail={auth?.email}
            authPicture={auth?.picture}
            onScreenNameChange={setScreenName}
            onColorChange={setCursorColor}
            projectSetDocId={projectSetActions.getProjectSetDocId()}
            projectSetSyncServer={projectSetActions.getSyncServer()}
            projectSetStatus={projectSetState.status}
            projectSetEntries={projectSetState.status === 'connected' ? projectSetState.projects : undefined}
            onRemoveProjectFromSet={projectSetActions.removeProject}
            onTouchProject={projectSetActions.touchProject}
            onAddProjectToSet={projectSetActions.addProject}
            onRenameProject={projectSetActions.updateProjectDescription}
            onUpdateProjectSummary={projectSetActions.updateProjectSummary}
            collections={projectSetState.collections}
            onCreateCollection={projectSetActions.createCollection}
            onUnsubscribeCollection={projectSetActions.unsubscribeCollection}
            onRenameCollection={projectSetActions.renameCollection}
            onAddProjectToCollection={projectSetActions.addProjectToCollection}
            onRemoveProjectFromCollection={projectSetActions.removeProjectFromCollection}
            onMoveProjectBetweenCollections={projectSetActions.moveProjectBetweenCollections}
            onSwitchToClassicUi={() => switchUiVariant('classic')}
          />
        ) : (
          <>
            <ProjectSelector
              onSelectProject={handleSelectProject}
              onProjectCreated={handleProjectCreated}
              isConnecting={isConnecting}
              error={connectionError}
              onSignOut={AUTH_ENABLED ? logout : undefined}
              authEmail={auth?.email}
              authPicture={auth?.picture}
              onScreenNameChange={setScreenName}
              onColorChange={setCursorColor}
              authName={auth?.name}
              projectSetDocId={projectSetActions.getProjectSetDocId()}
              projectSetSyncServer={projectSetActions.getSyncServer()}
              projectSetStatus={projectSetState.status}
              projectSetEntries={projectSetState.status === 'connected' ? projectSetState.projects : undefined}
              onRemoveProjectFromSet={projectSetActions.removeProject}
              onTouchProject={projectSetActions.touchProject}
              onAddProjectToSet={projectSetActions.addProject}
            />
            <button
              className="ui-variant-toggle"
              onClick={() => switchUiVariant('collections')}
              title="Collections-based projects home (UI exploration)"
            >
              Try the new projects home
            </button>
          </>
        )
      ) : (
        <ViewModeProvider>
          <ErrorBoundary>
            <Editor
              project={project}
              files={files}
              fileContents={fileContents}
              onDisconnect={handleDisconnect}
              onContentOperations={handleContentOperations}
              route={route}
              onNavigateToFile={(filePath, options) => {
                navigateToFile(project.id, filePath, options);
              }}
              identities={identities}
              captures={captures}
              executorsOnline={liveExecutors.length > 0}
              onRequestExecution={requestExecution}
              isOnline={isOnline}
              sessionEphemeral={previewSession?.allowEdit === false}
            />
          </ErrorBoundary>
        </ViewModeProvider>
      )}
      <Toast
        message="Auto-saved"
        visible={showSaveToast}
        onHide={() => setShowSaveToast(false)}
      />
    </>
  );
}

export default App;
