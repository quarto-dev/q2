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

import React, { useContext } from 'react';
import { Deck, Slide, Stack } from '@revealjs/react';
// Reveal CSS from the SAME vendored copy `q2 render` links — `resources/
// revealjs/` — in the SAME cascade order (reset → reveal → theme → quarto
// overrides), so render and preview cannot disagree on reveal styling
// (bd-ibqkf9ry). The `vendored_reveal_assets_match_npm_package` test keeps
// that copy byte-identical to the npm `reveal.js` `@revealjs/react` drives;
// importing the vendored files (not `reveal.js/*.css`) makes `resources/
// revealjs/` the single source of truth. `@revealjs/react` injects no CSS of
// its own — styling is entirely these imports.
import '../../../../resources/revealjs/reset.css';
import '../../../../resources/revealjs/reveal.css';
import '../../../../resources/revealjs/theme/white.css';
import '../../../../resources/revealjs/quarto-reveal.css';

import { extractMetaBool, Node, RegistryContext } from '../framework';
import type { BlockNode, DivBlock, FormatRegistry, PandocAST } from '../framework';
import { IncrementalContext } from './IncrementalContext';

interface RevealDeckProps {
    ast: PandocAST;
    registry: FormatRegistry;
    currentFilePath: string;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
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
 * Map one top-level section Div to a `<Slide>` (leaf) or `<Stack>` of
 * vertical `<Slide>`s (a section divider whose children are themselves
 * section Divs — the shape `RevealSlidesTransform` emits for `# Section`
 * dividers and their `## ` sub-slides).
 */
function renderTopSection(
    div: DivBlock,
    key: number,
    onNavigateToDocument?: (path: string, anchor: string | null) => void,
): React.ReactNode {
    const children = div.c[1];
    const verticalSlides = children.filter(isSectionDiv);

    if (verticalSlides.length > 0) {
        return (
            <Stack key={key}>
                {verticalSlides.map((slide, j) => (
                    <Slide key={j}>
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
        <Slide key={key}>
            <SlideBody
                blocks={children}
                sectionClasses={div.c[0][1]}
                onNavigateToDocument={onNavigateToDocument}
            />
        </Slide>
    );
}

export function RevealDeck(props: RevealDeckProps) {
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
                        width: 1050,
                        height: 700,
                        margin: 0.04,
                        minScale: 0.2,
                        maxScale: 2.0,
                        controls: true,
                        progress: true,
                        center: true,
                        // Preview re-renders on every edit; URL-hash navigation
                        // would fight that. Keep nav purely in-deck.
                        hash: false,
                        transition: 'slide',
                    }}
                >
                    {slides}
                </Deck>
            </IncrementalContext.Provider>
        </RegistryContext.Provider>
    );
}
