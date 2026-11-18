/**
 * ProjectProvider — one open hub project, shared by the parts in
 * `./parts`. It holds what the parts need from each other: the synced
 * project (`useProjectConnection`), which file is open, the open file's
 * current text, who else is on it, and the mounted Monaco editor.
 *
 * PROTOTYPE. A host arranges the parts however it likes inside this
 * provider; a part it does not render costs nothing, and a part it wants
 * to replace is replaced by any component that reads `useProject()`.
 */

import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '@quarto/preview-runtime';
import type { PresenceState } from '../services/presenceService';
import type { DocumentEditorHandle } from '../components/DocumentEditor';
import {
  useProjectConnection,
  type ProjectConnection,
  type ProjectConnectionOptions,
} from './useProjectConnection';

export interface ProjectContextValue extends ProjectConnection {
  /** The open file; null only while the project is empty or loading. */
  currentFile: FileEntry | null;
  openFile: (path: string) => void;

  /** The open file's text as the editor has it (edits, remote changes, file switches). */
  content: string;
  /** People with a cursor in the open file. */
  remoteUsers: PresenceState[];
  /** The mounted Monaco editor, or null. Changes on every file switch. */
  editor: Monaco.editor.IStandaloneCodeEditor | null;

  /** For parts that plug in to the above; a host does not normally need these. */
  internals: {
    setContent: (content: string) => void;
    setRemoteUsers: (users: PresenceState[]) => void;
    setEditor: (editor: Monaco.editor.IStandaloneCodeEditor | null) => void;
    editorRef: RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
    editorHandleRef: RefObject<DocumentEditorHandle | null>;
  };
}

const ProjectContext = createContext<ProjectContextValue | null>(null);

export interface ProjectProviderProps extends ProjectConnectionOptions {
  /** The file to open first. Default: index.qmd, else the first .qmd, else the first file. */
  initialFile?: string;
  children: ReactNode;
}

function defaultFile(files: FileEntry[]): FileEntry | null {
  return (
    files.find((file) => file.path === 'index.qmd') ??
    files.find((file) => file.path.endsWith('.qmd')) ??
    files[0] ??
    null
  );
}

export function ProjectProvider({ initialFile, children, ...connectionOptions }: ProjectProviderProps) {
  const connection = useProjectConnection(connectionOptions);
  const [filePath, setFilePath] = useState<string | null>(initialFile ?? null);
  const [content, setContent] = useState('');
  const [remoteUsers, setRemoteUsers] = useState<PresenceState[]>([]);
  const [editor, setEditorState] = useState<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const editorHandleRef = useRef<DocumentEditorHandle | null>(null);

  const setEditor = useCallback((next: Monaco.editor.IStandaloneCodeEditor | null) => {
    editorRef.current = next;
    setEditorState(next);
  }, []);

  // A path that is not (or no longer, or not yet) in the project falls
  // back to the default: the open file was deleted, or was just created
  // and has not arrived in `files`.
  const currentFile =
    (filePath === null ? undefined : connection.files.find((file) => file.path === filePath)) ??
    defaultFile(connection.files);

  const internals = useMemo(
    () => ({ setContent, setRemoteUsers, setEditor, editorRef, editorHandleRef }),
    [setEditor],
  );

  const value: ProjectContextValue = {
    ...connection,
    currentFile,
    openFile: setFilePath,
    content,
    remoteUsers,
    editor,
    internals,
  };

  return <ProjectContext.Provider value={value}>{children}</ProjectContext.Provider>;
}

export function useProject(): ProjectContextValue {
  const context = useContext(ProjectContext);
  if (context === null) throw new Error('useProject must be used within a ProjectProvider');
  return context;
}
