/**
 * Collaborative cursors for the editor wired into its "editor" port: the
 * hub-client presence service (`usePresence`), which broadcasts the local
 * cursor — labelled with the identity wired into "identity" — as Automerge
 * ephemeral messages on the file's doc handle in the hub project wired
 * into "project" (so they travel that repo's websocket to the server and
 * on to every peer on the doc), and draws the other users' cursors and
 * selections into the editor. Lists who else is on the file.
 */

import { useEffect } from 'react';
import { Handle, Position } from '@xyflow/react';
import { usePresence } from '../../../hooks/usePresence';
import { useInput, type EditorPort, type IdentityPort, type ProjectPort } from './ports';

export default function PresenceNode() {
  const port = useInput<EditorPort>('editor');
  const identity = useInput<IdentityPort>('identity');
  const project = useInput<ProjectPort>('project');
  const enabled = port !== null && identity !== null && project !== null;
  const { remoteUsers, onEditorMount, refreshIdentity } = usePresence(port?.path ?? null, { enabled });
  // The identity node edits the stored settings; pick them up when it does.
  useEffect(() => {
    if (enabled) void refreshIdentity();
  }, [identity, enabled, refreshIdentity]);
  // Attach to each Monaco instance the editor node publishes.
  useEffect(() => {
    if (enabled) onEditorMount(port.editor);
  }, [port, enabled, onEditorMount]);

  return (
    <div className="cd-node cd-presence">
      <div className="cd-title">Presence</div>
      {port === null
        ? <div className="cd-empty">No editor connected</div>
        : identity === null
          ? <div className="cd-empty">No identity connected to broadcast as</div>
        : project === null
          ? <div className="cd-empty">No hub project connected to carry the ephemeral messages</div>
        : remoteUsers.length === 0
          ? <div className="cd-empty">nobody else on {port.path}</div>
          : (
            <ul className="cd-users">
              {remoteUsers.map((user) => (
                <li key={user.peerId}>
                  <span className="cd-user-dot" style={{ background: user.userColor }} />
                  {user.userName}
                </li>
              ))}
            </ul>
          )}
      <Handle type="target" position={Position.Left} id="editor" title="editor" style={{ top: '35%' }} />
      <Handle type="target" position={Position.Left} id="project" title="project" style={{ top: '65%' }} />
      <Handle type="target" position={Position.Top} id="identity" title="identity" />
    </div>
  );
}
