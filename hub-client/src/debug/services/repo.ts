import { Repo, type PeerId } from '@automerge/automerge-repo'
import { WebSocketClientAdapter } from '@automerge/automerge-repo-network-websocket'
import { IndexedDBStorageAdapter } from '@automerge/automerge-repo-storage-indexeddb'
import { LoggingNetworkAdapter } from './LoggingNetworkAdapter'
import { ReadOnlyStorageAdapter } from './ReadOnlyStorageAdapter'
import type { MessageLogEntry } from '../types/messages'

export interface RepoConfig {
  syncServerUrl: string
  onMessage: (entry: MessageLogEntry) => void
  onConnectionChange: (connected: boolean, remotePeerId?: PeerId) => void
}

export interface InspectorRepo {
  repo: Repo
  adapter: LoggingNetworkAdapter
  shutdown: () => void
}

/**
 * Creates an ephemeral Automerge repository that logs all network traffic.
 *
 * No storage adapter is attached (`isEphemeral: true`), so the repo holds
 * nothing between page loads and does not pollute the user's real IndexedDB.
 * All sync messages are observed via the `LoggingNetworkAdapter` wrapper
 * around a plain `WebSocketClientAdapter` pointed at `syncServerUrl`.
 *
 * On same-origin hub servers, the browser automatically attaches the HttpOnly
 * auth cookie to the WebSocket upgrade, so the authenticated sync server
 * will accept the connection.
 */
export function createInspectorRepo(config: RepoConfig): InspectorRepo {
  const wsAdapter = new WebSocketClientAdapter(config.syncServerUrl)
  const loggingAdapter = new LoggingNetworkAdapter(
    wsAdapter,
    config.onMessage,
    config.onConnectionChange,
  )
  const repo = new Repo({
    network: [loggingAdapter],
    isEphemeral: true,
  })
  return {
    repo,
    adapter: loggingAdapter,
    shutdown: () => {
      loggingAdapter.disconnect()
    },
  }
}

/**
 * Build the default sync-server WebSocket URL. Uses same-origin `/ws`, which
 * is proxied to the hub server (in dev) or served by it directly (in prod),
 * so the HttpOnly auth cookie flows automatically on the WebSocket upgrade.
 */
export function defaultSyncServerUrl(): string {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${proto}//${window.location.host}/ws`
}

export interface LocalStorageRepo {
  repo: Repo
  shutdown: () => void
}

/**
 * Create a Repo that reads the main app's persistent Automerge state
 * (IndexedDB database "automerge", store "documents") in *read-only* mode
 * with no network attached.
 *
 * The underlying `IndexedDBStorageAdapter` does perform writes when given a
 * chance, so we wrap it in `ReadOnlyStorageAdapter` — `save`, `remove`,
 * and `removeRange` become no-ops. Combined with the absence of a network
 * adapter, this Repo observes on-disk state without ever mutating it or
 * perturbing the main app's sync. That guarantee is important: two Repos
 * writing to the same IndexedDB concurrently is exactly the kind of
 * scenario that could itself be a source of the bugs we're investigating.
 */
export function createLocalStorageRepo(): LocalStorageRepo {
  const storage = new ReadOnlyStorageAdapter(new IndexedDBStorageAdapter())
  const repo = new Repo({ storage })
  return {
    repo,
    // Repo doesn't expose a hard shutdown for storage-only mode, but the
    // adapter has no ongoing work to cancel — dropping the reference is
    // sufficient for GC to clean up.
    shutdown: () => {},
  }
}
