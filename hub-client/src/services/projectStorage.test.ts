/**
 * Tests for projectStorage service
 *
 * These tests verify the IndexedDB-based project storage operations.
 * Uses fake-indexeddb for in-memory database simulation.
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import 'fake-indexeddb/auto';
import { IDBFactory } from 'fake-indexeddb';
import {
  listProjects,
  getProject,
  getProjectByIndexDocId,
  addProject,
  updateProject,
  touchProject,
  deleteProject,
  exportData,
  importData,
  closeDatabase,
} from './projectStorage';

describe('projectStorage', () => {
  beforeEach(() => {
    // Reset IndexedDB for test isolation
    // fake-indexeddb/auto replaces the global indexedDB
    // We need to close any existing connections first
    closeDatabase();

    // Reset the global indexedDB instance
    // This ensures each test starts with a fresh database
    const idbFactory = new IDBFactory();
    Object.defineProperty(globalThis, 'indexedDB', {
      value: idbFactory,
      writable: true,
    });
  });

  afterEach(() => {
    closeDatabase();
  });

  describe('basic CRUD operations', () => {
    it('should add a project', async () => {
      const project = await addProject(
        'automerge:test-doc-123',
        'ws://localhost:3030',
        'Test Project',
      );

      expect(project.id).toBeTruthy();
      expect(project.indexDocId).toBe('automerge:test-doc-123');
      expect(project.syncServer).toBe('ws://localhost:3030');
      expect(project.description).toBe('Test Project');
      expect(project.createdAt).toBeTruthy();
      expect(project.lastAccessed).toBeTruthy();
    });

    it('should generate default description if not provided', async () => {
      const project = await addProject('automerge:doc-456', 'ws://localhost:3030');

      expect(project.description).toMatch(/^Project \d{4}-\d{2}-\d{2}/);
    });

    it('should get a project by ID', async () => {
      const created = await addProject('automerge:doc-1', 'ws://localhost:3030', 'Project 1');

      const retrieved = await getProject(created.id);

      expect(retrieved).toBeDefined();
      expect(retrieved?.id).toBe(created.id);
      expect(retrieved?.description).toBe('Project 1');
    });

    it('should return undefined for non-existent project', async () => {
      const result = await getProject('non-existent-id');
      expect(result).toBeUndefined();
    });

    it('should get a project by indexDocId', async () => {
      await addProject('automerge:unique-doc-id', 'ws://localhost:3030', 'Unique Project');

      const found = await getProjectByIndexDocId('automerge:unique-doc-id');

      expect(found).toBeDefined();
      expect(found?.description).toBe('Unique Project');
    });

    it('should list all projects', async () => {
      await addProject('automerge:doc-a', 'ws://localhost:3030', 'Project A');
      await new Promise((r) => setTimeout(r, 10));
      await addProject('automerge:doc-b', 'ws://localhost:3030', 'Project B');
      await new Promise((r) => setTimeout(r, 10));
      await addProject('automerge:doc-c', 'ws://localhost:3030', 'Project C');

      const projects = await listProjects();

      expect(projects).toHaveLength(3);
      // Projects should be sorted by lastAccessed (most recent first)
      expect(projects[0].description).toBe('Project C');
      expect(projects[2].description).toBe('Project A');
    });

    it('should update a project', async () => {
      const project = await addProject('automerge:update-test', 'ws://localhost:3030', 'Original');

      project.description = 'Updated Description';
      await updateProject(project);

      const updated = await getProject(project.id);
      expect(updated?.description).toBe('Updated Description');
    });

    it('should touch project to update lastAccessed', async () => {
      const project = await addProject('automerge:touch-test', 'ws://localhost:3030', 'Touch Test');
      const originalLastAccessed = project.lastAccessed;

      // Wait a bit to ensure timestamp changes
      await new Promise((r) => setTimeout(r, 10));

      await touchProject(project.id);

      const touched = await getProject(project.id);
      expect(touched?.lastAccessed).not.toBe(originalLastAccessed);
    });

    it('should delete a project', async () => {
      const project = await addProject('automerge:delete-test', 'ws://localhost:3030', 'To Delete');

      await deleteProject(project.id);

      const deleted = await getProject(project.id);
      expect(deleted).toBeUndefined();
    });
  });

  describe('export and import', () => {
    it('should export data as JSON', async () => {
      await addProject('automerge:export-1', 'ws://server1', 'Export Test 1');
      await addProject('automerge:export-2', 'ws://server2', 'Export Test 2');

      const exported = await exportData();
      const data = JSON.parse(exported);

      expect(data.schemaVersion).toBeDefined();
      expect(data.exportedAt).toBeTruthy();
      expect(data.projects).toHaveLength(2);
    });

    it('should import data from JSON', async () => {
      const exportedData = {
        schemaVersion: 1,
        exportedAt: new Date().toISOString(),
        projects: [
          {
            id: 'temp-1',
            indexDocId: 'automerge:import-1',
            syncServer: 'ws://import-server',
            description: 'Imported Project 1',
            createdAt: new Date().toISOString(),
            lastAccessed: new Date().toISOString(),
          },
          {
            id: 'temp-2',
            indexDocId: 'automerge:import-2',
            syncServer: 'ws://import-server',
            description: 'Imported Project 2',
            createdAt: new Date().toISOString(),
            lastAccessed: new Date().toISOString(),
          },
        ],
      };

      const count = await importData(JSON.stringify(exportedData));

      expect(count).toBe(2);

      const projects = await listProjects();
      expect(projects).toHaveLength(2);
    });

    it('should skip importing projects with duplicate indexDocId', async () => {
      // Create an existing project
      await addProject('automerge:existing-doc', 'ws://original-server', 'Existing');

      const exportedData = {
        schemaVersion: 1,
        exportedAt: new Date().toISOString(),
        projects: [
          {
            id: 'temp',
            indexDocId: 'automerge:existing-doc', // Duplicate!
            syncServer: 'ws://new-server',
            description: 'Should be skipped',
            createdAt: new Date().toISOString(),
            lastAccessed: new Date().toISOString(),
          },
        ],
      };

      const count = await importData(JSON.stringify(exportedData));

      expect(count).toBe(0); // Should skip the duplicate

      const projects = await listProjects();
      expect(projects).toHaveLength(1);
      expect(projects[0].description).toBe('Existing'); // Original preserved
    });

    it('should import legacy array format', async () => {
      const legacyData = [
        {
          id: 'legacy-1',
          indexDocId: 'automerge:legacy-1',
          syncServer: 'ws://legacy-server',
          description: 'Legacy Project',
          createdAt: new Date().toISOString(),
          lastAccessed: new Date().toISOString(),
        },
      ];

      const count = await importData(JSON.stringify(legacyData));

      expect(count).toBe(1);

      const projects = await listProjects();
      expect(projects).toHaveLength(1);
      expect(projects[0].description).toBe('Legacy Project');
    });

    it('should throw on invalid import format', async () => {
      await expect(importData(JSON.stringify({ invalid: 'format' }))).rejects.toThrow(
        'Invalid import format',
      );
    });
  });

  describe('ordering', () => {
    it('should return projects sorted by lastAccessed (most recent first)', async () => {
      await addProject('automerge:old', 'ws://test', 'Old Project');
      await new Promise((r) => setTimeout(r, 10));
      await addProject('automerge:middle', 'ws://test', 'Middle Project');
      await new Promise((r) => setTimeout(r, 10));
      await addProject('automerge:new', 'ws://test', 'New Project');

      const projects = await listProjects();

      expect(projects[0].description).toBe('New Project');
      expect(projects[1].description).toBe('Middle Project');
      expect(projects[2].description).toBe('Old Project');
    });

    it('should update ordering when project is touched', async () => {
      const oldProject = await addProject('automerge:was-old', 'ws://test', 'Was Old');
      await new Promise((r) => setTimeout(r, 10));
      await addProject('automerge:newer', 'ws://test', 'Newer');

      // Touch the old project to make it most recent
      await touchProject(oldProject.id);

      const projects = await listProjects();
      expect(projects[0].description).toBe('Was Old');
    });
  });

  describe('unique constraints', () => {
    it('should enforce unique indexDocId through index', async () => {
      await addProject('automerge:unique-constraint', 'ws://server1', 'First');

      // Adding another project with same indexDocId should fail
      // (IndexedDB unique index constraint)
      await expect(
        addProject('automerge:unique-constraint', 'ws://server2', 'Second'),
      ).rejects.toThrow();
    });
  });
});
