/**
 * useProjectConnection — open one hub project and keep its React state
 * current: the wiring between the preview runtime's sync client and the
 * props `Editor` takes.
 *
 * PROTOTYPE. `App.tsx` does this inline (`setSyncHandlers` in an effect,
 * `connectAndLoadContents`, a `useState` per field) and the Connect
 * satellite's shell copies it. This is the same wiring as one hook; `App`
 * is not converted yet.
 */

import { useEffect, useState } from 'react';
import {
  connect,
  getFileContent,
  setSyncHandlers,
  type ActorIdentity,
  type CaptureRef,
  type ConnectOptions,
  type FileEntry,
} from '@quarto/preview-runtime';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';
import { resolveSyncServerUrl } from '../utils/routing';
import { authorIdFromUserId, updateUserColor, updateUserName } from '../services/userSettings';
import { generateColorFromId } from '../services/storage/utils';

/** Who the local user is, as the host knows them. */
export interface HostIdentity {
  /** The host's stable id for the user; edits are attributed to it. */
  userId: string;
  /** Display name for presence (cut to hub-client's 50 characters). */
  name: string;
  /** Cursor color as hex. Default: derived from `userId`. */
  color?: string;
}

export interface ProjectConnectionOptions {
  /** Sync server: a ws(s):// URL or a root-relative path such as `/ws`. */
  syncServer: string;
  indexDocId: string;
  /** The project's display name. */
  name: string;
  /** Omit to use the name and color hub-client has stored for this browser. */
  identity?: HostIdentity;
  /** Forwarded to the sync client, e.g. `{ peerTimeoutMs: 15000 }`. Read once per connection. */
  connectOptions?: ConnectOptions;
}

export interface ProjectConnection {
  /** Null until the project has loaded. */
  project: ProjectEntry | null;
  files: FileEntry[];
  folders: string[];
  fileContents: Map<string, string>;
  binaryFileVersions: Map<string, number>;
  identities: Record<string, ActorIdentity>;
  captures: Record<string, CaptureRef>;
  isOnline: boolean;
  /** Why the project could not be opened. A sync error after that is only logged. */
  error: string | null;
}

export function useProjectConnection({
  syncServer,
  indexDocId,
  name,
  identity,
  connectOptions,
}: ProjectConnectionOptions): ProjectConnection {
  const [project, setProject] = useState<ProjectEntry | null>(null);
  const [files, setFiles] = useState<FileEntry[]>([]);
  const [folders, setFolders] = useState<string[]>([]);
  const [fileContents, setFileContents] = useState<Map<string, string>>(new Map());
  const [binaryFileVersions, setBinaryFileVersions] = useState<Map<string, number>>(new Map());
  const [identities, setIdentities] = useState<Record<string, ActorIdentity>>({});
  const [captures, setCaptures] = useState<Record<string, CaptureRef>>({});
  const [isOnline, setIsOnline] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const userId = identity?.userId;
  const userName = identity?.name;
  const userColor = identity?.color;

  useEffect(() => {
    let cancelled = false;

    const open = async () => {
      // The runtime has one set of sync handlers. Taking it a tick after
      // mount puts ours after any a parent registers in its own effect
      // (`App` does, around the demo page), and lets a dev double-mount
      // cancel the first run before it connects.
      await Promise.resolve();
      if (cancelled) return;
      setSyncHandlers({
        onFilesChange: setFiles,
        onFoldersChange: setFolders,
        onIdentitiesChange: setIdentities,
        onCapturesChange: setCaptures,
        onFileContent: (path, content) => {
          setFileContents((prev) => new Map(prev).set(path, content));
        },
        onBinaryContent: (path) => {
          setBinaryFileVersions((prev) => new Map(prev).set(path, (prev.get(path) ?? 0) + 1));
        },
        onConnectionChange: setIsOnline,
        // Must not become `error`: that would unmount a live editor over
        // something the session can survive.
        onError: (err) => console.error('sync error:', err),
      });

      let authorId: string | undefined;
      let screenName: string | undefined;
      let color: string | undefined;
      if (userId !== undefined && userName !== undefined) {
        // Presence reads the name and color back out of the stored settings.
        await updateUserName(userName.slice(0, 50));
        const settings = await updateUserColor(userColor ?? generateColorFromId(userId));
        authorId = authorIdFromUserId(userId);
        screenName = settings.userName;
        color = settings.userColor;
      }

      const loaded = await connect(
        resolveSyncServerUrl(syncServer),
        indexDocId,
        authorId,
        screenName,
        color,
        connectOptions ?? {},
      );
      if (cancelled) return;

      const contents = new Map<string, string>();
      for (const file of loaded) {
        const content = getFileContent(file.path);
        if (content !== null) contents.set(file.path, content);
      }
      const now = new Date().toISOString();
      setFiles(loaded);
      setFileContents(contents);
      setProject({
        id: indexDocId,
        indexDocId,
        syncServer,
        description: name,
        createdAt: now,
        lastAccessed: now,
      });
    };

    open().catch((err: unknown) => {
      if (!cancelled) setError(err instanceof Error ? err.message : String(err));
    });

    // Left connected on unmount: the runtime's next `connect` disconnects
    // first, and a disconnect here would race a remount's connect.
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- connectOptions is read once per connection
  }, [syncServer, indexDocId, name, userId, userName, userColor]);

  return { project, files, folders, fileContents, binaryFileVersions, identities, captures, isOnline, error };
}
