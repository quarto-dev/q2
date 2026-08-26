/**
 * Dev-only component gallery (#/dev/gallery): every design-system primitive
 * in its meaningful states, in both themes, covered by the Playwright
 * visual baselines and the menu keyboard-interaction spec. This is the
 * drift-prevention counterpart to lint:css — a new component belongs here
 * before it ships (see hub-client/design-system.md).
 *
 * Layout uses inline styles (out of lint:css scope, like DevTokensPage);
 * the primitives themselves use their real classes from ui.css.
 *
 * Phase 1 deliverable of the UI/UX modernization plan (bd-iguk0hpd).
 */

import React, { useState } from 'react';
import { Menu, MenuItem, MenuDivider, MenuLabel, MenuSubmenu } from './Menu';
import Tooltip from './Tooltip';
import {
  FilePlusIcon,
  UploadIcon,
  PrintIcon,
  SwitchIcon,
  ShareIcon,
  PreviewIcon,
  ForkIcon,
  PeekIcon,
  PeopleIcon,
  SortIcon,
  MoreIcon,
  LayoutMarkupIcon,
  LayoutSplitIcon,
  LayoutPreviewIcon,
  CommentsExpandIcon,
  CommentsShowIcon,
  CommentsHideIcon,
} from './icons';
import '../ui.css';

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
      <span
        style={{
          width: 140,
          flexShrink: 0,
          fontSize: 'var(--text-sm)',
          color: 'var(--text-secondary)',
        }}
      >
        {label}
      </span>
      <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-2)' }}>{children}</div>
    </div>
  );
}

/** Menu demo: a menu-button opening the shared Menu, exercising a strong
 *  item, a submenu, a hint, a divider, and a danger item. */
function MenuDemo() {
  const [open, setOpen] = useState(false);
  const [lastAction, setLastAction] = useState('(none)');
  const triggerRef = React.useRef<HTMLButtonElement>(null);
  const pick = (action: string) => () => setLastAction(action);
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 'var(--space-3)' }}>
      <div className="qh-menu-anchor">
        <button
          ref={triggerRef}
          className="qh-btn outline"
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
        >
          Gallery menu ▾
        </button>
        {open && (
          <Menu
            onClose={() => setOpen(false)}
            triggerRef={triggerRef}
            aria-label="Gallery actions"
          >
            <MenuLabel>GALLERY</MenuLabel>
            <MenuItem strong onSelect={pick('open')}>
              Open
            </MenuItem>
            <MenuSubmenu label="Move to">
              <MenuItem onSelect={pick('move-alpha')}>Alpha</MenuItem>
              <MenuItem onSelect={pick('move-beta')}>Beta</MenuItem>
            </MenuSubmenu>
            <MenuItem hint="⌘C" keepOpen onSelect={pick('copy')}>
              Copy link
            </MenuItem>
            <MenuItem subtext="A fresh copy, no history" onSelect={pick('duplicate')}>
              Duplicate
            </MenuItem>
            <MenuItem disabled onSelect={pick('disabled')}>
              Unavailable action
            </MenuItem>
            <MenuDivider />
            <MenuItem danger subtext="Cannot be undone" onSelect={pick('delete')}>
              Delete
            </MenuItem>
          </Menu>
        )}
      </div>
      <span
        data-testid="menu-last-action"
        style={{ fontSize: 'var(--text-sm)', color: 'var(--text-secondary)' }}
      >
        Last action: {lastAction}
      </span>
    </div>
  );
}

export default function DevGalleryPage() {
  const [inputValue, setInputValue] = useState('Editable text');
  return (
    <div
      style={{
        padding: 'var(--space-5)',
        maxWidth: 900,
        background: 'var(--page-bg)',
        color: 'var(--text-primary)',
        minHeight: '100vh',
      }}
    >
      <h1
        style={{
          fontSize: 'var(--text-xl)',
          color: 'var(--text-primary)',
          margin: `0 0 var(--space-5)`,
        }}
      >
        Component gallery
      </h1>

      <Section title="Buttons">
        <Row label="variants">
          <button className="qh-btn">Default</button>
          <button className="qh-btn primary">Primary</button>
          <button className="qh-btn outline">Outline</button>
          <button className="qh-btn danger">Danger</button>
          <button className="qh-btn ghost-accent">Ghost accent</button>
        </Row>
        <Row label="small">
          <button className="qh-btn small">Small</button>
          <button className="qh-btn small outline">Small outline</button>
        </Row>
        <Row label="disabled">
          <button className="qh-btn primary" disabled>
            Primary
          </button>
          <button className="qh-btn outline" disabled>
            Outline
          </button>
        </Row>
        <Row label="link buttons">
          <button className="qh-link">Link action</button>
          <button className="qh-link muted">Muted</button>
          <button className="qh-link danger">Danger</button>
        </Row>
      </Section>

      <Section title="Icon buttons">
        <Row label="default / boxed">
          <button className="qh-icon-btn" aria-label="Share">
            <ShareIcon />
          </button>
          <button className="qh-icon-btn boxed" aria-label="Switch project">
            <SwitchIcon />
          </button>
          <button className="qh-icon-btn" disabled aria-label="Disabled">
            <PrintIcon />
          </button>
        </Row>
      </Section>

      <Section title="Menu">
        <MenuDemo />
      </Section>

      <Section title="Tooltip">
        <Row label="hover / focus">
          <Tooltip content="Styled tooltip — 400ms on hover, instant on focus">
            <button className="qh-btn outline">Hover or focus me</button>
          </Tooltip>
        </Row>
      </Section>

      <Section title="Form controls">
        <Row label="text input">
          <input
            className="qh-input"
            style={{ maxWidth: 260 }}
            value={inputValue}
            onChange={(e) => setInputValue(e.target.value)}
            aria-label="Example input"
          />
        </Row>
        <Row label="select">
          <select className="qh-input" style={{ maxWidth: 260 }} aria-label="Example select">
            <option>First option</option>
            <option>Second option</option>
          </select>
        </Row>
        <Row label="invalid">
          <span style={{ display: 'flex', flexDirection: 'column', gap: 4, maxWidth: 260 }}>
            <input
              className="qh-input"
              aria-invalid="true"
              aria-describedby="gallery-input-error"
              defaultValue="bad value"
              aria-label="Invalid example"
            />
            <span
              id="gallery-input-error"
              style={{ fontSize: 'var(--text-sm)', color: 'var(--error-text)' }}
            >
              Error text wired via aria-describedby.
            </span>
          </span>
        </Row>
      </Section>

      <Section title="Icons">
        <Row label="stroke icons">
          <FilePlusIcon />
          <UploadIcon />
          <PrintIcon />
          <SwitchIcon />
          <ShareIcon />
          <PreviewIcon />
          <ForkIcon />
          <PeekIcon />
          <PeopleIcon />
          <SortIcon />
          <MoreIcon />
        </Row>
        <Row label="pictograms">
          <LayoutMarkupIcon />
          <LayoutSplitIcon />
          <LayoutPreviewIcon />
          <CommentsExpandIcon />
          <CommentsShowIcon />
          <CommentsHideIcon />
        </Row>
      </Section>
    </div>
  );
}
