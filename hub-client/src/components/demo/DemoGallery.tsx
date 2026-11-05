/**
 * #/demo — index of standalone example pages; #/demo/<id> renders one.
 * Add a demo by registering it in DEMOS.
 */

import type { ComponentType, ReactNode } from 'react';
import SimpleAuthDemo from './SimpleAuthDemo';
import RadioTower from './RadioTower';
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
