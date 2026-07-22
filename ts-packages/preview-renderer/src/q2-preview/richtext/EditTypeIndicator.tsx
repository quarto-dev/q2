// bd-igpm0xur — EditToolbar's type/nesting indicator. Reads editTarget/sourceIndex/
// unlockNestingCursor from context (no ctx prop). Nesting ON → full BreadcrumbCrumbs
// (◀/▶ path); OFF (default) → one non-interactive current-type crumb (the
// buildAncestorPath last entry) so a code chunk's toolbar is never empty.

import { useContext } from 'react';
import { PreviewContext } from '../PreviewContext';
import { buildAncestorPath } from '../nestingNav';
import { BreadcrumbCrumbs, ensureBreadcrumbStyles } from '../BreadcrumbCrumbs';

export function EditTypeIndicator() {
  // Inject the crumb stylesheet here too: the minimal (nesting-off) branch doesn't
  // render BreadcrumbCrumbs (the only other injector), so the crumb would be unstyled.
  ensureBreadcrumbStyles();

  const ctx = useContext(PreviewContext);
  const et = ctx?.editTarget;
  if (!ctx || !et) return null;

  const crumbs = buildAncestorPath(ctx.sourceIndex, et.anchorR0, et.anchorR1);
  if (crumbs.length === 0) return null;

  if (ctx.unlockNestingCursor) {
    return <BreadcrumbCrumbs crumbs={crumbs} />;
  }

  // Minimal current-type crumb (non-interactive; .q2-crumb is content-sized).
  const current = crumbs[crumbs.length - 1];
  return (
    <span
      className={`q2-crumb q2-crumb-cat-${current.category} q2-crumb-current`}
      title={current.label}
      aria-current="true"
    >
      {current.abbrev}
    </span>
  );
}
