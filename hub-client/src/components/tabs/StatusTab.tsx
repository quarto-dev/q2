/**
 * Status Tab Component
 *
 * Displays system status information:
 * - WASM renderer status
 * - Connected users (collaborators)
 */

import type { PresenceState } from '../../services/presenceService';
import { common, tabs } from '../../strings';
import './StatusTab.css';

type WasmStatus = 'loading' | 'ready' | 'error';

interface StatusTabProps {
  wasmStatus: WasmStatus;
  wasmError: string | null;
  userCount: number;
  remoteUsers: PresenceState[];
  isOnline: boolean;
  /** Recovery action for a WASM boot failure; defaults to a page reload
   *  (the renderer initializes at Editor mount, so a reload is the honest
   *  in-place retry). */
  onRetry?: () => void;
}

export default function StatusTab({
  wasmStatus,
  wasmError,
  userCount,
  remoteUsers,
  onRetry,
}: StatusTabProps) {
  return (
    <div className="status-tab">
      <div className="status-tab-section">
        <label className="section-label">{tabs.status.rendererLabel}</label>
        <div className={`status-indicator ${wasmStatus}`}>
          <span className="status-dot" />
          <span className="status-text">
            {wasmStatus === 'loading' && tabs.status.loadingWasm}
            {wasmStatus === 'ready' && tabs.status.ready}
            {wasmStatus === 'error' && tabs.status.error}
          </span>
        </div>
        {wasmStatus === 'error' && wasmError && (
          <div className="status-error">
            {wasmError}
            <button
              type="button"
              className="qh-error-action"
              onClick={onRetry ?? (() => window.location.reload())}
            >
              {common.reload}
            </button>
          </div>
        )}
      </div>

      <div className="status-tab-section">
        <label className="section-label">{tabs.status.collaboratorsLabel}</label>
        {userCount === 0 ? (
          <div className="no-users">{tabs.status.noOthers}</div>
        ) : (
          <div className="user-list">
            <div className="user-count-summary">
              {tabs.status.othersHere(userCount)}
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
