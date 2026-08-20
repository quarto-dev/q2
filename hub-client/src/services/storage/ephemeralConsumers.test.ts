/**
 * Ephemeral storage mode through the real consumer modules (bd-sw4xy1vw).
 *
 * The unit tests run in the node environment, where the `indexedDB`
 * global does not exist: any code path that touches the real database
 * throws. With VITE_EPHEMERAL_STORAGE=1 the projectStorage,
 * userSettings, and projectSetStorage modules must operate entirely in
 * memory — this is what keeps per-session `q2 preview` origins from
 * accumulating stale IndexedDB databases.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import * as projectStorage from '../projectStorage';
import * as userSettings from '../userSettings';
import * as projectSetStorage from '../projectSetStorage';
import { resetDbPromise } from './index';

describe('ephemeral storage consumers', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_EPHEMERAL_STORAGE', '1');
    resetDbPromise();
    // Guard: this test file must never install fake-indexeddb. If the
    // global exists, the "never touches IndexedDB" property is unproven.
    expect(globalThis.indexedDB).toBeUndefined();
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    resetDbPromise();
  });

  it('projectStorage CRUD round-trips in memory', async () => {
    const project = await projectStorage.addProject('automerge:abc', '/ws', 'Demo');
    expect(project.id).toBeTruthy();
    expect(await projectStorage.getProject(project.id)).toEqual(project);
    expect(await projectStorage.getProjectByIndexDocId('automerge:abc')).toEqual(project);
    expect(await projectStorage.listProjects()).toHaveLength(1);

    await projectStorage.updateProject({ ...project, description: 'Renamed' });
    expect((await projectStorage.getProject(project.id))?.description).toBe('Renamed');

    await projectStorage.touchProject(project.id);
    const touched = await projectStorage.getProject(project.id);
    expect(Date.parse(touched!.lastAccessed)).toBeGreaterThanOrEqual(
      Date.parse(project.lastAccessed),
    );

    await projectStorage.deleteProject(project.id);
    expect(await projectStorage.getProject(project.id)).toBeUndefined();
    expect(await projectStorage.listProjects()).toEqual([]);
  });

  it('userSettings identity is created once and stable within the session', async () => {
    const identity = await userSettings.getUserIdentity();
    expect(identity.userId).toBeTruthy();
    expect(identity.userName).toBeTruthy();

    const again = await userSettings.getUserIdentity();
    expect(again.userId).toBe(identity.userId);

    const renamed = await userSettings.updateUserName('Ada');
    expect(renamed.userName).toBe('Ada');
    expect((await userSettings.getUserIdentity()).userName).toBe('Ada');
    expect(await userSettings.getUserId()).toBe(identity.userId);
  });

  it('projectSetStorage pointer and collections round-trip in memory', async () => {
    expect(await projectSetStorage.getProjectSetPointer()).toBeNull();
    expect(await projectSetStorage.getCollectionPointers()).toEqual([]);

    await projectSetStorage.setProjectSetPointer('automerge:root', '/ws');
    expect((await projectSetStorage.getProjectSetPointer())?.projectSetDocId).toBe(
      'automerge:root',
    );

    // Lazy self-heal: the legacy singleton migrates to a one-element
    // collections array on read.
    expect(await projectSetStorage.getCollectionPointers()).toEqual([
      { projectSetDocId: 'automerge:root', syncServer: '/ws' },
    ]);

    await projectSetStorage.addCollectionPointer({
      projectSetDocId: 'automerge:team',
      syncServer: '/ws',
    });
    expect(await projectSetStorage.getCollectionPointers()).toHaveLength(2);

    await projectSetStorage.removeCollectionPointer('automerge:team');
    expect(await projectSetStorage.getCollectionPointers()).toHaveLength(1);

    await projectSetStorage.clearProjectSetPointer();
    expect(await projectSetStorage.getProjectSetPointer()).toBeNull();
  });

  it('nothing survives a resetDbPromise (the session boundary)', async () => {
    await projectStorage.addProject('automerge:abc', '/ws', 'Demo');
    await userSettings.getUserIdentity();
    await projectSetStorage.setProjectSetPointer('automerge:root', '/ws');

    resetDbPromise();

    expect(await projectStorage.listProjects()).toEqual([]);
    expect(await projectSetStorage.getProjectSetPointer()).toBeNull();
    // A fresh identity is generated (new userId).
    const identity = await userSettings.getUserIdentity();
    expect(identity.userId).toBeTruthy();
  });
});
