import { useState, useCallback, useEffect, useRef } from 'react';
import type { ProjectEntry, FileEntry } from './types/project';
import ProjectSelector from './components/ProjectSelector';
import Editor from './components/Editor';
import Toast from './components/Toast';
import {
  connect,
  disconnect,
  setSyncHandlers,
  getFileContent,
  updateFileContent,
  createNewProject,
} from './services/automergeSync';
import type { ProjectFile } from './services/wasmRenderer';
import * as projectStorage from './services/projectStorage';
import { useRouting } from './hooks/useRouting';
import type { Route, ShareRoute } from './utils/routing';
import './App.css';

/**
 * Data extracted from a shareable link, used to pre-fill the connect dialog.
 */
export interface PendingShareData {
  /** The Automerge index document ID (without 'automerge:' prefix) */
  indexDocId: string;
  /** The sync server URL */
  syncServer: string;
  /** Optional file path to open after connecting */
  filePath?: string;
}

function App() {
  const [project, setProject] = useState<ProjectEntry | null>(null);
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [isConnecting, setIsConnecting] = useState(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [fileContents, setFileContents] = useState<Map<string, string>>(new Map());
  const [showSaveToast, setShowSaveToast] = useState(false);

  // Pending share link data (when user visits a shareable URL for a project they don't have)
  const [pendingShareData, setPendingShareData] = useState<PendingShareData | null>(null);

  // Track if we've done the initial URL-based navigation
  const initialLoadRef = useRef(false);

  // URL-based routing
  const {
    route,
    navigateToProjectSelector,
    navigateToProject,
    navigateToFile,
  } = useRouting();

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
            // Connect to the project
            setIsConnecting(true);
            setConnectionError(null);
            try {
              const loadedFiles = await connect(targetProject.syncServer, targetProject.indexDocId);
              setProject(targetProject);
              setFiles(loadedFiles);

              const contents = new Map<string, string>();
              for (const file of loadedFiles) {
                const content = getFileContent(file.path);
                if (content !== null) {
                  contents.set(file.path, content);
                }
              }
              setFileContents(contents);
            } catch (err) {
              setConnectionError(err instanceof Error ? err.message : String(err));
              // Navigate back to project selector on error
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
      // Handle shareable link URLs
      if (route.type === 'share') {
        // SECURITY: Immediately clear the URL to prevent indexDocId from appearing
        // in browser history, bookmarks, or being accidentally shared.
        navigateToProjectSelector({ replace: true });

        const shareRoute = route as ShareRoute;
        // Normalize the indexDocId (add 'automerge:' prefix if not present)
        const normalizedIndexDocId = shareRoute.indexDocId.startsWith('automerge:')
          ? shareRoute.indexDocId
          : `automerge:${shareRoute.indexDocId}`;

        // Check if we already have this project locally
        const existingProject = await projectStorage.getProjectByIndexDocId(normalizedIndexDocId);

        if (existingProject) {
          // Project exists locally - connect to it
          setIsConnecting(true);
          setConnectionError(null);
          try {
            const loadedFiles = await connect(existingProject.syncServer, existingProject.indexDocId);
            setProject(existingProject);
            setFiles(loadedFiles);

            const contents = new Map<string, string>();
            for (const file of loadedFiles) {
              const content = getFileContent(file.path);
              if (content !== null) {
                contents.set(file.path, content);
              }
            }
            setFileContents(contents);

            // Navigate to the project (and optionally file) using local ID
            if (shareRoute.filePath) {
              navigateToFile(existingProject.id, shareRoute.filePath, { replace: true });
            } else {
              navigateToProject(existingProject.id, { replace: true });
            }
          } catch (err) {
            setConnectionError(err instanceof Error ? err.message : String(err));
          } finally {
            setIsConnecting(false);
          }
        } else {
          // Project doesn't exist locally - show connect dialog with pre-filled data
          setPendingShareData({
            indexDocId: shareRoute.indexDocId,
            syncServer: shareRoute.syncServer,
            filePath: shareRoute.filePath,
          });
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
            const loadedFiles = await connect(targetProject.syncServer, targetProject.indexDocId);
            setProject(targetProject);
            setFiles(loadedFiles);

            const contents = new Map<string, string>();
            for (const file of loadedFiles) {
              const content = getFileContent(file.path);
              if (content !== null) {
                contents.set(file.path, content);
              }
            }
            setFileContents(contents);
          } catch (err) {
            setConnectionError(err instanceof Error ? err.message : String(err));
            // Navigate to project selector on error
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
      onFileContent: (path, content, _patches) => {
        // Note: patches are ignored - we use diff-based sync in Editor.tsx
        setFileContents((prev) => {
          const next = new Map(prev);
          next.set(path, content);
          return next;
        });
      },
      onConnectionChange: (connected) => {
        if (!connected && project) {
          // Connection lost
          setConnectionError('Connection lost');
        }
      },
      onError: (error) => {
        setConnectionError(error.message);
      },
    });
  }, [project]);

  const handleSelectProject = useCallback(async (selectedProject: ProjectEntry, filePathOverride?: string) => {
    // Clear any pending share data
    setPendingShareData(null);

    setIsConnecting(true);
    setConnectionError(null);

    try {
      const loadedFiles = await connect(selectedProject.syncServer, selectedProject.indexDocId);
      setProject(selectedProject);
      setFiles(loadedFiles);

      // Initialize file contents from automerge
      const contents = new Map<string, string>();
      for (const file of loadedFiles) {
        const content = getFileContent(file.path);
        if (content !== null) {
          contents.set(file.path, content);
        }
      }
      setFileContents(contents);

      // Update URL to reflect the selected project (and optionally a specific file)
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
  }, [navigateToProject, navigateToFile]);

  const handleDisconnect = useCallback(async () => {
    await disconnect();
    setProject(null);
    setFiles([]);
    setFileContents(new Map());
    setConnectionError(null);
    // Update URL to show project selector
    navigateToProjectSelector({ replace: true });
  }, [navigateToProjectSelector]);

  const handleContentChange = useCallback((path: string, content: string) => {
    updateFileContent(path, content);
    setFileContents((prev) => {
      const next = new Map(prev);
      next.set(path, content);
      return next;
    });
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

      // Create the Automerge documents
      const result = await createNewProject({
        syncServer,
        files,
      });

      // Store the project in IndexedDB
      const projectEntry = await projectStorage.addProject(
        result.indexDocId,
        syncServer,
        title
      );

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
  }, [navigateToProject]);

  const handleClearPendingShare = useCallback(() => {
    setPendingShareData(null);
  }, []);

  return (
    <>
      {!project ? (
        <ProjectSelector
          onSelectProject={handleSelectProject}
          onProjectCreated={handleProjectCreated}
          isConnecting={isConnecting}
          error={connectionError}
          pendingShareData={pendingShareData}
          onClearPendingShare={handleClearPendingShare}
        />
      ) : (
        <Editor
          project={project}
          files={files}
          fileContents={fileContents}
          onDisconnect={handleDisconnect}
          onContentChange={handleContentChange}
          route={route}
          onNavigateToFile={(filePath, options) => {
            navigateToFile(project.id, filePath, options);
          }}
        />
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
