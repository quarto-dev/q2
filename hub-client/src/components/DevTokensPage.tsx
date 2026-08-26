/**
 * Dev-only token gallery (#/dev/tokens): renders every design-system scale
 * token (spacing, radii, shadows, z-layers, type, motion, focus ring) so
 * the scales are inspectable in both themes and covered by the Playwright
 * visual baselines. Inline styles keep this page out of the lint:css
 * scope; every value references a token.
 *
 * Phase 0 deliverable of the UI/UX modernization plan (bd-5nm6v8bl).
 */

import React from 'react';

const SPACES = [1, 2, 3, 4, 5, 6, 7, 8];
const RADII = ['sm', 'md', 'lg'];
const SHADOWS = [1, 2, 3];
const Z_LAYERS = [
  'base',
  'raised',
  'sticky',
  'header',
  'dropdown',
  'overlay',
  'modal',
  'skip',
  'toast',
  'max',
  'revealjs-menu',
];
const TEXT_SIZES = ['xs', 'sm', 'base', 'md', 'lg', 'xl'];
const FONT_WEIGHTS = ['normal', 'medium', 'semibold', 'bold'];
const LEADINGS = ['tight', 'base'];
const DURATIONS = ['fast', 'base'];
const EASES = ['out', 'standard'];

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section style={{ marginBottom: 'var(--space-6)' }}>
      <h2
        style={{
          fontSize: 'var(--text-lg)',
          color: 'var(--text-primary)',
          margin: `0 0 var(--space-3)`,
        }}
      >
        {title}
      </h2>
      {children}
    </section>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 'var(--space-3)',
        marginBottom: 'var(--space-2)',
      }}
    >
      <code
        style={{
          width: 220,
          flexShrink: 0,
          fontSize: 'var(--text-xs)',
          fontFamily: 'var(--font-mono)',
          color: 'var(--text-secondary)',
        }}
      >
        {label}
      </code>
      {children}
    </div>
  );
}

export default function DevTokensPage() {
  return (
    <div
      style={{
        padding: 'var(--space-6)',
        background: 'var(--page-bg)',
        color: 'var(--text-primary)',
        minHeight: '100vh',
      }}
    >
      <h1 style={{ fontSize: 'var(--text-xl)', margin: `0 0 var(--space-5)` }}>
        Design tokens
      </h1>

      <Section title="Spacing (4px base)">
        {SPACES.map((n) => (
          <Row key={n} label={`--space-${n}`}>
            <span
              style={{
                display: 'inline-block',
                width: `var(--space-${n})`,
                height: 'var(--space-4)',
                background: 'var(--posit-teal)',
              }}
            />
          </Row>
        ))}
      </Section>

      <Section title="Radii">
        {RADII.map((r) => (
          <Row key={r} label={`--radius-${r}`}>
            <span
              style={{
                display: 'inline-block',
                width: 48,
                height: 32,
                background: 'var(--input-bg-alpha)',
                border: '1px solid var(--border-color)',
                borderRadius: `var(--radius-${r})`,
              }}
            />
          </Row>
        ))}
      </Section>

      <Section title="Elevation">
        {SHADOWS.map((n) => (
          <Row key={n} label={`--shadow-${n}`}>
            <span
              style={{
                display: 'inline-block',
                width: 64,
                height: 40,
                background: 'var(--bg-modal)',
                borderRadius: 'var(--radius-md)',
                boxShadow: `var(--shadow-${n})`,
              }}
            />
          </Row>
        ))}
      </Section>

      <Section title="Z-index layers">
        {Z_LAYERS.map((z) => (
          <Row key={z} label={`--z-${z}`}>
            <span style={{ fontSize: 'var(--text-sm)', color: 'var(--text-secondary)' }}>
              layer
            </span>
          </Row>
        ))}
      </Section>

      <Section title="Type scale">
        {TEXT_SIZES.map((s) => (
          <Row key={s} label={`--text-${s}`}>
            <span style={{ fontSize: `var(--text-${s})`, color: 'var(--text-primary)' }}>
              Quarto Hub
            </span>
          </Row>
        ))}
        {FONT_WEIGHTS.map((w) => (
          <Row key={w} label={`--font-weight-${w}`}>
            <span style={{ fontWeight: `var(--font-weight-${w})`, fontSize: 'var(--text-base)' }}>
              Quarto Hub
            </span>
          </Row>
        ))}
        {LEADINGS.map((l) => (
          <Row key={l} label={`--leading-${l}`}>
            <span
              style={{
                lineHeight: `var(--leading-${l})`,
                fontSize: 'var(--text-base)',
                maxWidth: 320,
                display: 'inline-block',
              }}
            >
              The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy
              dog.
            </span>
          </Row>
        ))}
        <Row label="--font-mono">
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--text-base)' }}>
            quarto render index.qmd
          </span>
        </Row>
      </Section>

      <Section title="Motion">
        {DURATIONS.map((d) => (
          <Row key={d} label={`--duration-${d}`}>
            <span style={{ fontSize: 'var(--text-sm)', color: 'var(--text-secondary)' }}>
              duration token
            </span>
          </Row>
        ))}
        {EASES.map((e) => (
          <Row key={e} label={`--ease-${e}`}>
            <span style={{ fontSize: 'var(--text-sm)', color: 'var(--text-secondary)' }}>
              easing token
            </span>
          </Row>
        ))}
      </Section>

      <Section title="Focus ring">
        <Row label="--focus-ring">
          <button
            type="button"
            style={{
              fontSize: 'var(--text-base)',
              padding: `var(--space-2) var(--space-4)`,
              borderRadius: 'var(--radius-md)',
              border: '1px solid var(--border-color)',
              background: 'var(--bg-modal)',
              color: 'var(--text-primary)',
              outline: 'var(--focus-ring)',
              outlineOffset: 'var(--focus-ring-offset)',
            }}
          >
            Focused control
          </button>
        </Row>
      </Section>
    </div>
  );
}
