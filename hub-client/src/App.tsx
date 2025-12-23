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
} from './services/automergeSync';
import './App.css';

function App() {
  const [project, setProject] = useState<ProjectEntry | null>(null);
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [isConnecting, setIsConnecting] = useState(false);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [fileContents, setFileContents] = useState<Map<string, string>>(new Map());

  // Set up sync handlers
  useEffect(() => {
    setSyncHandlers({
      onFilesChange: (newFiles) => {
        setFiles(newFiles);
      },
      onFileContent: (path, content) => {
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

  if (!project) {
    return (
      <ProjectSelector
        onSelectProject={handleSelectProject}
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
      onDisconnect={handleDisconnect}
      onContentChange={handleContentChange}
    />
  );
}

export default App;
