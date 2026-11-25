/**
 * Is a rendered preview styled? Pure: `collectPreviewFacts` (in `preview-probe.ts`) reads the facts out of the preview
 * iframe, and this decides. A themed page (the journal sets `theme: cosmo`) has stylesheet rules and a sans-serif body;
 * a page with no CSS has the browser defaults: a serif body, an 8 px body margin, and a 32 px `h1`.
 */

export interface SheetFact {
  href: string | null;
  /** Number of CSS rules, or null when the sheet cannot be read (not loaded, or cross-origin). */
  rules: number | null;
}

export interface PreviewFacts {
  /** The iframe's body has text; a preview that has not mounted yet is not judged. */
  hasContent: boolean;
  bodyFontFamily: string;
  bodyFontSizePx: number;
  bodyMarginPx: number;
  h1FontSizePx: number | null;
  sheets: SheetFact[];
}

export type StyleReason = 'no-stylesheet-rules' | 'default-font';

export interface StyleVerdict {
  styled: boolean;
  reasons: StyleReason[];
}

/** The font a browser uses for a page with no CSS (a generic serif; on macOS, Times). */
const DEFAULT_FONT = /^["']?(Times( New Roman)?|serif)["']?$/i;

export function judgePreviewStyle(f: PreviewFacts): StyleVerdict {
  if (!f.hasContent) return { styled: true, reasons: [] };
  const reasons: StyleReason[] = [];
  if (!f.sheets.some((s) => (s.rules ?? 0) > 0)) reasons.push('no-stylesheet-rules');
  const first = f.bodyFontFamily.split(',')[0]!.trim();
  if (DEFAULT_FONT.test(first)) reasons.push('default-font');
  return { styled: reasons.length === 0, reasons };
}
