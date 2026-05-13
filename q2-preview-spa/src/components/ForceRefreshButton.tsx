/**
 * Always-visible "refresh preview" affordance.
 *
 * Phase A.6 (bd-b5hf) — the epic's resolution #4 (force-refresh
 * invariant): the dep-graph that drives auto-rerenders won't always
 * know that a cross-document edit affects the active page (and in
 * Phase A the project doesn't yet *have* a dep graph), so the SPA
 * always exposes a manual escape hatch. Click → caller bumps its
 * render trigger; in PreviewApp that's the `contentTick` counter
 * already used by the sync handler's `onFileContent`.
 *
 * Positioning is `position: absolute` against a `position: relative`
 * parent. The component does not own a wrapper — it expects its
 * caller to render it as a sibling of whatever fills the pane (the
 * `<Q2PreviewIframe>` in PreviewApp's ready state). That keeps the
 * iframe at full size and the button floating at the corner without
 * an extra flex/grid layer.
 */

interface ForceRefreshButtonProps {
  onRefresh: () => void;
}

export function ForceRefreshButton({ onRefresh }: ForceRefreshButtonProps) {
  return (
    <button
      type="button"
      aria-label="Refresh preview"
      title="Refresh preview"
      onClick={onRefresh}
      style={{
        position: 'absolute',
        top: '0.75rem',
        right: '0.75rem',
        zIndex: 10,
        width: '2rem',
        height: '2rem',
        padding: 0,
        display: 'inline-flex',
        alignItems: 'center',
        justifyContent: 'center',
        border: '1px solid rgba(0, 0, 0, 0.15)',
        borderRadius: '50%',
        background: 'rgba(255, 255, 255, 0.85)',
        color: 'rgba(0, 0, 0, 0.7)',
        cursor: 'pointer',
        fontSize: '1rem',
        lineHeight: 1,
        boxShadow: '0 1px 2px rgba(0, 0, 0, 0.08)',
      }}
    >
      {/* U+21BB (clockwise open circle arrow) — the standard
          refresh glyph, supported by every system font, no asset
          dep. The aria-label above carries the meaning for AT. */}
      <span aria-hidden="true">↻</span>
    </button>
  );
}
