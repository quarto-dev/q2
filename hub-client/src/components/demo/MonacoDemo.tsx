/**
 * #/demo/monaco — the hub-client document editor (`DocumentEditor`, the
 * Monaco editor `Editor.tsx` uses) on a file of a hub project, by itself:
 * Automerge sync, remote cursors, the qmd grammar and theme, outline /
 * folding / semantic tokens, Cmd+B, link and image paste. The preview
 * runtime's `connect` loads the project and keeps it synced.
 */

import { useState } from 'react';
import { applyEditorOperations } from '@quarto/preview-runtime';
import type { PresenceState } from '../../services/presenceService';
import DocumentEditor from '../DocumentEditor';
import DemoFrame from './DemoFrame';
import { FILE_PATH, useHubProject } from './hubProject';

export default function MonacoDemo() {
  const { fileContents, currentFile, error } = useHubProject(FILE_PATH);
  const [remoteUsers, setRemoteUsers] = useState<PresenceState[]>([]);

  const info = error ?? (remoteUsers.length === 0
    ? FILE_PATH
    : `${FILE_PATH} · with ${remoteUsers.map((user) => user.userName).join(', ')}`);

  return (
    <DemoFrame info={info}>
      {currentFile !== null && (
        <DocumentEditor
          file={currentFile}
          fileContents={fileContents}
          onContentOperations={applyEditorOperations}
          onPresenceChange={setRemoteUsers}
        />
      )}
    </DemoFrame>
  );
}
