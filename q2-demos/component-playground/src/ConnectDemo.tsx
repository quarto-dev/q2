/**
 * embed3.html — the embedded editor laid out as a Posit Connect page: the
 * host's top bar and sidebar sections around a card that holds the
 * document and preview. Same parts as EmbedDemo; only the layout differs.
 */

import { useEffect, useRef, useState } from 'react';
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
  type RenderComment,
  type SplitFrameHandle,
} from 'hub-client/src/embed';
import { CommentsShowIcon, GearIcon, PanelLeftIcon, PeopleIcon, PrintIcon } from 'hub-client/src/components/icons';
import { FILE_PATH, INDEX_DOC_ID, SYNC_SERVER_URL } from './hubProject';
import './ConnectDemo.css';

// The built-in `Block` and `Inline` registry entries are the comment chrome;
// this module replaces both. Its `Block` takes comments (`[>> ...]` spans)
// out of the text; the host shows them in its own gutter instead, and posts
// the hovered comment's text here so its block can be outlined. Only the
// preview knows which comments share a block, so it posts that back. It
// runs inside the preview iframe, so it is source text, not an import.
const HOVER_COMMENT = 'cn-hover-comment';
const COMMENT_BLOCKS = 'cn-comment-blocks';

const RENDER_COMPONENTS = {
  comments: `
const React = window.React;
const { Block: B, Inline, inlinesToPlainText } = window.__Q2_PREVIEW_RENDERER__;
export { Inline };

const isComment = (inline) => inline.t === 'Span' && inline.c[0][1].includes('quarto-edit-comment');

// Every commented block's comment texts, posted to the host whole on each change.
const blocks = new Map();
const postBlocks = () => window.parent.postMessage({ type: '${COMMENT_BLOCKS}', blocks: [...blocks.values()] }, '*');

// Sits unseen ahead of a commented block. It reports the block's comments
// to the host, and outlines the block and scrolls it to the top of the
// preview while one of them is hovered there.
// A Plain has no element of its own, so the outline falls back to the
// parent (the list item).
const Marker = ({ texts }) => {
  const ref = React.useRef(null);
  React.useEffect(() => {
    blocks.set(ref, texts);
    postBlocks();
    return () => {
      blocks.delete(ref);
      postBlocks();
    };
  }, [JSON.stringify(texts)]);
  React.useEffect(() => {
    const onMessage = (event) => {
      if (event.data?.type !== '${HOVER_COMMENT}') return;
      const block = ref.current.nextElementSibling ?? ref.current.parentElement;
      const hovered = texts.includes(event.data.text);
      block.style.outline = hovered ? '2px solid #e0a800' : '';
      if (hovered) block.scrollIntoView({ block: 'start', behavior: 'smooth' });
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  });
  return <span ref={ref} hidden />;
};

// A Para's or Plain's inlines are its 'c'; a Header's are 'c[2]'.
export const Block = ({ node, ...rest }) => {
  const inlines = node.t === 'Para' || node.t === 'Plain' ? node.c : node.t === 'Header' ? node.c[2] : [];
  const comments = inlines.filter(isComment);
  if (comments.length === 0) return <B node={node} {...rest} />;
  const text = inlines.filter((inline) => isComment(inline) === false);
  const shown = node.t === 'Header' ? { ...node, c: [node.c[0], node.c[1], text] } : { ...node, c: text };
  return (
    <>
      <Marker texts={comments.map((comment) => inlinesToPlainText(comment.c[1]).trim())} />
      <B node={shown} {...rest} />
    </>
  );
};
`,
};

/** Tell the preview which comment is hovered in the gutter (null: none). */
function hoverComment(text: string | null) {
  document
    .querySelector<HTMLIFrameElement>('.cn-split iframe')
    ?.contentWindow?.postMessage({ type: HOVER_COMMENT, text }, '*');
}

/**
 * Split the file's comments (in document order) into one group per block.
 * `blocks` is each commented block's comment texts as the preview reported
 * them; a comment no block claims is a group of its own.
 */
function groupByBlock(comments: RenderComment[], blocks: string[][]): RenderComment[][] {
  const unused = [...blocks];
  const groups: RenderComment[][] = [];
  for (let i = 0; i < comments.length; ) {
    const at = unused.findIndex((texts) => texts.every((text, j) => comments[i + j]?.text.trim() === text));
    const size = at === -1 ? 1 : unused.splice(at, 1)[0].length;
    groups.push(comments.slice(i, i + size));
    i += size;
  }
  return groups;
}

function ConnectPage() {
  const { project, currentFile, editor, error } = useProject();
  const [comments, setComments] = useState<RenderComment[]>([]);
  const [commentBlocks, setCommentBlocks] = useState<string[][]>([]);
  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      if (event.data?.type === COMMENT_BLOCKS) setCommentBlocks(event.data.blocks);
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, []);
  const splitFrame = useRef<SplitFrameHandle>(null);
  const [split, setSplit] = useState(0.5);
  const [sidebar, setSidebar] = useState(true);
  const [authors, setAuthors] = useState(false);
  const [gutter, setGutter] = useState(true);
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
            <div className="cn-lead">
              <button aria-label="Toggle sidebar" onClick={() => setSidebar(sidebar === false)}><PanelLeftIcon /></button>
              <div>
                <small>Format type here</small>
                {error ?? currentFile?.path ?? ''}
              </div>
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
              <button aria-pressed={gutter} onClick={() => setGutter(gutter === false)}>
                <CommentsShowIcon />Comments
              </button>
              <button><PrintIcon />Print</button>
              <button><span>⛶</span>Fullscreen</button>
            </div>
          </div>
          <div className="cn-split">
            <SplitFrame
              ref={splitFrame}
              left={<ProjectDocument />}
              right={<ProjectPreview renderComponents={RENDER_COMPONENTS} onCommentsChange={setComments} authors={authors} />}
              onFractionChange={setSplit}
              dividerLabel="Resize editor and preview panes"
            />
            {gutter === true && (
              <aside className="cn-gutter">
                <small>Comments</small>
                {groupByBlock(comments, commentBlocks).map((group, i) => (
                  <div
                    key={i}
                    className="cn-thread"
                    onMouseEnter={() => hoverComment(group[0].text.trim())}
                    onMouseLeave={() => hoverComment(null)}
                  >
                    {group.map((comment, j) => (
                      <button key={j} onClick={() => editor?.revealLineInCenter(comment.start_line ?? 1)}>
                        {comment.text}
                      </button>
                    ))}
                  </div>
                ))}
              </aside>
            )}
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
