/**
 * RevealDeck — render a `format: revealjs` deck in `q2 preview`.
 *
 * Phase 1P (B1a) of the revealjs epic. The Rust `RevealSlidesTransform`
 * (shared with `q2 render`) already produced the two-level slide tree as
 * nested `Div(.section)` blocks; this component maps that structure onto
 * `@revealjs/react` `<Deck>/<Slide>/<Stack>` for the reveal.js lifecycle, and
 * renders slide *content* through the framework `<Node>` dispatcher — i.e. the
 * SAME `previewRegistry` React mirror the html preview uses (already kept in
 * parity with the Rust HTML writer by the `/preview-render-parity` skill).
 *
 * This is the convergence point: slide construction lives once, in Rust;
 * content rendering lives once, in `previewRegistry`. There is no bespoke
 * TS slide splitter or content renderer here (the hub-client's `parseSlides`
 * / `renderBlock` are retired by this path).
 *
 * See claude-notes/plans/2026-06-08-revealjs-presentations.md (Phase 1P).
 */

import React, { useContext, useEffect, useLayoutEffect } from 'react';
import { Deck, Slide, Stack, useReveal } from '@revealjs/react';
// Reveal CSS from the SAME vendored copy `q2 render` links — `resources/
// revealjs/` — in the SAME cascade order, so render and preview cannot disagree
// on reveal styling (bd-ibqkf9ry). The `vendored_reveal_assets_match_npm_package`
// test keeps that copy byte-identical to the npm `reveal.js` `@revealjs/react`
// drives; importing the vendored files (not `reveal.js/*.css`) makes `resources/
// revealjs/` the single source of truth. `@revealjs/react` injects no CSS of its
// own — styling is entirely these imports plus the per-document theme below.
//
// The reveal *theme* slot is NOT a static import: bd-y259zb57 delivers the
// document's compiled Quarto reveal theme (default / named / custom .scss /
// `_brand.yml`) at runtime via the parent's `UPDATE_THEME` → `<link
// data-q2-theme>` transport — the same `css:theme:<fp>` artifact `q2 render`
// links in the theme slot. Statically importing `theme/white.css` here would
// pin every deck to stock white (centered, uppercase headings) regardless of
// `theme:`, the render↔preview divergence this strand fixes. `quarto-reveal.css`
// stays static (it is theme-independent: columns/asides/footnotes) and is purely
// additive, so the runtime theme link landing after it does not change the
// cascade.
// bd-dg8x84bu: import the .reveal-SCOPED reset, never the upstream global one.
// These are side-effecting CSS imports that Vite hoists into the single global
// q2-preview stylesheet, so they apply to ALL preview content. reveal.css and
// quarto-reveal.css are already fully .reveal-scoped (safe), but the upstream
// reset.css is a global Meyer page reset (html, body, ..., em { font: inherit })
// that zeroed font-style on <em>/<i>/<cite> in format:html documents. reset-scoped.css
// is the derived, .reveal-scoped equivalent — same effect on deck slides, no leak.
import '../../../../resources/revealjs/reset-scoped.css';
import '../../../../resources/revealjs/reveal.css';
import '../../../../resources/revealjs/quarto-reveal.css';

import { extractMetaBool, extractMetaString, getMetaPath, Node, RegistryContext } from '../framework';
import type { BlockNode, DivBlock, FormatRegistry, PandocAST } from '../framework';
import { IncrementalContext } from './IncrementalContext';

interface RevealDeckProps {
    ast: PandocAST;
    registry: FormatRegistry;
    currentFilePath: string;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    /**
     * Slide-navigation bridge (bd-mwbsdmel). `registerSlideNavigator`
     * receives an imperative `goTo(index)` the host can call to drive the
     * deck (cursor→slide sync); `null` is passed on unmount. `onSlideChange`
     * fires with the new horizontal index whenever the deck navigates
     * (arrows/controls/`goTo`), so the host can mirror the deck's position.
     * Both optional — the standalone `q2 preview` SPA omits them.
     */
    registerSlideNavigator?: (nav: ((index: number) => void) | null) => void;
    onSlideChange?: (slideIndex: number) => void;
}

/** A `Div` block carrying the `section` class — a reveal slide / stack. */
function isSectionDiv(block: BlockNode): block is DivBlock {
    return block.t === 'Div' && (block as DivBlock).c[0][1].includes('section');
}

const NOOP = () => {};

/**
 * Render a slide's content blocks via the shared previewRegistry.
 *
 * `sectionClasses` are the enclosing slide section's classes. A slide heading's
 * `.incremental` / `.nonincremental` (hoisted onto the section) must flip the
 * incremental scope for the slide's lists — but `RevealDeck` renders the
 * section's children directly (bypassing `Div.tsx`), so that override is
 * applied here. Mirrors the native writer flipping `writerIncremental` on a
 * `.incremental` section.
 */
function SlideBody(props: {
    blocks: BlockNode[];
    sectionClasses?: string[];
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}) {
    const parent = useContext(IncrementalContext);
    const classes = props.sectionClasses ?? [];
    let incremental = parent.incremental;
    if (classes.includes('incremental')) incremental = true;
    else if (classes.includes('nonincremental')) incremental = false;

    const body = (
        <>
            {props.blocks.map((block, i) => (
                <Node
                    key={i}
                    node={block}
                    onNavigateToDocument={props.onNavigateToDocument}
                    setLocalAst={NOOP}
                />
            ))}
        </>
    );

    if (incremental === parent.incremental) return body;
    return (
        <IncrementalContext.Provider value={{ enabled: parent.enabled, incremental }}>
            {body}
        </IncrementalContext.Provider>
    );
}

/**
 * Forward a section `Div`'s `Attr` (`[id, classes, kvs]`) onto the reveal
 * `<section>` as `id` + `className`, mirroring the native writer — the same
 * `id`/`class` `q2 render` puts on every `<section>`
 * (`crates/quarto-core/src/revealjs/assemble.rs`). Without this, preview
 * `<section>`s carry only reveal's runtime `present`/`future` classes: the
 * title slide loses `center` (reveal's vertical centering) and `title-slide`
 * (a theme-CSS hook), in-deck `#id` anchors break, and author per-slide classes
 * (`## Slide {.smaller}`) are dropped. F1+F2 of bd-vv8jft5n.
 *
 * Empty id / empty class list collapse to `undefined` so we don't emit
 * `id=""` / `class=""`.
 */
export function sectionAttrProps(div: DivBlock): { id?: string; className?: string } {
    const [id, classes] = div.c[0];
    return {
        id: id || undefined,
        className: classes.length > 0 ? classes.join(' ') : undefined,
    };
}

/**
 * Map one top-level section Div to a `<Slide>` (leaf) or `<Stack>` of
 * vertical `<Slide>`s (a section divider whose children are themselves
 * section Divs — the shape `RevealSlidesTransform` emits for `# Section`
 * dividers and their `## ` sub-slides).
 *
 * Each `<section>` carries its Div's `id` + classes via [`sectionAttrProps`].
 * Caveat: `@revealjs/react`'s `<Stack>` only accepts `className` (it drops
 * `id`), so a section-divider stack's own `id` cannot round-trip through the
 * component today — the inner vertical `<Slide>`s still get theirs. Tracked
 * under bd-vv8jft5n.
 */
export function renderTopSection(
    div: DivBlock,
    key: number,
    onNavigateToDocument?: (path: string, anchor: string | null) => void,
): React.ReactNode {
    const children = div.c[1];
    const verticalSlides = children.filter(isSectionDiv);

    if (verticalSlides.length > 0) {
        return (
            <Stack key={key} {...sectionAttrProps(div)}>
                {verticalSlides.map((slide, j) => (
                    <Slide key={j} {...sectionAttrProps(slide as DivBlock)}>
                        <SlideBody
                            blocks={(slide as DivBlock).c[1]}
                            sectionClasses={(slide as DivBlock).c[0][1]}
                            onNavigateToDocument={onNavigateToDocument}
                        />
                    </Slide>
                ))}
            </Stack>
        );
    }

    return (
        <Slide key={key} {...sectionAttrProps(div)}>
            <SlideBody
                blocks={children}
                sectionClasses={div.c[0][1]}
                onNavigateToDocument={onNavigateToDocument}
            />
        </Slide>
    );
}

/**
 * Read the pre-rendered deck-level footer/logo markup from the
 * `rendered.reveal.{footer,logo}` meta slots (populated by the
 * `reveal-footer-logo` transform — `crates/quarto-core/src/revealjs/
 * footer_logo.rs` — which runs in the q2-preview pipeline, so the markup
 * is already present in the AST handed to preview). Each slot is a raw
 * HTML string, byte-identical to what `q2 render` links.
 */
export function revealChromeFromMeta(meta: PandocAST['meta'] | undefined): {
    footerHtml?: string;
    logoHtml?: string;
} {
    return {
        logoHtml: extractMetaString(getMetaPath(meta, ['rendered', 'reveal', 'logo'])),
        footerHtml: extractMetaString(getMetaPath(meta, ['rendered', 'reveal', 'footer'])),
    };
}

/**
 * Place the deck-level footer/logo as **direct children of `.reveal`, OUTSIDE
 * `.slides`**, and add `has-logo` to `.reveal` — mirroring the native scaffold
 * (`crates/quarto-core/src/revealjs/assemble.rs::render_revealjs_document` +
 * `footer_logo_html`). `position: fixed` only resolves against the viewport
 * outside `.slides` (reveal applies a CSS `transform` to `.slides`/`<section>`).
 *
 * `@revealjs/react`'s `<Deck>` renders its children only *inside* `.slides`, so
 * we cannot place this chrome declaratively. Instead we reach the `.reveal`
 * element via `useReveal()` → `getRevealElement()` and inject the markup
 * imperatively — the same escape hatch `HeaderIncludesEffect` uses for
 * `document.head`. `RevealChrome` itself renders nothing in the React tree;
 * mount it as a child of `<Deck>` so `useReveal()` sees the deck context.
 *
 * Order (logo then footer) matches `footer_logo_html`. Cleanup on unmount keeps
 * test re-mounts and edit re-renders from accumulating nodes / the class.
 */
export function RevealChrome(props: { footerHtml?: string; logoHtml?: string }) {
    const reveal = useReveal();
    const { footerHtml, logoHtml } = props;
    useEffect(() => {
        if (!reveal) return;
        // `useReveal()`'s `RevealApi` resolves loosely (the package's
        // `../../dist/reveal` type import widens to `any`), so annotate the DOM
        // node explicitly — otherwise `Array.from(wrapper.children)` below
        // infers `unknown[]` under the SPA's stricter `tsc -b`.
        const el = reveal.getRevealElement?.() as HTMLElement | null;
        if (!el) return;

        const inserted: Element[] = [];
        const parts = [logoHtml, footerHtml].filter((s): s is string => !!s);
        if (parts.length > 0) {
            const wrapper = el.ownerDocument.createElement('div');
            wrapper.innerHTML = parts.join('\n');
            for (const node of Array.from(wrapper.children)) {
                node.setAttribute('data-q2-reveal-chrome', '1');
                el.appendChild(node);
                inserted.push(node);
            }
        }
        // `.reveal.has-logo` repositions the slide number (quarto-revealjs.scss);
        // the native scaffold sets it iff the logo slot is present.
        if (logoHtml) el.classList.add('has-logo');

        return () => {
            for (const node of inserted) node.remove();
            if (logoHtml) el.classList.remove('has-logo');
        };
    }, [reveal, footerHtml, logoHtml]);

    return null;
}

/**
 * Two-way slide-navigation bridge (bd-mwbsdmel). Mounted as a child of
 * `<Deck>` so `useReveal()` resolves the deck context — the same
 * escape-hatch pattern as `RevealChrome`. Renders nothing in the tree.
 *
 * - Outbound: subscribes to reveal's `slidechanged` and reports the new
 *   horizontal index through `onSlideChange`, so the host (the editor)
 *   can mirror in-deck navigation (arrows, controls).
 * - Inbound: registers an imperative `goTo(index)` via
 *   `registerSlideNavigator` (and clears it on unmount). The host calls
 *   it to move the deck — driven by the cursor→slide mapping — without an
 *   AST re-render. `goTo` no-ops when the deck is already on that slide,
 *   so an echoed host index doesn't fight a user mid-navigation.
 *
 * Mirrors only the horizontal index (`h`), matching the editor's
 * `useCursorToSlide` granularity and the retired hand-rolled deck's
 * `revealApi.slide(n)` / `getIndices().h` behaviour.
 */
export function RevealNavSync(props: {
    registerSlideNavigator?: (nav: ((index: number) => void) | null) => void;
    onSlideChange?: (slideIndex: number) => void;
}) {
    const reveal = useReveal();
    const { registerSlideNavigator, onSlideChange } = props;
    useEffect(() => {
        if (!reveal) return;
        // `useReveal()`'s RevealApi widens to `any`; pin the slice we use.
        const api = reveal as unknown as {
            slide?: (h: number, v?: number) => void;
            getIndices?: () => { h?: number; v?: number };
            on?: (type: string, cb: () => void) => void;
            off?: (type: string, cb: () => void) => void;
        };

        const goTo = (index: number) => {
            const cur = api.getIndices?.()?.h ?? 0;
            if (cur !== index) api.slide?.(index);
        };
        registerSlideNavigator?.(goTo);

        const handleSlideChanged = () => {
            onSlideChange?.(api.getIndices?.()?.h ?? 0);
        };
        api.on?.('slidechanged', handleSlideChanged);

        return () => {
            registerSlideNavigator?.(null);
            api.off?.('slidechanged', handleSlideChanged);
        };
    }, [reveal, registerSlideNavigator, onSlideChange]);

    return null;
}

/**
 * Broadcasts reveal's slide scale to interested chrome (the comment
 * bubbles counter-scale themselves with it). Published as a window
 * CustomEvent to avoid coupling; fires once the deck is ready and on
 * every reveal re-layout (viewport resize), and resets to 1 when the
 * deck unmounts. Renders null; must sit inside `<Deck>` so
 * `useReveal()` resolves.
 */
function RevealScaleSync() {
    const reveal = useReveal();
    useEffect(() => {
        if (!reveal) return;
        const api = reveal as unknown as {
            getScale?: () => number;
            on?: (type: string, cb: () => void) => void;
            off?: (type: string, cb: () => void) => void;
        };
        const publish = () => {
            window.dispatchEvent(
                new CustomEvent('q2-reveal-scale', { detail: api.getScale?.() ?? 1 }),
            );
        };
        publish();
        api.on?.('ready', publish);
        api.on?.('resize', publish);
        // Slide changes re-publish too: hidden sections never
        // mount/unmount bubbles, so this is what tells the bubble
        // layout to re-solve for the newly visible slide.
        api.on?.('slidechanged', publish);
        // Reveal also moves content at times we can't hook exhaustively
        // (fragments, async embeds, late layout settling) — while a
        // deck is live, republish on a slow tick so bubbles are
        // continuously re-positioned. Each publish triggers a bubble
        // reset-solve, which is a no-op (no re-renders) once settled.
        const tick = window.setInterval(publish, 250);
        return () => {
            window.clearInterval(tick);
            api.off?.('ready', publish);
            api.off?.('resize', publish);
            api.off?.('slidechanged', publish);
            window.dispatchEvent(new CustomEvent('q2-reveal-scale', { detail: 1 }));
        };
    }, [reveal]);
    return null;
}

export function RevealDeck(props: RevealDeckProps) {
    // Reveal sizes the deck from its container, so #root needs an
    // explicit height while a deck is mounted. The iframe HTML doesn't
    // style #root (a fixed 100vh there broke normal HTML pages), so
    // apply it here, scoped to the deck's lifetime.
    useLayoutEffect(() => {
        const root = document.getElementById('root');
        if (!root) return;
        root.style.width = '100%';
        root.style.height = '100vh';
        root.style.overflow = 'auto';
        return () => {
            root.style.width = '';
            root.style.height = '';
            root.style.overflow = '';
        };
    }, []);

    const chrome = revealChromeFromMeta(props.ast.meta);
    const slides = props.ast.blocks.map((block, i) => {
        if (isSectionDiv(block)) {
            return renderTopSection(block as DivBlock, i, props.onNavigateToDocument);
        }
        // Stray top-level content (no enclosing section) — wrap in a slide.
        return (
            <Slide key={i}>
                <SlideBody blocks={[block]} onNavigateToDocument={props.onNavigateToDocument} />
            </Slide>
        );
    });

    // Enable incremental-list handling for the whole deck; the global
    // `incremental: true` sets the starting state (`.incremental` /
    // `.nonincremental` Divs flip it per subtree). Mirrors the native writer.
    const globalIncremental = extractMetaBool(props.ast.meta?.incremental) === true;

    return (
        <RegistryContext.Provider value={{ registry: props.registry }}>
            <IncrementalContext.Provider value={{ enabled: true, incremental: globalIncremental }}>
                <Deck
                    config={{
                        // Match the render path's Quarto-1 opinionated defaults
                        // (see reveal_config_json in quarto-core) so preview and
                        // render look the same: top-aligned slides, no
                        // transition, 0.1 margin, linear nav, edge controls.
                        width: 1050,
                        height: 700,
                        margin: 0.1,
                        minScale: 0.2,
                        maxScale: 2.0,
                        controls: true,
                        progress: true,
                        center: false,
                        navigationMode: 'linear',
                        controlsLayout: 'edges',
                        controlsTutorial: false,
                        backgroundTransition: 'none',
                        // Preview re-renders on every edit; URL-hash navigation
                        // would fight that. Keep nav purely in-deck.
                        hash: false,
                        transition: 'none',
                    }}
                >
                    {slides}
                    {/* Deck-level footer/logo: injected into `.reveal` outside
                        `.slides` (renders null in-tree). Mirrors assemble.rs. */}
                    <RevealChrome footerHtml={chrome.footerHtml} logoHtml={chrome.logoHtml} />
                    {/* Cursor↔slide bridge (bd-mwbsdmel). Renders null;
                        no-op when the host wires no navigator (q2 preview SPA). */}
                    <RevealNavSync
                        registerSlideNavigator={props.registerSlideNavigator}
                        onSlideChange={props.onSlideChange}
                    />
                    {/* Publishes the slide scale for the comment bubbles. */}
                    <RevealScaleSync />
                </Deck>
            </IncrementalContext.Provider>
        </RegistryContext.Provider>
    );
}
