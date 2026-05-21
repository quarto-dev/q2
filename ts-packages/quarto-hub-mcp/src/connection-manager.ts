/**
 * Connection Manager
 *
 * Manages sync client instances for multiple automerge projects.
 * Each project (identified by its index document ID) gets its own
 * sync client connection. Connections are created lazily on first access.
 */

import {
  createSyncClient,
  type SyncClient,
  type SyncClientCallbacks,
  type FilePayload,
  type Patch,
} from '@quarto/quarto-sync-client';

/**
 * Observed hub auth-mode (process-local). Phase 7 consults this to
 * decide whether `authenticate_start` should short-circuit. Phase 8
 * is responsible for flipping this in response to actual connect
 * outcomes (101 / 401); until then the value stays `'unknown'`.
 */
export type ObservedAuthMode = 'no-auth' | 'requires-auth' | 'unknown';

/**
 * Tracked state for a connected project.
 */
interface ProjectState {
  client: SyncClient;
  /** Current file contents, kept in sync via callbacks */
  files: Map<string, FilePayload>;
}

/**
 * Manages connections to multiple automerge projects.
 */
export class ConnectionManager {
  private readonly serverUrl: string;
  private readonly projects = new Map<string, ProjectState>();
  private observedAuthMode: ObservedAuthMode = 'unknown';

  constructor(serverUrl: string) {
    this.serverUrl = serverUrl;
  }

  /**
   * Most recent observation of the hub's auth requirement. Phase 8
   * will flip this based on actual WS-upgrade outcomes; Phase 7's
   * auth-tools call this to decide whether to short-circuit
   * `authenticate_start` against a no-auth hub.
   */
  lastObservedAuthMode(): ObservedAuthMode {
    return this.observedAuthMode;
  }

  /**
   * Get or create a connection to a project.
   * Returns the project state with the sync client and tracked file contents.
   */
  async connect(indexDocId: string): Promise<ProjectState> {
    const existing = this.projects.get(indexDocId);
    if (existing) {
      return existing;
    }

    const files = new Map<string, FilePayload>();

    const callbacks: SyncClientCallbacks = {
      onFileAdded(path: string, file: FilePayload) {
        files.set(path, file);
      },
      onFileChanged(path: string, text: string, _patches: Patch[]) {
        files.set(path, { type: 'text', text });
      },
      onBinaryChanged(path: string, data: Uint8Array, mimeType: string) {
        files.set(path, { type: 'binary', data, mimeType });
      },
      onFileRemoved(path: string) {
        files.delete(path);
      },
      onError(error: Error) {
        console.error(`[hub-mcp] Sync error for project ${indexDocId}:`, error.message);
      },
    };

    const client = createSyncClient(callbacks);
    await client.connect(this.serverUrl, indexDocId);

    const state: ProjectState = { client, files };
    this.projects.set(indexDocId, state);
    return state;
  }

  /**
   * Get an existing project connection, or null if not connected.
   */
  get(indexDocId: string): ProjectState | undefined {
    return this.projects.get(indexDocId);
  }

  /**
   * Get an existing project connection, throwing if not connected.
   */
  require(indexDocId: string): ProjectState {
    const state = this.projects.get(indexDocId);
    if (!state) {
      throw new Error(
        `Not connected to project ${indexDocId}. Call connect_project first.`
      );
    }
    return state;
  }

  /**
   * Create a new project on the sync server.
   */
  async createProject(
    files: Array<{ path: string; content: string }>
  ): Promise<{ indexDocId: string; files: Array<{ path: string; docId: string }> }> {
    // We need a temporary sync client to create the project
    const tempFiles = new Map<string, FilePayload>();
    const callbacks: SyncClientCallbacks = {
      onFileAdded(path: string, file: FilePayload) {
        tempFiles.set(path, file);
      },
      onFileChanged(path: string, text: string, _patches: Patch[]) {
        tempFiles.set(path, { type: 'text', text });
      },
      onBinaryChanged(path: string, data: Uint8Array, mimeType: string) {
        tempFiles.set(path, { type: 'binary', data, mimeType });
      },
      onFileRemoved(path: string) {
        tempFiles.delete(path);
      },
    };

    const client = createSyncClient(callbacks);
    const result = await client.createNewProject({
      syncServer: this.serverUrl,
      files: files.map(f => ({
        path: f.path,
        content: f.content,
        contentType: 'text' as const,
      })),
    });

    // Store the connected client for subsequent operations
    const state: ProjectState = { client, files: tempFiles };
    this.projects.set(result.indexDocId, state);

    return {
      indexDocId: result.indexDocId,
      files: result.files,
    };
  }

  /**
   * Disconnect from all projects.
   */
  async disconnectAll(): Promise<void> {
    const disconnects = Array.from(this.projects.values()).map(
      state => state.client.disconnect()
    );
    await Promise.all(disconnects);
    this.projects.clear();
  }
}
