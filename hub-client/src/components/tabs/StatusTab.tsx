/**
 * Status Tab Component
 *
 * Displays system status information:
 * - WASM renderer status
 * - Connected users (collaborators)
 */

import type { PresenceState } from '../../services/presenceService';
import './StatusTab.css';

type WasmStatus = 'loading' | 'ready' | 'error';

interface StatusTabProps {
  wasmStatus: WasmStatus;
  wasmError: string | null;
  userCount: number;
  remoteUsers: PresenceState[];
}

export default function StatusTab({
  wasmStatus,
  wasmError,
  userCount,
  remoteUsers,
}: StatusTabProps) {
  return (
    <div className="status-tab">
      <div className="status-tab-section">
        <label className="section-label">Renderer</label>
        <div className={`status-indicator ${wasmStatus}`}>
          <span className="status-dot" />
          <span className="status-text">
            {wasmStatus === 'loading' && 'Loading WASM...'}
            {wasmStatus === 'ready' && 'Ready'}
            {wasmStatus === 'error' && 'Error'}
          </span>
        </div>
        {wasmStatus === 'error' && wasmError && (
          <div className="status-error">{wasmError}</div>
        )}
      </div>

      <div className="status-tab-section">
        <label className="section-label">Collaborators</label>
        {userCount === 0 ? (
          <div className="no-users">No other users connected</div>
        ) : (
          <div className="user-list">
            <div className="user-count-summary">
              {userCount} other{userCount === 1 ? '' : 's'} here
            </div>
            <ul className="user-names">
              {remoteUsers.map((user) => (
                <li key={user.peerId}>
                  <span
                    className="user-color-dot"
                    style={{ backgroundColor: user.userColor }}
                  />
                  <span className="user-name">{user.userName}</span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}
