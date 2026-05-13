/**
 * Dev-only harness for rendering components in isolation.
 *
 * Used by dev routes (#/dev/<page>) and Playwright visual regression tests
 * to render hard-to-reach UI states (migration screens, error states, etc.)
 * without needing real data.
 *
 * This component is only imported in development builds.
 */

import React from 'react';
import ProjectSetSetup from './ProjectSetSetup';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';

const FAKE_LEGACY_PROJECTS: ProjectEntry[] = [
  {
    id: 'fake-1',
    indexDocId: 'automerge:fake1',
    syncServer: 'wss://sync.automerge.org',
    description: 'My Research Paper',
    createdAt: new Date(Date.now() - 86400000).toISOString(),
    lastAccessed: new Date(Date.now() - 3600000).toISOString(),
  },
  {
    id: 'fake-2',
    indexDocId: 'automerge:fake2',
    syncServer: 'wss://sync.automerge.org',
    description: 'Course Notes',
    createdAt: new Date(Date.now() - 172800000).toISOString(),
    lastAccessed: new Date(Date.now() - 7200000).toISOString(),
  },
  {
    id: 'fake-3',
    indexDocId: 'automerge:fake3',
    syncServer: 'wss://sync.automerge.org',
    description: 'Blog',
    createdAt: new Date().toISOString(),
    lastAccessed: new Date().toISOString(),
  },
];

const noop = async () => {};

interface Props {
  page: string;
}

const DEV_PAGES: Record<string, () => React.ReactNode> = {
  'setup-migration': () => (
    <ProjectSetSetup
      hasMigration={true}
      legacyProjects={FAKE_LEGACY_PROJECTS}
      error={null}
      isConnecting={false}
      onCreateProjectSet={noop}
      onLinkProjectSet={noop}
      onMigrateProjects={noop}
      onMergeIntoProjectSet={noop}
    />
  ),
  'setup-migration-error': () => (
    <ProjectSetSetup
      hasMigration={true}
      legacyProjects={FAKE_LEGACY_PROJECTS}
      error="Connection failed: could not reach sync server"
      isConnecting={false}
      onCreateProjectSet={noop}
      onLinkProjectSet={noop}
      onMigrateProjects={noop}
      onMergeIntoProjectSet={noop}
    />
  ),
  'setup-fresh': () => (
    <ProjectSetSetup
      hasMigration={false}
      legacyProjects={[]}
      error={null}
      isConnecting={false}
      onCreateProjectSet={noop}
      onLinkProjectSet={noop}
      onMigrateProjects={noop}
      onMergeIntoProjectSet={noop}
    />
  ),
};

export default function DevHarness({ page }: Props) {
  const renderPage = DEV_PAGES[page];

  if (!renderPage) {
    const available = Object.keys(DEV_PAGES).join(', ');
    return (
      <div style={{ padding: 40, color: 'var(--text-primary)', fontFamily: 'monospace' }}>
        <h2>Unknown dev page: {page}</h2>
        <p>Available pages: {available}</p>
      </div>
    );
  }

  return renderPage();
}
