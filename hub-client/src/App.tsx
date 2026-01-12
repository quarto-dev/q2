import { useState, useCallback, useEffect } from 'react';
import type { ProjectEntry, FileEntry } from './types/project';
import ProjectSelector from './components/ProjectSelector';
import Editor from './components/Editor';
import {
  connect,
  disconnect,
  setSyncHandlers,
  getFileContent,
  updateFileContent,
  createNewProject,
} from './services/automergeSync';
import type { Patch } from './services/automergeSync';
import type { ProjectFile } from './services/wasmRenderer';
import * as projectStorage from './services/projectStorage';
import './App.css';

function App() {
  const [project, setProject] = useState<ProjectEntry | null>(null);
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [isConnecting, setIsConnecting] = useState(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [fileContents, setFileContents] = useState<Map<string, string>>(new Map());
  const [filePatches, setFilePatches] = useState<Map<string, Patch[]>>(new Map());

  // Set up sync handlers
  useEffect(() => {
    setSyncHandlers({
      onFilesChange: (newFiles) => {
        setFiles(newFiles);
      },
      onFileContent: (path, content, patches) => {
        setFileContents((prev) => {
          const next = new Map(prev);
          next.set(path, content);
          return next;
        });
        setFilePatches((prev) => {
          const next = new Map(prev);
          next.set(path, patches);
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

  const handleSelectProject = useCallback(async (selectedProject: ProjectEntry) => {
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
    } catch (err) {
      setConnectionError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsConnecting(false);
    }
  }, []);

  const handleDisconnect = useCallback(async () => {
    await disconnect();
    setProject(null);
    setFiles([]);
    setFileContents(new Map());
    setFilePatches(new Map());
    setConnectionError(null);
  }, []);

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
    _projectType: string
  ) => {
    setIsConnecting(true);
    setConnectionError(null);

    try {
      // Default sync server (same as the connect form default)
      const syncServer = 'wss://sync.automerge.org';

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

      console.log('Project created successfully:', result.indexDocId);
    } catch (err) {
      setConnectionError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsConnecting(false);
    }
  }, []);

  if (!project) {
    return (
      <ProjectSelector
        onSelectProject={handleSelectProject}
        onProjectCreated={handleProjectCreated}
        isConnecting={isConnecting}
        error={connectionError}
      />
    );
  }

  return (
    <Editor
      project={project}
      files={files}
      fileContents={fileContents}
      filePatches={filePatches}
      onDisconnect={handleDisconnect}
      onContentChange={handleContentChange}
    />
  );
}

export default App;
