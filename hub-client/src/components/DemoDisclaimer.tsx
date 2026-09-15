/**
 * The demo disclaimer (bd-m6u9qu3u): use test data, not real data; and
 * nothing entered is protected or returned.
 *
 * Shown on every card a visitor can reach without a session — the
 * landing / sign-in screen and the invite landing cards — since each is
 * the last thing they read before signing in and entering data. Always
 * open: a notice about what not to enter must not need a click.
 *
 * Structured copy rendered as plain elements rather than markdown. These
 * cards show before the WASM renderer the About tab uses for its markdown
 * documents exists, and the shape is fixed anyway: a heading, then two
 * paragraphs that each open with a bold single-sentence lead. The host
 * card owns the block's outer margins (`.ls-card .demo-disclaimer`,
 * `.il-card .demo-disclaimer`); this file owns its type and colors.
 */

import './DemoDisclaimer.css';
import { useId } from 'react';
import { demoDisclaimer } from '../strings';

export default function DemoDisclaimer() {
  const headingId = useId();
  return (
    <section className="demo-disclaimer" aria-labelledby={headingId}>
      <h2 id={headingId} className="demo-disclaimer-heading">
        {demoDisclaimer.heading}
      </h2>
      {[demoDisclaimer.useTestData, demoDisclaimer.notProtected].map((item) => (
        <p key={item.lead}>
          <strong>{item.lead}</strong> {item.body}
        </p>
      ))}
    </section>
  );
}
