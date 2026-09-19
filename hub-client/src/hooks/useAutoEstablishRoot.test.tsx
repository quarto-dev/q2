/**
 * @vitest-environment jsdom
 *
 * useAutoEstablishRoot: silently establish the personal root collection
 * the moment the collections hook reports that none exists, so a new user
 * never sees a setup screen (bd-4h1hv60p). Generalizes the invite
 * (bd-fxdcxbpq) and ephemeral-preview (bd-zf4ryvuq) effects that App.tsx
 * used to carry as two copies.
 *
 * Plan: claude-notes/plans/2026-09-15-auto-create-project-set-on-first-run.md
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { renderHook, cleanup } from '@testing-library/react';
import { useAutoEstablishRoot } from './useAutoEstablishRoot';
import type { CollectionsStatus } from './useCollectionSets';
import { DEFAULT_SYNC_SERVER } from '../utils/routing';

afterEach(() => cleanup());

function harness(initial: CollectionsStatus, enabled = true) {
  const createProjectSet = vi.fn(async () => {});
  const migrateProjects = vi.fn(async () => {});
  const hook = renderHook(
    ({ status, enabled }) =>
      useAutoEstablishRoot({ status, enabled, createProjectSet, migrateProjects }),
    { initialProps: { status: initial, enabled } },
  );
  return { ...hook, createProjectSet, migrateProjects };
}

describe('useAutoEstablishRoot', () => {
  it('creates the root against DEFAULT_SYNC_SERVER on needs-setup', () => {
    const h = harness('loading');
    expect(h.createProjectSet).not.toHaveBeenCalled();
    h.rerender({ status: 'needs-setup', enabled: true });
    expect(h.createProjectSet).toHaveBeenCalledTimes(1);
    expect(h.createProjectSet).toHaveBeenCalledWith(DEFAULT_SYNC_SERVER);
    expect(h.migrateProjects).not.toHaveBeenCalled();
  });

  it('migrates legacy projects silently on needs-migration', () => {
    const h = harness('needs-migration');
    expect(h.migrateProjects).toHaveBeenCalledTimes(1);
    expect(h.migrateProjects).toHaveBeenCalledWith(DEFAULT_SYNC_SERVER);
    expect(h.createProjectSet).not.toHaveBeenCalled();
  });

  it.each<CollectionsStatus>(['loading', 'connecting', 'connected', 'error'])(
    'does nothing while %s',
    (status) => {
      const h = harness(status);
      h.rerender({ status, enabled: true });
      expect(h.createProjectSet).not.toHaveBeenCalled();
      expect(h.migrateProjects).not.toHaveBeenCalled();
    },
  );

  it('fires once: a failed migration that returns to needs-migration does not retry-loop', () => {
    const h = harness('needs-migration');
    h.rerender({ status: 'connecting', enabled: true });
    h.rerender({ status: 'needs-migration', enabled: true });
    h.rerender({ status: 'connecting', enabled: true });
    h.rerender({ status: 'needs-migration', enabled: true });
    expect(h.migrateProjects).toHaveBeenCalledTimes(1);
  });

  it('fires once across the create → error → needs-setup path too', () => {
    const h = harness('needs-setup');
    h.rerender({ status: 'connecting', enabled: true });
    h.rerender({ status: 'error', enabled: true });
    h.rerender({ status: 'needs-setup', enabled: true });
    expect(h.createProjectSet).toHaveBeenCalledTimes(1);
  });

  it('re-arms when a retry cycle passes through loading', () => {
    const h = harness('needs-setup');
    h.rerender({ status: 'connecting', enabled: true });
    h.rerender({ status: 'error', enabled: true });
    h.rerender({ status: 'loading', enabled: true });
    h.rerender({ status: 'needs-setup', enabled: true });
    expect(h.createProjectSet).toHaveBeenCalledTimes(2);
  });

  it('stays out of the way when disabled (a link-project-set boot URL owns setup)', () => {
    const h = harness('needs-setup', false);
    h.rerender({ status: 'needs-migration', enabled: false });
    expect(h.createProjectSet).not.toHaveBeenCalled();
    expect(h.migrateProjects).not.toHaveBeenCalled();
  });
});

/**
 * `onFreshRoot` (bd-3fwtdhil): runs once, after a `needs-setup` boot has
 * created its root and reached `connected`. That is the one moment a
 * brand-new browser exists with an empty root, which is when the
 * "Examples / Templates" collection gets seeded. Migrations, returning
 * browsers, failed setups, and disabled boots never fire it.
 */
describe('useAutoEstablishRoot onFreshRoot', () => {
  function harness(initial: CollectionsStatus, enabled = true) {
    const onFreshRoot = vi.fn(async () => {});
    const createProjectSet = vi.fn(async () => {});
    const migrateProjects = vi.fn(async () => {});
    const hook = renderHook(
      ({ status, enabled }) =>
        useAutoEstablishRoot({ status, enabled, createProjectSet, migrateProjects, onFreshRoot }),
      { initialProps: { status: initial, enabled } },
    );
    return { ...hook, onFreshRoot };
  }

  it('fires once when a needs-setup boot reaches connected', () => {
    const h = harness('loading');
    h.rerender({ status: 'needs-setup', enabled: true });
    h.rerender({ status: 'connecting', enabled: true });
    expect(h.onFreshRoot).not.toHaveBeenCalled();
    h.rerender({ status: 'connected', enabled: true });
    expect(h.onFreshRoot).toHaveBeenCalledTimes(1);
    h.rerender({ status: 'connected', enabled: true });
    h.rerender({ status: 'connecting', enabled: true });
    h.rerender({ status: 'connected', enabled: true });
    expect(h.onFreshRoot).toHaveBeenCalledTimes(1);
  });

  it('does not fire for a migration boot', () => {
    const h = harness('needs-migration');
    h.rerender({ status: 'connecting', enabled: true });
    h.rerender({ status: 'connected', enabled: true });
    expect(h.onFreshRoot).not.toHaveBeenCalled();
  });

  it('does not fire for a returning browser that connects directly', () => {
    const h = harness('loading');
    h.rerender({ status: 'connecting', enabled: true });
    h.rerender({ status: 'connected', enabled: true });
    expect(h.onFreshRoot).not.toHaveBeenCalled();
  });

  it('does not fire when a needs-setup boot ends in error', () => {
    const h = harness('needs-setup');
    h.rerender({ status: 'connecting', enabled: true });
    h.rerender({ status: 'error', enabled: true });
    expect(h.onFreshRoot).not.toHaveBeenCalled();
  });

  it('fires after a retry cycle that finally connects', () => {
    const h = harness('needs-setup');
    h.rerender({ status: 'connecting', enabled: true });
    h.rerender({ status: 'error', enabled: true });
    h.rerender({ status: 'loading', enabled: true });
    h.rerender({ status: 'needs-setup', enabled: true });
    h.rerender({ status: 'connecting', enabled: true });
    h.rerender({ status: 'connected', enabled: true });
    expect(h.onFreshRoot).toHaveBeenCalledTimes(1);
  });

  it('does not fire when disabled', () => {
    const h = harness('needs-setup', false);
    h.rerender({ status: 'connecting', enabled: false });
    h.rerender({ status: 'connected', enabled: false });
    expect(h.onFreshRoot).not.toHaveBeenCalled();
  });

  it('is optional: the hook works without it', () => {
    const createProjectSet = vi.fn(async () => {});
    const migrateProjects = vi.fn(async () => {});
    const hook = renderHook(
      ({ status }) =>
        useAutoEstablishRoot({ status, enabled: true, createProjectSet, migrateProjects }),
      { initialProps: { status: 'needs-setup' as CollectionsStatus } },
    );
    hook.rerender({ status: 'connected' });
    expect(createProjectSet).toHaveBeenCalledTimes(1);
  });
});
