/** Shared chrome for demo pages: gray page, "← Demos" bar, white rounded card. */

import type { ReactNode } from 'react';
// The demos in this frame import hub-client components one by one, so they
// load the global stylesheets hub-client's own entry point would have.
// EmbedDemo does not use this frame: it gets only what `embed` imports.
import 'hub-client/src/theme.css';
import 'hub-client/src/index.css';
import 'hub-client/src/ui.css';
import './DemoFrame.css';

export default function DemoFrame({ info, children }: { info?: ReactNode; children: ReactNode }) {
  return (
    <div className="df-page">
      <header className="df-bar">
        <a href="index.html">← Demos</a>
        {info !== undefined && <span className="df-info">{info}</span>}
      </header>
      <div className="df-card">{children}</div>
    </div>
  );
}
