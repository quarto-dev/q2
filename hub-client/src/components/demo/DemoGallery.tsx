/**
 * #/demo — index of standalone example pages; #/demo/<id> renders one.
 * Add a demo by registering it in DEMOS.
 */

import type { ComponentType, ReactNode } from 'react';
import SimpleAuthDemo from './SimpleAuthDemo';
import MonacoDemo from './MonacoDemo';
import PreviewDemo from './PreviewDemo';
import ProjectDemo from './ProjectDemo';
import RadioTower from './RadioTower';
import { QuartoFolderIcon } from '../icons';
import './DemoGallery.css';

interface Demo {
  title: string;
  description: string;
  icon: ReactNode;
  Component: ComponentType;
}

const DEMOS: Record<string, Demo> = {
  'simple-auth': {
    title: 'Simple auth',
    description: 'A local Automerge counter that syncs to the hub only while signed in.',
    icon: <RadioTower lit={false} />,
    Component: SimpleAuthDemo,
  },
  monaco: {
    title: 'Monaco editor',
    description: 'The hub-client Monaco editor on an Automerge text doc, synced through a public server.',
    icon: <span className="dg-glyph">✎</span>,
    Component: MonacoDemo,
  },
  preview: {
    title: 'Preview',
    description: 'Read-only preview of the same project file, rendered by the hub preview pipeline.',
    icon: <span className="dg-glyph">◎</span>,
    Component: PreviewDemo,
  },
  project: {
    title: 'Project',
    description: 'The hub-client file sidebar listing the same project, live from its index doc.',
    icon: <span className="dg-glyph"><QuartoFolderIcon size={40} /></span>,
    Component: ProjectDemo,
  },
};

export default function DemoGallery({ page }: { page?: string }) {
  const demo = page !== undefined ? DEMOS[page] : undefined;
  if (demo !== undefined) return <demo.Component />;

  return (
    <div className="dg-page">
      <h1>Demos</h1>
      {page !== undefined && <p className="dg-missing">No demo named “{page}”.</p>}
      <ul className="dg-list">
        {Object.entries(DEMOS).map(([id, { title, description, icon }]) => (
          <li key={id}>
            <span className="dg-icon">{icon}</span>
            <div>
              <a href={`#/demo/${id}`}>{title}</a>
              <p>{description}</p>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
