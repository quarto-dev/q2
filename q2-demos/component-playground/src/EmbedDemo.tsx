/**
 * embed.html — an editor built by a host out of hub-client's parts, with
 * this page playing the host (think: the Connect satellite).
 *
 * `ProjectProvider` opens the project; everything inside is this page's
 * own layout. Two things to notice:
 *
 *   - the header and the panel layout are the host's, built with
 *     hub-client's `SplitFrame` and layout buttons;
 *   - the parts find each other through the provider: the outline moves
 *     the editor, the preview follows it, the header lists who is here.
 */

import { useRef, useState } from 'react';
import {
  ProjectDocument,
  ProjectFileToolbar,
  ProjectFileTree,
  ProjectOutline,
  ProjectPreview,
  ProjectProvider,
  ProjectSyncStatus,
  SplitFrame,
  ThemeProvider,
  ViewToggleControl,
  usePreference,
  useProject,
  type CommentsMode,
  type SplitFrameHandle,
} from 'hub-client/src/embed';
import { FILE_PATH, INDEX_DOC_ID, SYNC_SERVER_URL } from './hubProject';
import './EmbedDemo.css';

// Stand-in for the page the host would send people to.
function hostFileUrl(path: string): string {
  const file = path.split('/').map(encodeURIComponent).join('/');
  return `https://connect.example/connect/#/sat/quartohub/open-content/file/${file}?content_guid=demo`;
}

/** The host's header: its own actions next to state read from the project. */
function Header({ split, onSplit, commentsMode, onCommentsMode, authors, onAuthors }: {
  split: number;
  onSplit: (fraction: number) => void;
  commentsMode: CommentsMode;
  onCommentsMode: (mode: CommentsMode) => void;
  authors: boolean;
  onAuthors: (authors: boolean) => void;
}) {
  const { project, currentFile, remoteUsers, error } = useProject();
  // Whether preview blocks can be edited is a persisted hub-client
  // preference the preview reads itself, so this picker writes it.
  const [previewEditing, setPreviewEditing] = usePreference('previewEditing');
  return (
    <header className="ed-header">
      <a href="index.html">← Demos</a>
      <strong>{project?.description ?? 'Opening…'}</strong>
      <span className="ed-path">{error ?? currentFile?.path ?? ''}</span>
      {remoteUsers.length > 0 && (
        <span className="ed-people">with {remoteUsers.map((user) => user.userName).join(', ')}</span>
      )}
      <span className="ed-spacer" />
      <select
        aria-label="Preview mode"
        value={previewEditing ? 'edit' : 'view'}
        onChange={(event) => setPreviewEditing(event.target.value === 'edit')}
      >
        <option value="edit">Edit</option>
        <option value="view">View</option>
      </select>
      <select
        aria-label="Comments"
        value={commentsMode}
        onChange={(event) => onCommentsMode(event.target.value as CommentsMode)}
      >
        <option value="expand">Comments: expanded</option>
        <option value="show">Comments: shown</option>
        <option value="hide">Comments: hidden</option>
      </select>
      <select
        aria-label="Authors"
        value={authors ? 'on' : 'off'}
        onChange={(event) => onAuthors(event.target.value === 'on')}
      >
        <option value="off">Authors: off</option>
        <option value="on">Authors: on</option>
      </select>
      {/* hub-client's layout buttons, driving the SplitFrame below. */}
      <ViewToggleControl fraction={split} onSelect={onSplit} />
    </header>
  );
}

// `ProjectDocument` reads the color scheme from hub-client's ThemeProvider,
// which hub-client's own entry point mounts; a host has to mount it too.
export default function EmbedDemo() {
  return (
    <ThemeProvider>
      <EmbedEditor />
    </ThemeProvider>
  );
}

function EmbedEditor() {
  // The split lives in SplitFrame; this mirrors it for the layout buttons.
  const splitFrame = useRef<SplitFrameHandle>(null);
  const [split, setSplit] = useState(0.5);
  // Session-only, as in the editor; handed to the preview below.
  const [commentsMode, setCommentsMode] = useState<CommentsMode>('show');
  const [authors, setAuthors] = useState(false);

  return (
    <ProjectProvider
      syncServer={SYNC_SERVER_URL}
      indexDocId={INDEX_DOC_ID}
      name="Embedded project"
      initialFile={FILE_PATH}
    // No `identity`: passing one stores its name and color as this
    // browser's hub-client user, which a demo should not do. A real host
    // passes `{ userId, name }` from its own session.
    >
      <div className="ed-page">
        <Header
          split={split}
          onSplit={(fraction) => splitFrame.current?.setFraction(fraction)}
          commentsMode={commentsMode}
          onCommentsMode={setCommentsMode}
          authors={authors}
          onAuthors={setAuthors}
        />
        <div className="ed-body">
          <aside className="ed-side">
            <ProjectFileToolbar />
            <div className="ed-side-files">
              <ProjectFileTree
                // Links are the host's to define, so these two are opt-in.
                onCopyLink={(file) => void navigator.clipboard.writeText(hostFileUrl(file.path))}
                onOpenInNewTab={(file) => window.open(hostFileUrl(file.path), '_blank')}
              />
            </div>
            <div className="ed-side-outline">
              <ProjectOutline />
            </div>
          </aside>
          <SplitFrame
            ref={splitFrame}
            left={<ProjectDocument />}
            right={<ProjectPreview commentsMode={commentsMode} authors={authors} />}
            onFractionChange={setSplit}
            dividerLabel="Resize editor and preview panes"
          />
        </div>
        <footer className="ed-footer">
          <ProjectSyncStatus scope="project" />
          <ProjectSyncStatus scope="document" />
        </footer>
      </div>
    </ProjectProvider>
  );
}
