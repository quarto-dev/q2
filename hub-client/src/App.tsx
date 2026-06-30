import { useState, useCallback, useEffect, useRef, lazy, Suspense } from 'react';
import type { ProjectEntry, FileEntry } from '@quarto/preview-renderer/types/project';
import ProjectSelector from './components/ProjectSelector';
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
import { getUserIdentity, updateUserName } from './services/userSettings';
import { useRouting } from './hooks/useRouting';
import { useProjectSet } from './hooks/useProjectSet';
import { useAuth } from './hooks/useAuth';
import { useAuthProbe } from './hooks/useAuthProbe';
import { useExecutionChannel } from './hooks/useExecutionChannel';
import { resolveActorId as resolveActorIdRequest } from './services/authService';
import type { Route, ShareRoute, LinkProjectSetRoute } from './utils/routing';
import './App.css';

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
  // Production opens offline-first (the sync client's 1 ms default peer wait):
  // a returning user sees cached content instantly while the peer connects in
  // the background. But the smoke-all E2E env always starts with EMPTY storage
  // and must sync every doc from the (local) server, so opening offline-first
  // there means loadFileDocuments races the still-connecting websocket — and
  // under CI contention the render-target doc loses that race, is marked
  // unavailable, and the preview fails "Path not found" (stage
  // EDITOR_NO_PREVIEW; sometimes the index loses it too → CONNECT_STALL).
  // waitForPeer resolves the instant the peer connects, so this only adds wall
  // time when the connection is genuinely slow — exactly the CI case we want to
  // wait out. Tree-shaken in production, so no offline-first UX change there.
  const peerTimeoutMs = import.meta.env.VITE_E2E === '1' ? 15000 : undefined;
  const files = await connect(syncServer, indexDocId, actorId, screenName, color, peerTimeoutMs);
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
    triggerRefresh,
    sessionExpired,
    expireSession,
  } = useAuth();

  const [project, setProject] = useState<ProjectEntry | null>(null);
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [isConnecting, setIsConnecting] = useState(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [fileContents, setFileContents] = useState<Map<string, string>>(new Map());
  const [showSaveToast, setShowSaveToast] = useState(false);
  const [screenName, setScreenName] = useState<string | undefined>();
  const [cursorColor, setCursorColor] = useState<string | undefined>();
  const [identities, setIdentities] = useState<Record<string, ActorIdentity>>({});
  // bd-sfet3264 (Phase 1C): IndexDocument V2 capture sidecar (path → CaptureRef).
  // Populated by the sync client's onCapturesChange; threaded down to the
  // preview so recorded engine output can be spliced into the rendered AST.
  const [captures, setCaptures] = useState<Record<string, CaptureRef>>({});
  const [isOnline, setIsOnline] = useState<boolean>(false);

  // bd-sfet3264 (Phase 2D): track which q2 executors are online for the
  // connected project (via the index-handle capability beacon). No executor
  // produces beacons until Phase 4, so this is [] in practice today; the
  // wiring + a read-only indicator are in place for when it lands.
  const liveExecutors = useExecutionChannel(isOnline, project?.indexDocId ?? null);

  // While a project's sync is disconnected, check whether the disconnect is
  // actually an auth rejection (browsers hide the WS upgrade status). Only
  // definitive 401/403 evidence ever clears auth — never network errors.
  // Past the token's exp, useAuth's expiry timer logs out on the first 401
  // (preempting this probe's two-strike); the probe governs earlier drops.
  useAuthProbe({
    enabled: AUTH_ENABLED && !!auth && !!project && !isOnline,
    triggerRefresh,
    onAuthRejected: expireSession,
  });

  // Project set management (synced project list)
  const [projectSetState, projectSetActions] = useProjectSet();

  // Keep a ref so callbacks that intentionally omit projectSetState from their
  // dependency arrays (to avoid re-creation churn) can still read the latest status.
  const projectSetStateRef = useRef(projectSetState);
  projectSetStateRef.current = projectSetState;

  // Resolve the per-project actor ID before opening a document. See
  // `resolveActorIdRequest` for the three-valued contract; callers abandon
  // the open only on `null` (auth failure), proceed on `string`/`undefined`.
  const resolveActorId = useCallback(
    (indexDocId: string) => resolveActorIdRequest(indexDocId, AUTH_ENABLED, triggerRefresh),
    [triggerRefresh],
  );

  // Capture auth error from redirect query param (once, before URL is cleaned).
  const [authError] = useState(() => {
    const has = new URLSearchParams(window.location.search).has('auth_error');
    if (has) window.history.replaceState(null, '', window.location.pathname + window.location.hash);
    return has;
  });

  // Load screen name from IndexedDB (for identity mapping in Automerge docs).
  // When auth is enabled, wait for it to resolve so we can upgrade anonymous
  // names to the OIDC display name on first login. Without auth, load immediately.
  useEffect(() => {
    if (AUTH_ENABLED && authLoading) return;
    getUserIdentity().then(async (settings) => {
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

  // URL-based routing
  const {
    route,
    navigateToProjectSelector,
    navigateToProject,
    navigateToFile,
  } = useRouting();

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

        // Normalize the indexDocId (add 'automerge:' prefix if not present)
        const normalizedIndexDocId = shareRoute.indexDocId.startsWith('automerge:')
          ? shareRoute.indexDocId
          : `automerge:${shareRoute.indexDocId}`;

        // Check if we already have this project locally, or auto-create it
        let targetProject = await projectStorage.getProjectByIndexDocId(normalizedIndexDocId);
        if (!targetProject) {
          targetProject = await projectStorage.addProject(
            normalizedIndexDocId,
            shareRoute.syncServer,
            shareRoute.name
          );
        }

        // Also add to the synced project set (if connected)
        if (projectSetStateRef.current.status === 'connected') {
          try {
            projectSetActions.addProject({
              indexDocId: normalizedIndexDocId,
              syncServer: shareRoute.syncServer,
              description: shareRoute.name,
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

          navigateToFile(targetProject.id, shareRoute.filePath, { replace: true });
        } catch (err) {
          setConnectionError(err instanceof Error ? err.message : String(err));
        } finally {
          setIsConnecting(false);
        }
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
      const result = await createNewProject({
        syncServer,
        files,
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
        error={authError}
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

  // Show project set setup/migration screen if needed
  if (
    projectSetState.status === 'needs-setup' ||
    projectSetState.status === 'needs-migration'
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

  // Show error if project set connection failed
  if (projectSetState.status === 'error') {
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
              isOnline={isOnline}
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
