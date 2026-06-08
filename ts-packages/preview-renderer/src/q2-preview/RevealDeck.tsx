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

import React from 'react';
import { Deck, Slide, Stack } from '@revealjs/react';
import 'reveal.js/reveal.css';
import 'reveal.js/theme/white.css';

import { Node, RegistryContext } from '../framework';
import type { BlockNode, DivBlock, FormatRegistry, PandocAST } from '../framework';

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

/** Render a slide's content blocks via the shared previewRegistry. */
function SlideBody(props: {
    blocks: BlockNode[];
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
}) {
    return (
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
                            onNavigateToDocument={onNavigateToDocument}
                        />
                    </Slide>
                ))}
            </Stack>
        );
    }

    return (
        <Slide key={key}>
            <SlideBody blocks={children} onNavigateToDocument={onNavigateToDocument} />
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

    return (
        <RegistryContext.Provider value={{ registry: props.registry }}>
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
        </RegistryContext.Provider>
    );
}
