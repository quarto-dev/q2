import type { ConnectionInfo } from '../types/messages'

interface Props {
  /** The user-editable sync server URL. Drives the input field. */
  url: string
  /** Current connection state (independent of what the user is editing). */
  connection: ConnectionInfo
  onUrlChange: (url: string) => void
  onReconnect: () => void
}

export function ConnectionStatus({
  url,
  connection,
  onUrlChange,
  onReconnect,
}: Props) {
  const statusClass =
    connection.state === 'connected'
      ? 'connected'
      : connection.state === 'connecting'
        ? 'connecting'
        : 'disconnected'

  return (
    <div className="connection-status">
      <div className="connection-indicator">
        <span className={`status-dot ${statusClass}`} />
        <span className="status-text">
          {connection.state === 'connected'
            ? 'Connected'
            : connection.state === 'connecting'
              ? 'Connecting…'
              : 'Disconnected'}
        </span>
      </div>

      <div className="connection-details">
        <label>
          Sync Server:
          <input
            type="text"
            value={url}
            onChange={(e) => onUrlChange(e.target.value)}
            placeholder="wss://host/ws"
          />
        </label>
        {/*
          Always enabled: the underlying WebSocket adapter never fires a
          failure signal on its own (peer-disconnected requires a prior
          peer-candidate), so an initial attempt that never succeeds
          otherwise leaves the user with no way to give up and try a
          different URL. Each click tears down the current repo and
          rebuilds it at the current URL.
        */}
        <button onClick={onReconnect}>
          {connection.state === 'connected' ? 'Reconnect' : 'Connect'}
        </button>
      </div>

      {connection.peerId && (
        <div className="peer-info">
          <span>
            Our ID: <code>{connection.peerId.slice(0, 12)}…</code>
          </span>
          {connection.remotePeerId && (
            <span>
              Server ID: <code>{connection.remotePeerId.slice(0, 12)}…</code>
            </span>
          )}
        </div>
      )}
    </div>
  )
}
