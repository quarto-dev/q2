/**
 * Shared icon module — the single source for UI icons in hub-client.
 *
 * Contract:
 * - Icons are **decorative**: every icon renders `aria-hidden="true"`.
 *   Meaning is conveyed by the wrapping control's `aria-label` (icon-only
 *   buttons) or by visible text next to the icon. Never give an icon its
 *   own accessible name.
 * - One visual style: 24×24 viewBox, `currentColor` stroke, stroke-width 2,
 *   round caps/joins (the Lucide/Feather style already in use).
 * - Sizes come from the `size` prop (default 16). Use 16 for toolbar/row
 *   icons, 13 for compact list affordances, 12×10 layout glyphs are the
 *   fixed-size exception (view-toggle and comments-mode pictograms).
 * - Color is always `currentColor` — never hardcode a fill/stroke color;
 *   the parent control's CSS owns color.
 *
 * When adding an icon: follow the style above, give it a doc comment
 * naming the concept it represents, and render it in the DevHarness
 * gallery (`#/dev/gallery`) so states are covered by visual baselines.
 */

import type { ReactNode } from 'react';

export interface IconProps {
  /** Width/height in px. Default 16. */
  size?: number;
}

interface StrokeIconProps extends IconProps {
  children: ReactNode;
  /** Override stroke width (default 2). */
  strokeWidth?: number;
}

/** Shared wrapper for the standard 24×24 stroke-icon style. */
function StrokeIcon({ size = 16, strokeWidth = 2, children }: StrokeIconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

/** Document with a plus — "new file". */
export function FilePlusIcon({ size }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
      <path d="M14 2v4a2 2 0 0 0 2 2h4" />
      <path d="M9 15h6" />
      <path d="M12 12v6" />
    </StrokeIcon>
  );
}

/** Arrow rising out of a tray — "upload". */
export function UploadIcon({ size }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
      <polyline points="17 8 12 3 7 8" />
      <line x1="12" y1="3" x2="12" y2="15" />
    </StrokeIcon>
  );
}

/** Printer — "open printable version". */
export function PrintIcon({ size }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <polyline points="6 9 6 2 18 2 18 9" />
      <path d="M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2" />
      <rect x="6" y="14" width="12" height="8" />
    </StrokeIcon>
  );
}

/** Grid of four squares — "switch / all projects". */
export function SwitchIcon({ size }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <rect x="3" y="3" width="7" height="7" rx="1" />
      <rect x="14" y="3" width="7" height="7" rx="1" />
      <rect x="3" y="14" width="7" height="7" rx="1" />
      <rect x="14" y="14" width="7" height="7" rx="1" />
    </StrokeIcon>
  );
}

/** Connected nodes — "share". */
export function ShareIcon({ size }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <circle cx="18" cy="5" r="3" />
      <circle cx="6" cy="12" r="3" />
      <circle cx="18" cy="19" r="3" />
      <line x1="8.59" y1="13.51" x2="15.42" y2="17.49" />
      <line x1="15.41" y1="6.51" x2="8.59" y2="10.49" />
    </StrokeIcon>
  );
}

/** Outward corners — "fullscreen preview". */
export function PreviewIcon({ size }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <path d="M8 3H5a2 2 0 0 0-2 2v3" />
      <path d="M21 8V5a2 2 0 0 0-2-2h-3" />
      <path d="M3 16v3a2 2 0 0 0 2 2h3" />
      <path d="M16 21h3a2 2 0 0 0 2-2v-3" />
    </StrokeIcon>
  );
}

/** Three nodes with branch lines — "duplicate / fork". */
export function ForkIcon({ size = 13 }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <circle cx="6" cy="5" r="2.2" />
      <circle cx="18" cy="5" r="2.2" />
      <circle cx="12" cy="19" r="2.2" />
      <path d="M6 7.5v1.5c0 1.7 1.3 3 3 3h6c1.7 0 3-1.3 3-3V7.5M12 12v4.5" />
    </StrokeIcon>
  );
}

/** Magnifying glass — "peek / preview on hover". */
export function PeekIcon({ size = 13 }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="M15.5 15.5L21 21" />
    </StrokeIcon>
  );
}

/** Two people — "collection members". */
export function PeopleIcon({ size = 12 }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <circle cx="9" cy="8" r="3.4" />
      <path d="M3 19c0-3 2.7-4.8 6-4.8s6 1.8 6 4.8" />
      <circle cx="17" cy="9" r="2.6" />
      <path d="M16.5 14.4c2.6.3 4.5 1.9 4.5 4.1" />
    </StrokeIcon>
  );
}

/** Opposing vertical arrows — "sort order". */
export function SortIcon({ size = 13 }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <path d="M7 4v14M7 18l-3.5-3.5M7 18l3.5-3.5" />
      <path d="M17 20V6M17 6l-3.5 3.5M17 6l3.5 3.5" />
    </StrokeIcon>
  );
}

/** Horizontal ellipsis — "more actions" (kebab/menu affordance). */
export function MoreIcon({ size = 16 }: IconProps) {
  return (
    <StrokeIcon size={size}>
      <circle cx="5" cy="12" r="1" fill="currentColor" />
      <circle cx="12" cy="12" r="1" fill="currentColor" />
      <circle cx="19" cy="12" r="1" fill="currentColor" />
    </StrokeIcon>
  );
}

/* ------------------------------------------------------------------ */
/* Layout pictograms (fixed 12×10 viewBox) — view-toggle glyphs.      */
/* ------------------------------------------------------------------ */

interface LayoutGlyphProps {
  children: ReactNode;
}

function LayoutGlyph({ children }: LayoutGlyphProps) {
  return (
    <svg width="12" height="10" viewBox="0 0 12 10" aria-hidden="true">
      {children}
    </svg>
  );
}

/** Wide left pane, dim right pane — "expand markup". */
export function LayoutMarkupIcon() {
  return (
    <LayoutGlyph>
      <rect x="0" y="0" width="7" height="10" rx="0.5" fill="currentColor" />
      <rect x="9" y="0" width="3" height="10" rx="0.5" fill="currentColor" opacity="0.25" />
    </LayoutGlyph>
  );
}

/** Two equal panes — "split equally". */
export function LayoutSplitIcon() {
  return (
    <LayoutGlyph>
      <rect x="0" y="0" width="5" height="10" rx="0.5" fill="currentColor" />
      <rect x="7" y="0" width="5" height="10" rx="0.5" fill="currentColor" />
    </LayoutGlyph>
  );
}

/** Dim left pane, wide right pane — "expand preview". */
export function LayoutPreviewIcon() {
  return (
    <LayoutGlyph>
      <rect x="0" y="0" width="3" height="10" rx="0.5" fill="currentColor" opacity="0.25" />
      <rect x="5" y="0" width="7" height="10" rx="0.5" fill="currentColor" />
    </LayoutGlyph>
  );
}

/* ------------------------------------------------------------------ */
/* Comments-mode pictograms (fixed 12×10 viewBox) — replay drawer.    */
/* ------------------------------------------------------------------ */

const BUBBLE_PATH =
  'M1 0 h10 a1 1 0 0 1 1 1 v5 a1 1 0 0 1 -1 1 H5 L2 10 V7 H1 a1 1 0 0 1 -1 -1 V1 a1 1 0 0 1 1 -1 Z';
const TALL_BUBBLE_PATH =
  'M1 0 h10 a1 1 0 0 1 1 1 v6 a1 1 0 0 1 -1 1 H5 L2 10 V8 H1 a1 1 0 0 1 -1 -1 V1 a1 1 0 0 1 1 -1 Z';

/** Tall comment bubble — "expand all comments". */
export function CommentsExpandIcon() {
  return (
    <LayoutGlyph>
      <path d={TALL_BUBBLE_PATH} fill="currentColor" />
    </LayoutGlyph>
  );
}

/** Comment bubble — "show comment bubbles". */
export function CommentsShowIcon() {
  return (
    <LayoutGlyph>
      <path d={BUBBLE_PATH} fill="currentColor" />
    </LayoutGlyph>
  );
}

/** Dim comment bubble with strike-through — "hide comment bubbles". */
export function CommentsHideIcon() {
  return (
    <LayoutGlyph>
      <path d={BUBBLE_PATH} fill="currentColor" opacity="0.25" />
      <line x1="1" y1="9" x2="11" y2="1" stroke="currentColor" strokeWidth="1.5" />
    </LayoutGlyph>
  );
}
