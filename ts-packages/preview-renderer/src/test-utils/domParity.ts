/**
 * DOM canonicalisation for preview ↔ render parity.
 *
 * Plan: claude-notes/plans/2026-08-24-preview-render-dom-parity-harness.md
 *
 * Converts an Element subtree to a line-oriented canonical text so two
 * independently-produced DOMs (the native HTML writer's, parsed by
 * jsdom; the React renderer's, mounted by testing-library) can be
 * compared with a plain string diff. Every normalisation applied here
 * is listed in the plan's § Normalisation table with its reason; do not
 * add a rule without one.
 *
 * Test-only: this directory is excluded from the package build, and
 * hub-client reaches it through vitest's `@quarto/preview-renderer`
 * source alias, not the package exports map.
 */

export interface ParityRules {
  /** Attribute names removed from every element before comparison. */
  stripAttrs: ReadonlySet<string>;
  /** Attribute names whose presence on either side is an error. */
  forbidAttrs: ReadonlySet<string>;
  /**
   * CSS selectors whose matching elements keep their tag + attributes
   * but have their children replaced by a single OPAQUE_MARKER line.
   */
  opaqueSelectors: readonly string[];
  /**
   * Tag names whose elements are replaced by their children when no
   * attribute survives `stripAttrs`. Applied on the clone before text
   * nodes are merged, so the unwrapped children take part in the
   * whitespace pass as siblings of the wrapper's neighbours.
   */
  unwrapTags: ReadonlySet<string>;
}

export const OPAQUE_MARKER = '⟨opaque⟩';

/** Thrown when a forbidden attribute is found (see PARITY_RULES.forbidAttrs). */
export class ParityRuleViolation extends Error {}

export const PARITY_RULES: ParityRules = {
  stripAttrs: new Set<string>([
    // Source tracking: emitted only when `include_source_locations` is on
    // (crates/pampa/src/writers/html.rs, `data-sid`/`data-loc` doc block
    // ~L743-748). Off for `q2 render`; on for the preview AST
    // (`PreviewAstOutput.ast_json` is written with
    // `include_inline_locations: true`, crates/quarto-core/src/pipeline.rs
    // ~L195) and forwarded by React via `dataLocProps`. Preview-only by
    // construction.
    //
    // NOT listed: `data-block-pool-id` (React edit chrome). The parity
    // runner mounts read-only (no PreviewContext), so it is never emitted;
    // add it here only if the mount configuration changes.
    'data-loc',
    'data-sid',
  ]),
  forbidAttrs: new Set<string>([
    // Consumed by both writers — `write_code_container_attr`
    // (crates/pampa/src/writers/html.rs ~L539) and
    // q2-preview/blocks/CodeBlock.tsx decode it into <span class="hl-…">
    // markup and must NOT forward it. Leakage is a bug (bd-nxslt), so the
    // harness errors instead of normalising it away.
    'data-hl-spans',
  ]),
  opaqueSelectors: [
    // `math-js` is excluded from the preview pipeline
    // (Q2_PREVIEW_STAGE_EXCLUDED, crates/quarto-core/src/pipeline.rs ~L394):
    // render leaves TeX in \( \) delimiters for MathJax; React
    // (q2-preview/inlines/Math.tsx) emits KaTeX HTML. Divergent by design.
    // The <span> itself and its `math inline|display` classes still
    // compare — which is why bd-tmb2u5yu (Math.tsx emits no class) must
    // close before any fixture containing math can opt in.
    'span.math',
  ],
  unwrapTags: new Set<string>([
    // React cannot inject raw HTML without a host element:
    // q2-preview/blocks/RawBlock.tsx wraps every RawBlock(format:"html")
    // in a <div dangerouslySetInnerHTML> (carrying only data-loc) that the
    // native writer (crates/pampa/src/writers/html.rs, Block::RawBlock)
    // never emits — most visibly the code-copy button. Symmetric on
    // purpose: a Div block with an empty Attr is a bare <div> on BOTH
    // sides, so a preview-only unwrap would false-positive. Accepted
    // cost: a missing/extra bare <div> is invisible to the runner. Found
    // by the Task 0.2 spike
    // (claude-notes/research/2026-08-24-preview-render-parity-spike.md).
    'div',
  ]),
};

/**
 * Elements whose adjacency to a text node makes that text node's edge
 * whitespace significant. Whitespace next to anything else (block
 * elements, or nothing) is the writer's pretty-printing and is dropped.
 *
 * `button`, `input`, `label`, `svg` are listed even though they aren't
 * classically "inline" HTML: `label`/`input` are how the writer renders
 * task items (`<label><input type="checkbox">…</label>`,
 * crates/pampa/src/writers/html.rs ~L1347), and `button`/`svg` are the
 * code-copy scaffold — both sit directly next to significant text.
 */
export const INLINE_TAGS: ReadonlySet<string> = new Set([
  'a', 'abbr', 'b', 'br', 'button', 'cite', 'code', 'del', 'em', 'i', 'img',
  'input', 'ins', 'kbd', 'label', 'mark', 'q', 's', 'samp', 'small', 'span',
  'strong', 'sub', 'sup', 'svg', 'time', 'u', 'var',
]);

const ELEMENT_NODE = 1;
const TEXT_NODE = 3;

function escapeAttr(value: string): string {
  // Newline → space keeps the canonical form one-line-per-node (a literal
  // newline in an attribute value would break the line-oriented diff);
  // attribute newlines are not semantic in HTML, so this is lossless for
  // comparison purposes.
  return value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/\n/g, ' ');
}

function isInlineNeighbor(n: Node | null): boolean {
  if (!n) return false;
  if (n.nodeType === TEXT_NODE) return (n.textContent ?? '').trim().length > 0;
  if (n.nodeType === ELEMENT_NODE) return INLINE_TAGS.has((n as Element).tagName.toLowerCase());
  return false;
}

/** Whitespace-normalised text outside <pre>; '' means "drop this node". */
function normalizeText(node: Node): string {
  let s = (node.textContent ?? '').replace(/\s+/g, ' ');
  if (!isInlineNeighbor(node.previousSibling)) s = s.replace(/^ /, '');
  if (!isInlineNeighbor(node.nextSibling)) s = s.replace(/ $/, '');
  return s;
}

function openTagLine(el: Element, rules: ParityRules): string {
  const attrs: Array<{ name: string; line: string }> = [];
  for (const { name, value } of Array.from(el.attributes)) {
    if (rules.forbidAttrs.has(name)) {
      throw new ParityRuleViolation(
        `forbidden attribute '${name}' on <${el.tagName.toLowerCase()}> — see PARITY_RULES.forbidAttrs`,
      );
    }
    if (rules.stripAttrs.has(name)) continue;
    const v = name === 'class' ? value.replace(/\s+/g, ' ').trim() : value;
    attrs.push({ name, line: `${name}="${escapeAttr(v)}"` });
  }
  attrs.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  const tag = el.tagName.toLowerCase();
  return attrs.length ? `<${tag} ${attrs.map((a) => a.line).join(' ')}>` : `<${tag}>`;
}

function walk(node: Node, depth: number, rules: ParityRules, out: string[], inPre: boolean): void {
  const pad = '  '.repeat(depth);
  if (node.nodeType === TEXT_NODE) {
    const text = inPre ? (node.textContent ?? '') : normalizeText(node);
    if (text) out.push(pad + JSON.stringify(text));
    return;
  }
  if (node.nodeType !== ELEMENT_NODE) return; // comments, processing instructions
  const el = node as Element;
  out.push(pad + openTagLine(el, rules));
  if (rules.opaqueSelectors.some((sel) => el.matches(sel))) {
    out.push(`${'  '.repeat(depth + 1)}${OPAQUE_MARKER}`);
    return;
  }
  const childInPre = inPre || el.tagName.toLowerCase() === 'pre';
  for (const child of Array.from(el.childNodes)) walk(child, depth + 1, rules, out, childInPre);
}

/** True when every attribute on `el` is in `rules.stripAttrs`. */
function hasNoSurvivingAttrs(el: Element, rules: ParityRules): boolean {
  return Array.from(el.attributes).every((a) => rules.stripAttrs.has(a.name));
}

/**
 * Replace every `rules.unwrapTags` element that has no surviving
 * attributes by its children (document order, so nested wrappers unwrap
 * too). Mutates `root`, which must be the private clone.
 */
function unwrapBareElements(root: Element, rules: ParityRules): void {
  for (const tag of rules.unwrapTags) {
    for (const el of Array.from(root.querySelectorAll(tag))) {
      if (hasNoSurvivingAttrs(el, rules)) el.replaceWith(...Array.from(el.childNodes));
    }
  }
}

/**
 * Canonical, line-oriented rendering of `el` and its subtree. Works on a
 * clone so the caller's DOM is untouched. Order on the clone: unwrap bare
 * wrapper elements (rules.unwrapTags, judged after the strip list) →
 * merge adjacent text nodes with `normalize()` (React emits one text
 * node per Str/Space) → walk (strip/forbid/sort attributes, opaque
 * subtrees, whitespace).
 */
export function canonicalize(el: Element, rules: ParityRules = PARITY_RULES): string {
  const clone = el.cloneNode(true) as Element;
  unwrapBareElements(clone, rules);
  clone.normalize();
  const out: string[] = [];
  walk(clone, 0, rules, out, false);
  return out.join('\n');
}

/** The subtree both pipelines are contractually required to agree on. */
export const PARITY_ROOT_SELECTOR = 'main#quarto-document-content';

/**
 * Locate the parity root inside a document / container. `label` names
 * the side ("render" / "preview") so a missing root is attributable.
 */
export function extractParityRoot(scope: ParentNode, label: string): Element {
  const root = scope.querySelector(PARITY_ROOT_SELECTOR);
  if (!root) {
    throw new Error(`${label}: no element matches ${PARITY_ROOT_SELECTOR}`);
  }
  return root;
}

export interface ParityResult {
  equal: boolean;
  /** Canonical text of the render side. */
  render: string;
  /** Canonical text of the preview side. */
  preview: string;
}

/**
 * Canonicalise both roots and compare. Never throws for a mismatch; does
 * throw (ParityRuleViolation) on a forbidden attribute.
 */
export function compareParity(
  renderRoot: Element,
  previewRoot: Element,
  rules: ParityRules = PARITY_RULES,
): ParityResult {
  const render = canonicalize(renderRoot, rules);
  const preview = canonicalize(previewRoot, rules);
  return { equal: render === preview, render, preview };
}
