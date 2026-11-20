/**
 * embed3.html — the embedded editor laid out as a Posit Connect page: the
 * host's top bar and sidebar sections around a card that holds the
 * document and preview. Same parts as EmbedDemo; only the layout differs.
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
import { CommentsShowIcon, GearIcon, PanelLeftIcon, PeopleIcon, PrintIcon } from 'hub-client/src/components/icons';
import { FILE_PATH, INDEX_DOC_ID, SYNC_SERVER_URL } from './hubProject';
import './ConnectDemo.css';

function ConnectPage() {
  const { project, currentFile, error } = useProject();
  const splitFrame = useRef<SplitFrameHandle>(null);
  const [split, setSplit] = useState(0.5);
  const [sidebar, setSidebar] = useState(true);
  const [authors, setAuthors] = useState(false);
  const [commentsMode, setCommentsMode] = useState<CommentsMode>('show');
  const [previewEditing, setPreviewEditing] = usePreference('previewEditing');

  return (
    <div className="cn-page">
      <header className="cn-top">
        <a href="index.html">‹ CONTENT</a>
        <span className="cn-title">{project?.description ?? 'Opening…'}</span>
        {/* Stand-ins for the host's own actions. */}
        {Array.from({ length: 7 }, (_, i) => <button key={i}><span>●</span>Button</button>)}
        <button><GearIcon />Settings</button>
        <button><span>●</span>My Name</button>
      </header>
      <div className="cn-body">
        {sidebar === true && (
          <aside className="cn-side">
            <details open>
              <summary>Files <ProjectSyncStatus scope="project" /></summary>
              <ProjectFileToolbar />
              <ProjectFileTree />
            </details>
            <details open>
              <summary>Outlines</summary>
              <ProjectOutline />
            </details>
            <details>
              <summary>Settings</summary>
            </details>
          </aside>
        )}
        <main className="cn-card">
          <div className="cn-bar">
            <button aria-label="Toggle sidebar" onClick={() => setSidebar(sidebar === false)}><PanelLeftIcon /></button>
            <div className="cn-file">
              <small>Format type here</small>
              {error ?? currentFile?.path ?? ''}
            </div>
            <ViewToggleControl fraction={split} onSelect={(fraction) => splitFrame.current?.setFraction(fraction)} />
            <div className="cn-actions">
              <select
                aria-label="Preview mode"
                value={previewEditing === true ? 'edit' : 'view'}
                onChange={(event) => setPreviewEditing(event.target.value === 'edit')}
              >
                <option value="edit">Editor</option>
                <option value="view">Viewer</option>
              </select>
              <button aria-pressed={authors} onClick={() => setAuthors(authors === false)}>
                <PeopleIcon size={16} />Authors
              </button>
              <label>
                <CommentsShowIcon />
                <select
                  aria-label="Comments"
                  value={commentsMode}
                  onChange={(event) => setCommentsMode(event.target.value as CommentsMode)}
                >
                  <option value="expand">Comments: expanded</option>
                  <option value="show">Comments</option>
                  <option value="hide">Comments: hidden</option>
                </select>
              </label>
              <button><PrintIcon />Print</button>
              <button><span>⛶</span>Fullscreen</button>
            </div>
          </div>
          <div className="cn-split">
            <SplitFrame
              ref={splitFrame}
              left={<ProjectDocument />}
              right={<ProjectPreview commentsMode={commentsMode} authors={authors} />}
              onFractionChange={setSplit}
              dividerLabel="Resize editor and preview panes"
            />
          </div>
          <footer className="cn-bar">
            <small>› Version history</small>
            <ProjectSyncStatus scope="document" />
          </footer>
        </main>
      </div>
    </div>
  );
}

export default function ConnectDemo() {
  return (
    <ThemeProvider>
      <ProjectProvider syncServer={SYNC_SERVER_URL} indexDocId={INDEX_DOC_ID} name="Embedded project" initialFile={FILE_PATH}>
        <ConnectPage />
      </ProjectProvider>
    </ThemeProvider>
  );
}
