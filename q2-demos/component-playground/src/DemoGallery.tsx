/**
 * index.html — the list of demos. Each demo is a page of its own
 * (`<id>.html`, mounted by `src/pages/<id>.tsx`), so it loads only the
 * hub-client modules and stylesheets it imports.
 *
 * Add a demo by listing it in DEMOS and in PAGES in vite.config.ts, and
 * adding its `<id>.html` and `src/pages/<id>.tsx`.
 */

import type { ReactNode } from 'react';
import RadioTower from './RadioTower';
import { QuartoFolderIcon } from 'hub-client/src/components/icons';
import './DemoGallery.css';

interface Demo {
  title: string;
  description: string;
  icon: ReactNode;
}

const DEMOS: Record<string, Demo> = {
  'simple-auth': {
    title: 'Simple auth',
    description: 'A local Automerge counter that syncs to the hub only while signed in.',
    icon: <RadioTower lit={false} />,
  },
  monaco: {
    title: 'Monaco editor',
    description: 'The hub-client document editor on a project file: Automerge sync, remote cursors, qmd highlighting, outline and folding, Cmd+B, link and image paste.',
    icon: <span className="dg-glyph">✎</span>,
  },
  preview: {
    title: 'Preview',
    description: 'Read-only preview of the same project file, rendered by the hub preview pipeline.',
    icon: <span className="dg-glyph">◎</span>,
  },
  project: {
    title: 'Project',
    description: 'The hub-client file sidebar listing the same project, live from its index doc.',
    icon: <span className="dg-glyph"><QuartoFolderIcon size={40} /></span>,
  },
  canvas: {
    title: 'Canvas',
    description: 'Every part of the hub-client editor as a node on an infinite canvas — server, websocket adapter, repo, project sync, file explorer, WASM and its VFS, document, editor, raw preview, language, presence, identity, LSP, preview, and the stores — with the dependencies as edges.',
    icon: <span className="dg-glyph">⧉</span>,
  },
  embed: {
    title: 'Embedded editor',
    description: 'An editor a host builds from hub-client parts: its own header and layout around the file tree, document, preview and outline, which find each other through one ProjectProvider.',
    icon: <span className="dg-glyph">▣</span>,
  },
  embed2: {
    title: 'Embedded editor, lava theme',
    description: 'The same embedded editor, restyled by the host: one extra stylesheet overrides hub-client\'s theme tokens, in light and dark, without targeting a hub-client class.',
    icon: <span className="dg-glyph">▲</span>,
  },
};

export default function DemoGallery() {
  return (
    <div className="dg-page">
      <h1>Demos</h1>
      <ul className="dg-list">
        {Object.entries(DEMOS).map(([id, { title, description, icon }]) => (
          <li key={id}>
            <span className="dg-icon">{icon}</span>
            <div>
              <a href={`${id}.html`}>{title}</a>
              <p>{description}</p>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
