/**
 * Who we are to others: the display name and colour presence broadcasts,
 * from the hub-client's user settings (IndexedDB). Editable here; offered
 * on the "identity" port.
 */

import { useEffect, useMemo, useState } from 'react';
import { Handle, Position } from '@xyflow/react';
import { getUserIdentity, updateUserColor, updateUserName } from '../../../services/userSettings';
import { useOutput, type IdentityPort } from './ports';
import TextBox from './TextBox';

export default function UserNode() {
  const [identity, setIdentity] = useState<IdentityPort | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void getUserIdentity()
      .then((settings) => setIdentity({ name: settings.userName, color: settings.userColor }))
      .catch((err: unknown) => setError(String(err)));
  }, []);

  const port = useMemo(() => identity ?? undefined, [identity]);
  useOutput('identity', port);

  const onName = (name: string) => {
    void updateUserName(name)
      .then((settings) => setIdentity({ name: settings.userName, color: settings.userColor }))
      .catch((err: unknown) => setError(String(err)));
  };
  const onColor = (color: string) => {
    void updateUserColor(color)
      .then((settings) => setIdentity({ name: settings.userName, color: settings.userColor }))
      .catch((err: unknown) => setError(String(err)));
  };

  return (
    <div className="cd-node cd-user">
      <div className="cd-title">User</div>
      {identity === null
        ? <div className="cd-meta">{error ?? 'loading…'}</div>
        : (
          <div className="cd-row">
            <input
              className="cd-color nodrag"
              type="color"
              value={identity.color}
              onChange={(e) => onColor(e.target.value)}
              title="colour"
            />
            <TextBox value={identity.name} placeholder="name" className="cd-name" onCommit={onName} />
          </div>
        )}
      {identity !== null && error !== null && <div className="cd-meta">{error}</div>}
      <Handle type="source" position={Position.Top} id="identity" title="identity" />
    </div>
  );
}
