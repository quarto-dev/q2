/**
 * Entry point for the q2-preview renderer iframe.
 *
 * Loaded by `/q2-preview.html` and handles the postMessage protocol
 * with the parent (`Q2PreviewIframe`). Mounts the framework's `<Ast>`
 * with `previewRegistry` as the format-side defaults, layered with any
 * user-TSX overrides loaded via `LOAD_CUSTOM_COMPONENTS`.
 *
 * Two structural differences from `q2-debug/entry.tsx`:
 *
 *  1. `UPDATE_THEME` is handled at module top (not inside any React
 *     component). The handler imperatively manages a single
 *     `<link rel="stylesheet" data-q2-theme>` element in `document.head`
 *     — pure DOM, no React state. This avoids a race: if the listener
 *     lived in `PreviewRoot`'s `useEffect`, it would only attach after
 *     React commits the mount triggered by the first `UPDATE_AST`. The
 *     parent posts theme + AST from sibling `useEffect`s on the same
 *     `iframeReady` transition; if theme posts first the message would
 *     be dropped.
 *
 *  2. `__Q2_PREVIEW_RENDERER__` is set at module top (not inside
 *     `loadCustomComponents`). This makes the renderer surface
 *     importable in tests without firing `LOAD_CUSTOM_COMPONENTS`
 *     setup messages — the framework-primitive parity test (Plan 2A
 *     item 14) relies on this.
 */

import { createRoot } from 'react-dom/client';
import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import katex from 'katex';
import 'katex/dist/katex.min.css';
// Phase F.1 (bd-kw93.14): Bootstrap 5 bundled JS, vendored at the
// repo's `resources/js/bootstrap/`. We embed it as raw text and
// inject as an inline `<script>` at module top so Bootstrap's
// data-API click delegates are attached before any chrome HTML
// arrives. The vendored copy is paired-versioned with the SCSS at
// `resources/scss/bootstrap/` (5.3.1 today). See the Phase F plan
// for the rationale: `BootstrapJsStage` could ride the WASM
// pipeline (math_js.rs precedent), but q2-preview also excludes
// `ApplyTemplateStage` so the JS would never get a `<script>` tag.
// Static iframe injection is the cleaner separation — chrome JS is
// iframe-template responsibility, not document-render responsibility.
// `?raw` is typed via `src/global.d.ts` (Vite's `?raw` suffix
// returns a string at build time).
import bootstrapJsSrc from '../../../../resources/js/bootstrap/bootstrap.bundle.min.js?raw';

(() => {
    const existing = document.head.querySelector('script[data-q2-bootstrap]');
    if (existing) return;
    const tag = document.createElement('script');
    tag.setAttribute('data-q2-bootstrap', '1');
    tag.textContent = bootstrapJsSrc;
    document.head.appendChild(tag);
})();

import {
    Ast,
    Node,
    renderChildren,
    renderNode,
    rewrapCustomNodes,
    extractMetaString,
    extractMetaBool,
    extractMetaStringList,
    inlinesToPlainText,
    blocksToPlainText,
    AttributionLookupContext,
    useNodeAttribution,
    CurrentActorContext,
    useCurrentActor,
} from '../framework';
import type { FormatRegistry, NoteInline, PandocAST } from '../framework';
import type { BlockNode } from '../framework/types';
import type { PreviewNodeEditPayload } from '../types/diagnostic';
import { Block, Inline, previewRegistry, PreviewContext, usePreviewEdit } from '.';
import { buildSourceIndex, serializeSourceEntry } from './sourceIndex';
import type { ResolvedSource } from './sourceIndex';
import { tileForAnchorR0, findReanchorCandidate } from './lockedTiles';
import { AssetManifestContext } from './AssetManifestContext';
import { NoteNumberingContext } from './NoteNumberingContext';
import { renderSlot } from './utils';
import { PreviewTitleBlock } from './custom/PreviewTitleBlock';
import { RevealDeck } from './RevealDeck';
import {
    buildCustomRegistry,
    type ComponentExports,
} from '../utils/customRegistry';
import { installLinkHandlers } from '../utils/iframeLinkHandlers';

// Set the renderer-surface global at module top. Importing this module
// is sufficient to populate `window.__Q2_PREVIEW_RENDERER__`. The
// explicit-object form (rather than `{ ...framework, ...preview }`
// spread) keeps framework internals (`renderChildrenRegistry`,
// `RegistryContext`) off the global and locks the public surface.
//
// Plan 2C exposes `renderSlot` so user TSX overrides of CustomNode
// components can recurse into named slots (Callout's title/content,
// FloatRefTarget's caption_long/caption_short, ...) without
// reimplementing the per-slot setLocalAst plumbing.
//
// Plan 2D (6.0c.1) exposes the framework-tier meta and plain-text
// helpers so a user TSX override of `__title_block__` can coerce
// `ast.meta` values without re-implementing the Pandoc-AST walks.
// Plan 2D (7.3.1) exposes `PreviewTitleBlock` so a user override
// can compose the built-in chrome (e.g. wrap it and add a DOI line)
// instead of re-implementing it from scratch.
//
// Reactji-authorship demo (2026-05-25) exposes `useNodeAttribution`
// + `AttributionLookupContext` (Plan 5 wire surfaces) so user TSX can
// resolve per-node authorship from the attribution lookup map that
// `framework/Ast.tsx` provides. `useCurrentActor` is added below in
// the `CurrentActorContext` block once the iframe payload carries the
// actor id.
(window as any).__Q2_PREVIEW_RENDERER__ = {
    renderChildren,
    renderNode,
    renderSlot,
    Node,
    Block,
    Inline,
    previewRegistry,
    extractMetaString,
    extractMetaBool,
    extractMetaStringList,
    inlinesToPlainText,
    blocksToPlainText,
    PreviewTitleBlock,
    usePreviewEdit,
    useNodeAttribution,
    AttributionLookupContext,
    useCurrentActor,
    CurrentActorContext,
};

let root: ReturnType<typeof createRoot> | null = null;
let customRegistry: Record<string, React.ComponentType<any>> = {};
let componentsLoading = false;

interface UpdateAstPayload {
    astJson: string;
    currentFilePath: string;
    /**
     * Pre-pipeline (untransformed) AST JSON shipped in lockstep with
     * `astJson` + `renderedContent` (same compound-state generation).
     * Received by `PreviewRoot` to build `sourceIndex` for the
     * structural editability gate (Plan 2a).
     */
    untransformedAstJson?: string | null;
    /**
     * Manifest of `{ origPath → blobUrl }` produced by the parent's
     * `assetWalker.ts`. Forwarded into `AssetManifestContext` so
     * `<Image>` can resolve project-relative URLs to blob URLs without
     * any VFS access in the iframe. External URLs (`https?:`, `data:`,
     * `//`) are not in the manifest — `lookupAssetUrl` passes them
     * through.
     */
    assetManifest?: Record<string, string>;
    /**
     * Phase F.1 (bd-kw93.14): project file paths (no leading slash)
     * forwarded into the iframe link handler so artifact-rooted
     * `.html` clicks can be reverse-mapped to source `.qmd`. Used
     * for documentation today — `installLinkHandlers` always
     * intercepts artifact-rooted hrefs.
     */
    projectFilePaths?: readonly string[];
    /**
     * Phase F.1 (bd-kw93.14): anchor (without `#`) to scroll into
     * view after React commits this AST. Set on cross-page nav and
     * back/forward; null/undefined means "no pending scroll".
     *
     * Paired with `pendingAnchorEpoch`: the iframe scrolls only when
     * the epoch ticks past what it last saw. Re-renders from edits
     * carry the same epoch so they don't trigger a re-scroll.
     */
    pendingAnchor?: string | null;
    pendingAnchorEpoch?: number;
    renderedContent?: string;
    /**
     * Reactji-authorship demo (2026-05-25 plan): current viewer's
     * Automerge actor id. Provided via `CurrentActorContext` to user
     * TSX so `useCurrentActor()` can drive `actor === me` checks.
     */
    currentActor?: string | null;
    /**
     * Globally disable the edit surface (bd-ov4gqk3m). Forwarded into
     * `PreviewContext.editingDisabled`; set by read-only hosts
     * (`q2 preview` without `--allow-edit`). Absent/false ⇒ editable.
     */
    editingDisabled?: boolean;
}

// Module-top message handler. Registered before `IFRAME_READY` is
// posted so the parent's `UPDATE_THEME` (which can fire immediately
// after `IFRAME_READY` from a sibling `useEffect`) is never dropped.
window.addEventListener('message', async (event) => {
    if (event.data.type === 'LOAD_CUSTOM_COMPONENTS') {
        componentsLoading = true;
        await loadCustomComponents(event.data.componentsCode);
        componentsLoading = false;
    } else if (event.data.type === 'UPDATE_AST') {
        if (componentsLoading) {
            await new Promise((resolve) => {
                const check = setInterval(() => {
                    if (!componentsLoading) {
                        clearInterval(check);
                        resolve(undefined);
                    }
                }, 50);
            });
        }
        updateAst(event.data.payload);
    } else if (event.data.type === 'UPDATE_THEME') {
        lastThemeCssUrl = event.data.cssUrl;
        reconcileThemeLink();
    }
});

/**
 * Imperatively manage a single `<link data-q2-theme>` in
 * `document.head`. The `data-q2-theme` attribute is the idempotency
 * selector — repeated `applyTheme` calls with the same URL just
 * `setAttribute('href', sameUrl)` in place, no element duplication.
 *
 * `cssUrl === null` removes the element (explicit clear). The pre-
 * first-message state has no `<link data-q2-theme>` element at all
 * and is distinct from a received `cssUrl: null` (which removes any
 * prior element).
 */
function applyTheme(cssUrl: string | null): void {
    let link = document.head.querySelector<HTMLLinkElement>(
        'link[data-q2-theme]',
    );
    if (cssUrl === null) {
        if (link) link.remove();
        return;
    }
    if (!link) {
        link = document.createElement('link');
        link.setAttribute('rel', 'stylesheet');
        link.setAttribute('data-q2-theme', '1');
        document.head.appendChild(link);
    }
    link.setAttribute('href', cssUrl);
}

// bd-y259zb57: the `UPDATE_THEME` channel carries the active document's
// *compiled theme*. For an HTML page that's Bootstrap; for a `format: revealjs`
// deck (`q2-slides`) it's the compiled Quarto reveal theme, delivered through
// the SAME `css:theme:<fp>` → styles.css transport (see CompileThemeCssStage's
// reveal branch). Both must be applied as the `<link data-q2-theme>` so preview
// matches render.
//
// (Previously this suppressed the theme link on slides, because the preview
// only ever produced Bootstrap CSS — never the reveal theme — and reveal decks
// fell back to a hard-coded stock `white.css` import in `RevealDeck`. That was
// the centered/uppercase render↔preview divergence this strand fixes.)
let lastThemeCssUrl: string | null = null;

function reconcileThemeLink(): void {
    applyTheme(lastThemeCssUrl);
}

/**
 * Phase F.1 (bd-kw93.14): scroll the iframe document to the element
 * with the given id. No-op when the element doesn't exist (a stale
 * back/forward to a section that's been removed shouldn't blow up).
 */
/**
 * Scroll the iframe document to the element with the given id.
 * Returns true when the element was found and scrolled, false when
 * it wasn't yet in the DOM (the caller uses this to decide whether
 * the epoch has been "consumed" — see the anchor-scroll useEffect).
 */
function scrollToAnchorInDocument(anchor: string): boolean {
    const el = document.getElementById(anchor);
    if (!el) return false;
    el.scrollIntoView({ behavior: 'instant', block: 'start' });
    return true;
}

async function loadCustomComponents(componentsCode: Record<string, string>) {
    // Tied to dynamic user-TSX imports — materialize React and katex
    // when LOAD_CUSTOM_COMPONENTS arrives. q2-preview does NOT set
    // `window.RevealReact` (q2-debug-only — slide-demo template).
    (window as any).React = React;
    (window as any).katex = katex;

    const loadedModules: ComponentExports[] = [];
    for (const [componentName, code] of Object.entries(componentsCode)) {
        try {
            const blob = new Blob([code], { type: 'application/javascript' });
            const url = URL.createObjectURL(blob);
            try {
                const module = await import(/* @vite-ignore */ url);
                loadedModules.push(module as ComponentExports);
                console.log(
                    `[Q2PreviewIframe] Loaded custom component: ${componentName}`,
                );
            } finally {
                URL.revokeObjectURL(url);
            }
        } catch (err) {
            console.error(
                `[Q2PreviewIframe] Failed to load custom component ${componentName}:`,
                err,
            );
        }
    }

    customRegistry = buildCustomRegistry(loadedModules);
}

interface PreviewRootProps {
    astJson: string;
    currentFilePath: string;
    /** Forwarded from `UPDATE_AST` payload; default is empty manifest. */
    assetManifest: Record<string, string>;
    /** Phase F.1: forwarded into `installLinkHandlers`. */
    projectFilePaths?: readonly string[];
    /** Phase F.1: post-render scroll target (no leading `#`). */
    pendingAnchor?: string | null;
    /** Phase F.1: monotonic epoch — scroll fires when this advances. */
    pendingAnchorEpoch?: number;
    /**
     * Reactji-authorship demo (2026-05-25 plan): viewer's Automerge
     * actor id, provided via `CurrentActorContext` to user TSX.
     */
    currentActor?: string | null;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
    renderedContent?: string;
    /** Pre-pipeline AST JSON for the structural editability gate (Plan 2a). */
    untransformedAstJson?: string | null;
    /** Globally disable the edit surface (bd-ov4gqk3m). */
    editingDisabled?: boolean;
}

/**
 * Walk the parsed AST for `Note` inlines and assign each a sequential
 * number by document order. Lookup keyed by object identity — the
 * framework's walker-purity contract preserves Note references through
 * `unwrapCustomNodes`, so the WeakMap built here works on both pre-
 * and post-unwrap AST shapes.
 *
 * Walks pre-unwrap (over the JSON.parse output) so the same parsed
 * object can be handed to <Ast> via the discriminated input — avoids
 * a double-parse.
 *
 * Descends both `c` fields and CustomNode wrapper slot children
 * (`c[1][i].c[1]`) so notes nested inside callout / theorem bodies
 * are reached.
 */
function walkForNoteNumbers(ast: PandocAST): WeakMap<NoteInline, number> {
    const map = new WeakMap<NoteInline, number>();
    let counter = 0;
    function visit(value: unknown) {
        if (!value || typeof value !== 'object') return;
        if (Array.isArray(value)) {
            for (const v of value) visit(v);
            return;
        }
        const obj = value as { t?: unknown; c?: unknown };
        if (obj.t === 'Note') {
            counter += 1;
            map.set(value as NoteInline, counter);
        }
        if ('c' in obj) visit(obj.c);
    }
    visit(ast.blocks);
    return map;
}

function PreviewRoot(props: PreviewRootProps) {
    const [editTarget, setEditTargetRaw] = useState<{
        anchorR0: number;
        anchorR1: number;
        anchorSlice: string;
        contentHeight: number;
        boxStyle: Record<string, string>;
    } | null>(null);
    // Root-held ref for the in-flight edit draft. Seeded with anchorSlice at
    // activation; reset to null on close. Referentially stable → no extra
    // re-renders from draft changes.
    const editDraftRef = useRef<string | null>(null);
    // Ref pointing to the inner wrapper div of the currently active edit
    // region (set by renderMeasuredEdit in dispatchers.tsx). Used by
    // useBlockEditHover's onPointerUp to suppress the parent-climb bug:
    // a click inside this region must not activate any other surface.
    const activeEditRegionRef = useRef<HTMLDivElement | null>(null);
    const setEditTarget = useCallback((target: typeof editTarget | null) => {
        if (target !== null) {
            // Draft is seeded at the fresh-open site (activate / future nav-reland),
            // never here, so a P2.3b self-heal re-anchor via setEditTarget preserves
            // the in-flight draft without clobbering it.
            setEditTargetRaw(target);
        } else {
            editDraftRef.current = null;
            setEditTargetRaw(null);
            // P2.3b: drop-focus is handled in the self-heal layout effect below.
            // Plain-commit focus-restoration is deferred to P2.4 (pendingLanding).
        }
    }, []);

    // ---------------------------------------------------------------------------
    // P2.3b: self-heal + hidden-surface drop
    //
    // Runs after every external re-render (keyed on render inputs, NOT on
    // editTarget — keying on editTarget would fire this on fresh activation and
    // corrupt the open). editTarget is read via ref so the effect always sees
    // the current value without becoming stale.
    //
    // The effect re-anchors the open editor when the active block shifted but is
    // content-verified, or drops it when the block was edited under the user or
    // the tile became hidden.
    // ---------------------------------------------------------------------------

    // Keep editTarget in a ref so the layout effect reads it without depending on it.
    const editTargetRef = useRef<typeof editTarget>(editTarget);
    editTargetRef.current = editTarget;

    // previewHostRef: a ref to the wrapper div that scopes tile queries
    // (tileForAnchorR0) to the preview document tree. The iframe already
    // has a single document so `document.body` would also work, but using
    // a ref keeps the DOM queries scoped below the React root and avoids
    // querying any elements injected outside the React tree (e.g. the
    // Bootstrap script tag in document.head).
    const previewHostRef = useRef<HTMLDivElement | null>(null);

    // Keep pool in a ref for the same reason — pool changes with astJson but we
    // don't want pool in the effect deps (that would fire on every render, not
    // just epoch-ticks). Pool is re-derived from astJson in the useMemo below.
    const poolRef = useRef<unknown[]>([]);

    // Self-heal layout effect. Fires post-DOM / pre-paint so re-anchoring is
    // invisible (no flicker). Keyed on the render inputs that signal an external
    // re-render: [astJson, renderedContent, untransformedAstJson].
    useLayoutEffect(() => {
        const et = editTargetRef.current;
        if (et === null) return;  // no open editor — nothing to do

        const currentPool = poolRef.current;
        const currentContent = props.renderedContent ?? '';

        // Step 1: find a re-anchor candidate using content-verification.
        const cand = findReanchorCandidate(currentPool, currentContent, et.anchorR0, et.anchorSlice);

        if (cand) {
            // Re-anchor: update anchorR0/anchorR1; draft (editDraftRef) is untouched.
            // anchorSlice is unchanged (content-verified).
            const reanchored = { ...et, anchorR0: cand.r0, anchorR1: cand.r1 };
            setEditTargetRaw(reanchored);

            // Step 2: check that the re-anchored tile is EXACTLY visible. A re-render
            // can move the block into a collapsed region — drop if so.
            // Must use exactOnly:true to test visibility of THIS specific tile, not
            // the nearest subsequent tile. Without exactOnly, a later visible tile
            // would return non-null and the hidden-drop would be silently missed.
            if (previewHostRef.current) {
                const tile = tileForAnchorR0(previewHostRef.current, currentPool, cand.r0, { exactOnly: true });
                if (tile === null) {
                    // Re-anchored tile is hidden — drop
                    editDraftRef.current = null;
                    setEditTargetRaw(null);
                    // drop-focus: no tile to focus at/after (everything hidden here)
                }
            }
        } else {
            // Drop — content mismatch or no candidate at/after anchorR0.
            // The block was edited under the user, or all blocks after anchorR0 are gone.
            editDraftRef.current = null;
            setEditTargetRaw(null);

            // Drop-focus: best-effort focus on the nearest visible tile at/after anchorR0.
            // This positions roving-tabindex without reopening an editor.
            // Note: a "don't steal focus if a new edit started" guard belongs to P2.4's
            // async reland path (pendingLanding), not this synchronous layout-effect path —
            // setEditTargetRaw(null) is async so editTargetRef.current still holds the
            // pre-drop non-null value here, making any such guard ineffective anyway.
            if (previewHostRef.current) {
                const tile = tileForAnchorR0(previewHostRef.current, currentPool, et.anchorR0);
                if (tile) {
                    (tile as HTMLElement).focus?.();
                }
            }
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [props.astJson, props.renderedContent, props.untransformedAstJson]);

    // Refs so the link-handler closure (installed once at mount)
    // sees the *latest* currentFilePath / projectFilePaths instead
    // of the values captured at first install. Without these, every
    // cross-page nav would still resolve relative links against the
    // boot-time activeFile and the same-doc fast-path would never
    // recognize the user's current page.
    const currentFilePathRef = useRef(props.currentFilePath);
    currentFilePathRef.current = props.currentFilePath;
    const onNavigateRef = useRef(props.onNavigateToDocument);
    onNavigateRef.current = props.onNavigateToDocument;

    // Install link handlers once per mount. The iframe remounts on
    // every document switch (q2-debug's existing behavior — see
    // `ReactPreview.tsx` previewState reset), so closures captured
    // here cannot go stale within a single mount.
    useEffect(() => {
        installLinkHandlers(document, {
            currentFilePath: props.currentFilePath,
            projectFilePaths: props.projectFilePaths,
            onQmdLinkClick: (arg) => {
                if ('path' in arg) {
                    // Phase F.1 (bd-kw93.14): same-doc cross-page
                    // navigation (path === currentFilePath) is just
                    // an anchor scroll. Handle locally to avoid a
                    // pointless round-trip + re-render. Use refs so
                    // we read the *latest* activeFile / callback,
                    // not the boot-time values.
                    if (arg.path === currentFilePathRef.current) {
                        if (arg.anchor) {
                            scrollToAnchorInDocument(arg.anchor);
                        }
                    } else {
                        onNavigateRef.current?.(arg.path, arg.anchor);
                    }
                } else {
                    // Pure `#frag` click — same-doc anchor scroll.
                    scrollToAnchorInDocument(arg.anchor);
                }
            },
        });
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // Phase F.1 (bd-kw93.14): scroll to the cross-page anchor after
    // React commits the new AST. Triggered when the epoch advances —
    // each user-driven nav event in `PreviewApp` bumps the epoch,
    // edits don't.
    //
    // The race we have to guard against: the parent posts UPDATE_AST
    // with the new pendingAnchor *before* the new astJson is ready
    // (the React render effect runs async after activeFile changes).
    // So this effect can fire once with the new anchor + the OLD
    // astJson — `getElementById('intro')` returns null because that
    // doc doesn't have it yet. Only mark the epoch consumed when the
    // element was actually found, so the next pass (with the new
    // astJson) tries again. requestAnimationFrame is defensive
    // against a Firefox layout-thrash race where scrollIntoView
    // fires before the post-commit layout pass.
    const lastScrolledEpochRef = useRef<number>(0);
    useEffect(() => {
        const epoch = props.pendingAnchorEpoch ?? 0;
        if (!props.pendingAnchor || epoch === 0) return;
        if (epoch === lastScrolledEpochRef.current) return;
        const anchor = props.pendingAnchor;
        const raf = requestAnimationFrame(() => {
            if (scrollToAnchorInDocument(anchor)) {
                lastScrolledEpochRef.current = epoch;
            }
        });
        return () => cancelAnimationFrame(raf);
    }, [props.pendingAnchor, props.pendingAnchorEpoch, props.astJson]);

    // Single merge site: built-in preview leaves + CustomNode
    // components (under their type_name keys) + the user's TSX
    // overrides. Pandoc tags and CustomNode type_names share one
    // namespace by project policy (registry.test.ts locks it), so the
    // same `customRegistry` spread covers overrides of either kind.
    const mergedPreviewRegistry: FormatRegistry = {
        ...previewRegistry,
        ...customRegistry,
    } as FormatRegistry;

    // Pre-parse the AST and run the Note-numbering walk in one
    // useMemo. The parsed AST is then passed to <Ast> via the
    // discriminated input — no second JSON.parse. On parse failure,
    // hand <Ast> the original astJson so its existing error pane
    // surfaces the message.
    const { parsed, noteNumbers, pool } = useMemo(() => {
        try {
            const raw = JSON.parse(props.astJson) as PandocAST & {
                astContext?: { p?: unknown[] };
            };
            return {
                parsed: raw as PandocAST,
                noteNumbers: walkForNoteNumbers(raw as PandocAST),
                pool: raw.astContext?.p ?? ([] as unknown[]),
            };
        } catch {
            return { parsed: null, noteNumbers: new WeakMap<NoteInline, number>(), pool: [] as unknown[] };
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [props.astJson]);

    // Keep poolRef in sync with the latest pool value. Updated in render body
    // (not in an effect) so the self-heal layout effect always reads the pool
    // from the current render cycle when it fires.
    poolRef.current = pool;

    const astProps = parsed
        ? { ast: parsed }
        : { astJson: props.astJson };

    // `format: revealjs` previews as the `q2-slides` pseudo-format
    // (MetadataMergeStage writes the target format into `meta.format`). When
    // the AST is a slide deck, render it with the reveal shell
    // (`RevealDeck`) instead of the flowing html mirror. Slide *content*
    // still renders via the same `previewRegistry`.
    const previewFormat = parsed ? extractMetaString(parsed.meta?.format) : undefined;
    const isSlides = previewFormat === 'q2-slides' || previewFormat === 'revealjs';

    // Build SourceInfo-value index from the untransformed AST (Plan 2a).
    // Keyed by serializeSourceEntry(entry); used by resolveSource below.
    const sourceIndex = useMemo(
        () => buildSourceIndex(props.untransformedAstJson),
        // eslint-disable-next-line react-hooks/exhaustive-deps
        [props.untransformedAstJson],
    );

    // resolveSource: look up a transformed block's untransformed counterpart
    // via the SourceInfo value (Plan 2a). Returns null when the block is
    // Generated, from an included file, or has no source counterpart.
    const resolveSource = useCallback(
        (node: BlockNode): ResolvedSource | null => {
            if (!sourceIndex || !pool || pool.length === 0) return null;
            const s = (node as unknown as { s?: unknown }).s;
            if (s === undefined) return null;
            const sourceEntry = pool[Number(s)] as { t: 0; r: [number, number]; d: number } | undefined;
            if (!sourceEntry || sourceEntry.t !== 0) return null;
            const key = serializeSourceEntry(sourceEntry);
            const indexEntry = sourceIndex.get(key);
            if (!indexEntry) return null;
            return {
                sourceNode: indexEntry.sourceNode,
                reachabilityClass: indexEntry.reachabilityClass,
                sourceEntry,
            };
        },
        [pool, sourceIndex],
    );

    // commitTextEdit: send a text-channel PreviewNodeEditPayload (Plan 2b).
    // `destinationSourceInfoJson` is already resolved by the caller (Para/Header
    // pass JSON.stringify(resolved.sourceEntry) directly).
    const commitTextEdit = (destinationSourceInfoJson: string, newText: string) => {
        const payload: PreviewNodeEditPayload = {
            __isPreviewNodeEdit: true,
            channel: 'text',
            destinationSourceInfoJson,
            newText,
        };
        props.setAst(payload as unknown as PandocAST);
    };

    // commitSubtreeEdit: send a subtree-channel PreviewNodeEditPayload (Plan 2b).
    // Used by render-component authors via usePreviewEdit().
    //
    // apply_node_edit (Rust) expects `modifiedSubtreeJson` to be a full Pandoc
    // document (`{"pandoc-api-version":…,"meta":{},"blocks":[…]}`), not a bare
    // block.  Wrap the single block here so callers can work with BlockNode
    // values directly without knowing the wire-format requirement.
    const commitSubtreeEdit = (destinationSourceInfoJson: string, modifiedBlock: BlockNode) => {
        // Strip all `s` (block/inline SourceInfo pool-index) and `a`
        // (AttrSourceInfo object whose id/classes/kvs are also pool indices)
        // fields from the block tree before sending.  The source block carries
        // references into the current render's pool; `apply_node_edit` (Rust)
        // rejects them as `InvalidSourceInfoRef` when deserialising the
        // replacement subtree, because that doc carries no pool.
        const stripped = JSON.parse(JSON.stringify(modifiedBlock, (key, value) =>
            key === 's' || key === 'a' ? undefined : value,
        )) as BlockNode;
        const wrappedDoc = {
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [stripped],
        };
        const modifiedSubtreeJson = JSON.stringify(wrappedDoc);
        const payload: PreviewNodeEditPayload = {
            __isPreviewNodeEdit: true,
            channel: 'subtree',
            destinationSourceInfoJson,
            modifiedSubtreeJson,
        };
        props.setAst(payload as unknown as PandocAST);
    };

    return (
        <PreviewContext.Provider
            value={{
                currentFilePath: props.currentFilePath,
                pool,
                commitTextEdit,
                commitSubtreeEdit,
                content: props.renderedContent,
                editTarget,
                setEditTarget,
                editDraftRef,
                activeEditRegionRef,
                sourceIndex,
                resolveSource,
                editingDisabled: props.editingDisabled,
            }}
        >
            {/* previewHostRef scopes tile queries (tileForAnchorR0) to the
                preview document so the self-heal effect does not accidentally
                query tiles from other parts of the page. The div is a
                transparent pass-through with no visual effect. */}
            <div ref={previewHostRef} style={{ display: 'contents' }}>
                <CurrentActorContext.Provider value={props.currentActor ?? null}>
                    <AssetManifestContext.Provider value={props.assetManifest}>
                        <NoteNumberingContext.Provider value={noteNumbers}>
                            {isSlides && parsed ? (
                                <RevealDeck
                                    ast={parsed}
                                    registry={mergedPreviewRegistry}
                                    currentFilePath={props.currentFilePath}
                                    onNavigateToDocument={props.onNavigateToDocument}
                                />
                            ) : (
                                <Ast
                                    {...astProps}
                                    currentFilePath={props.currentFilePath}
                                    onNavigateToDocument={props.onNavigateToDocument}
                                    setAst={props.setAst}
                                    registry={mergedPreviewRegistry}
                                />
                            )}
                        </NoteNumberingContext.Provider>
                    </AssetManifestContext.Provider>
                </CurrentActorContext.Provider>
            </div>
        </PreviewContext.Provider>
    );
}

function updateAst(payload: UpdateAstPayload) {
    const {
        astJson,
        currentFilePath,
        assetManifest,
        projectFilePaths,
        pendingAnchor,
        pendingAnchorEpoch,
        renderedContent,
        untransformedAstJson,
        currentActor,
        editingDisabled,
    } = payload;
    const rootElement = document.getElementById('root');
    if (!rootElement) {
        console.error('Root element not found');
        return;
    }

    try {
        if (!root) {
            root = createRoot(rootElement);
        }
        root.render(
            <PreviewRoot
                astJson={astJson}
                currentFilePath={currentFilePath}
                assetManifest={assetManifest ?? {}}
                projectFilePaths={projectFilePaths}
                pendingAnchor={pendingAnchor}
                pendingAnchorEpoch={pendingAnchorEpoch}
                renderedContent={renderedContent}
                untransformedAstJson={untransformedAstJson}
                currentActor={currentActor ?? null}
                editingDisabled={editingDisabled}
                onNavigateToDocument={(path, anchor) => {
                    window.parent.postMessage(
                        { type: 'NAVIGATE_TO_DOCUMENT', path, anchor },
                        '*',
                    );
                }}
                setAst={(newAst) => {
                    // PreviewNodeEditPayload: pass through directly without
                    // rewrapping custom nodes (it's not a PandocAST).
                    if ((newAst as unknown as { __isPreviewNodeEdit?: boolean }).__isPreviewNodeEdit) {
                        window.parent.postMessage({ type: 'SET_AST', ast: newAst }, '*');
                        return;
                    }
                    // Rewrap JS-native CustomNodes back to wire-format
                    // Div/Span before posting. Keeps the parent-side
                    // (and any downstream consumer reading `data-custom-*`
                    // attributes) on the wire shape it expects.
                    window.parent.postMessage(
                        { type: 'SET_AST', ast: rewrapCustomNodes(newAst) },
                        '*',
                    );
                }}
            />,
        );
    } catch (err) {
        console.error('Failed to render AST:', err);
        rootElement.innerHTML = `
      <div style="padding: 20px; color: red;">
        <strong>Render Error:</strong>
        <pre>${err instanceof Error ? err.message : String(err)}</pre>
      </div>
    `;
    }
}

// Signal that the iframe is ready to receive messages. Posted AFTER
// the module-top message listener is registered so no UPDATE_THEME
// or UPDATE_AST is dropped.
window.parent.postMessage({ type: 'IFRAME_READY' }, '*');
