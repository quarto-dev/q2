import React, { useEffect, useRef } from 'react';
import { Deck, Slide } from '@revealjs/react';
// Base reveal CSS comes from the VENDORED copy (same files `q2 render`
// embeds), not npm `reveal.js/*.css` — bd-4b7f1hr7, same convergence
// as q2-preview's RevealDeck (bd-ibqkf9ry). The plugin CSS/JS below
// stays npm: plugins are editor chrome with no render-side
// counterpart. Note this surface deliberately does NOT import
// `reset.css`/`quarto-reveal.css` — it predates them and adding them
// would change the live editor's deck appearance.
import '../../../../resources/revealjs/reveal.css';
import '../../../../resources/revealjs/theme/white.css';
import 'katex/dist/katex.min.css';
import './revealjs-menu-override.css';
import RevealNotes from 'reveal.js/plugin/notes';
import RevealSearch from 'reveal.js/plugin/search';
import RevealZoom from 'reveal.js/plugin/zoom';
// @ts-ignore - no type definitions
import RevealMenuPlugin from 'reveal.js-menu/plugin.js';
const RevealMenu = RevealMenuPlugin.default || RevealMenuPlugin;
import { parseSlides, renderBlock, type Slide as PandocSlide } from './ReactAstSlideRenderer';
import type { PandocAST } from '@quarto/preview-renderer/framework';

interface RevealjsSlideRendererProps {
  astJson: string;
  currentFilePath: string;
  onNavigateToDocument?: (path: string, anchor: string | null) => void;
  currentSlide?: number;
  onSlideChange?: (slideIndex: number) => void;
}

/**
 * Render slide content for reveal.js Slide component.
 */
function renderSlideContent(
  slide: PandocSlide,
  currentFilePath: string,
  onNavigateToDocument?: (path: string, anchor: string | null) => void
): React.ReactNode {
  if (slide.type === 'title') {
    return (
      <>
        {slide.title && (
          <h1 style={{
            fontSize: '72px',
            margin: '0 0 40px 0',
            color: '#1a1a1a',
            fontWeight: 'bold'
          }}>
            {slide.title}
          </h1>
        )}
        {slide.author && (
          <p style={{
            fontSize: '36px',
            margin: 0,
            color: '#666'
          }}>
            {slide.author}
          </p>
        )}
      </>
    );
  }

  return (
    <>
      {slide.blocks.map((block, i) => renderBlock(block, i, currentFilePath, onNavigateToDocument))}
    </>
  );
}

/**
 * Component that renders Pandoc AST as React elements for slides using reveal.js
 */
export function RevealjsSlideAst({ astJson, currentFilePath, onNavigateToDocument, currentSlide: controlledSlide, onSlideChange }: RevealjsSlideRendererProps) {
  const deckRef = useRef<any>(null);

  let ast: PandocAST;

  try {
    ast = JSON.parse(astJson);
  } catch (err) {
    return (
      <div className="error" style={{ padding: '20px', color: 'red' }}>
        Failed to parse AST: {err instanceof Error ? err.message : String(err)}
      </div>
    );
  }

  const slides = parseSlides(ast);

  useEffect(() => {
    if (controlledSlide !== undefined && deckRef.current) {
      const revealApi = deckRef.current;
      const currentIndices = revealApi.getIndices();
      if (currentIndices.h !== controlledSlide) {
        revealApi.slide(controlledSlide);
      }
    }
  }, [controlledSlide]);

  const handleSlideChange = (event: any) => {
    if (onSlideChange) {
      const indices = event.currentSlide ?
        deckRef.current?.getIndices() :
        { h: 0, v: 0 };
      onSlideChange(indices?.h ?? 0);
    }
  };

  return (
    <div
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        background: 'white'
      }}
      className="revealjs-container"
    >
      <Deck
        deckRef={deckRef}
        config={{
          // Match the render path's Quarto-1 opinionated defaults (see
          // reveal_config_json in quarto-core): top-aligned slides, no
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
          hash: false,
          transition: 'none',
          backgroundTransition: 'none',
          // probably need to re-enable this on-focus or something
          // but it was making it so I can't type!
          keyboard: false,
          // @ts-ignore - menu config not in base types
          menu: {
            path: '/reveal-menu/',
            side: 'left',
            width: 'normal',
            numbers: false,
            titleSelector: 'h1, h2, h3, h4, h5, h6',
            useTextContentForMissingTitles: true,
            hideMissingTitles: false,
            markers: true,
            custom: false,
            themes: false,
            transitions: false,
            openButton: true,
            openSlideNumber: false,
            keyboard: true,
            loadIcons: false,
          },
        }}
        plugins={[RevealNotes, RevealSearch, RevealZoom, RevealMenu]}
        onSlideChange={handleSlideChange}
      >
        {slides.map((slide, index) => (
          <Slide key={index}>
            {renderSlideContent(slide, currentFilePath, onNavigateToDocument)}
          </Slide>
        ))}
      </Deck>
    </div>
  );
}
