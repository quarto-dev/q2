/** Shared chrome for demo pages: gray page, "← Demos" bar, white rounded card. */

import type { ReactNode } from 'react';
import './DemoFrame.css';

export default function DemoFrame({ info, children }: { info?: ReactNode; children: ReactNode }) {
  return (
    <div className="df-page">
      <header className="df-bar">
        <a href="#/demo">← Demos</a>
        {info !== undefined && <span className="df-info">{info}</span>}
      </header>
      <div className="df-card">{children}</div>
    </div>
  );
}
