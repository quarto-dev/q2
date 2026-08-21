/**
 * Tests for ProjectSetDocument schema helpers.
 *
 * These test the pure functions that manipulate the project set document.
 * In production, these run inside Automerge `change()` callbacks, but
 * they can be tested with plain objects.
 */

import { describe, it, expect } from 'vitest';
import {
  CURRENT_PROJECT_SET_SCHEMA_VERSION,
  initProjectSetDocument,
  projectSetKey,
  addProjectToSet,
  removeProjectFromSet,
  touchProjectInSet,
  updateProjectSummaryInSet,
  setProjectSetName,
  getProjectSetTombstones,
} from './index.js';
import type { ProjectSetDocument } from './index.js';

function emptyDoc(): ProjectSetDocument {
  const doc = {} as ProjectSetDocument;
  initProjectSetDocument(doc);
  return doc;
}

describe('ProjectSetDocument schema helpers', () => {
  describe('initProjectSetDocument', () => {
    it('should initialize an empty document', () => {
      const doc = emptyDoc();
      expect(doc.projects).toEqual({});
      expect(doc.version).toBe(CURRENT_PROJECT_SET_SCHEMA_VERSION);
    });
  });

  describe('projectSetKey', () => {
    it('should strip automerge: prefix', () => {
      expect(projectSetKey('automerge:abc123')).toBe('abc123');
    });

    it('should return as-is if no prefix', () => {
      expect(projectSetKey('abc123')).toBe('abc123');
    });
  });

  describe('addProjectToSet', () => {
    it('should add a new project', () => {
      const doc = emptyDoc();
      const result = addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'My Project',
      }, '2026-01-15T00:00:00.000Z');

      expect(result).toBe(true);
      expect(doc.projects['proj1']).toEqual({
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'My Project',
        addedAt: '2026-01-15T00:00:00.000Z',
        lastAccessed: '2026-01-15T00:00:00.000Z',
      });
    });

    it('should return false for duplicate with same metadata', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'My Project',
      }, '2026-01-15T00:00:00.000Z');

      const result = addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'My Project',
      });

      expect(result).toBe(false);
    });

    it('should update description if changed', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'Old Name',
      }, '2026-01-15T00:00:00.000Z');

      const result = addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'New Name',
      });

      expect(result).toBe(true);
      expect(doc.projects['proj1'].description).toBe('New Name');
      // addedAt should not change
      expect(doc.projects['proj1'].addedAt).toBe('2026-01-15T00:00:00.000Z');
    });

    it('should update syncServer if changed', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://old-server',
        description: 'Project',
      });

      const result = addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://new-server',
        description: 'Project',
      });

      expect(result).toBe(true);
      expect(doc.projects['proj1'].syncServer).toBe('wss://new-server');
    });

    it('should handle multiple projects', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'First',
      });
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj2',
        syncServer: 'wss://sync.example.com',
        description: 'Second',
      });

      expect(Object.keys(doc.projects)).toHaveLength(2);
      expect(doc.projects['proj1'].description).toBe('First');
      expect(doc.projects['proj2'].description).toBe('Second');
    });
  });

  describe('removeProjectFromSet', () => {
    it('should remove an existing project', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'To Remove',
      });

      const result = removeProjectFromSet(doc, 'automerge:proj1');
      expect(result).toBe(true);
      expect(doc.projects['proj1']).toBeUndefined();
    });

    it('should return false for non-existent project', () => {
      const doc = emptyDoc();
      const result = removeProjectFromSet(doc, 'automerge:nonexistent');
      expect(result).toBe(false);
    });

    it('should handle indexDocId without prefix', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'Project',
      });

      const result = removeProjectFromSet(doc, 'proj1');
      expect(result).toBe(true);
      expect(doc.projects['proj1']).toBeUndefined();
    });
  });

  describe('tombstones', () => {
    it('removeProjectFromSet records a deletion tombstone', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'To Remove',
      });

      const result = removeProjectFromSet(doc, 'automerge:proj1', '2026-08-21T10:00:00.000Z');

      expect(result).toBe(true);
      expect(doc.projects['proj1']).toBeUndefined();
      expect(doc.tombstones).toEqual({ proj1: '2026-08-21T10:00:00.000Z' });
      expect(getProjectSetTombstones(doc)).toEqual({ proj1: '2026-08-21T10:00:00.000Z' });
    });

    it('removeProjectFromSet of an absent project writes no tombstone', () => {
      const doc = emptyDoc();

      const result = removeProjectFromSet(doc, 'automerge:ghost');

      expect(result).toBe(false);
      expect(doc.tombstones).toBeUndefined();
    });

    it('addProjectToSet clears the tombstone — a re-add wins over the delete', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'Project',
      });
      removeProjectFromSet(doc, 'automerge:proj1', '2026-08-21T10:00:00.000Z');

      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'Project',
      });

      expect(doc.projects['proj1']).toBeDefined();
      expect(doc.tombstones).toEqual({});
    });

    it('addProjectToSet update path also clears a lingering tombstone', () => {
      // Torn state after a concurrent add/delete merge: entry present AND
      // tombstone present. The add heals it.
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'Project',
      });
      doc.tombstones = { proj1: '2026-08-21T10:00:00.000Z' };

      const result = addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'Project',
      });

      expect(result).toBe(true); // clearing the tombstone counts as a change
      expect(doc.tombstones).toEqual({});
    });

    it('getProjectSetTombstones returns {} for documents predating tombstones', () => {
      const doc = emptyDoc();
      expect(doc.tombstones).toBeUndefined();
      expect(getProjectSetTombstones(doc)).toEqual({});
    });
  });

  describe('touchProjectInSet', () => {
    it('should update lastAccessed', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'Project',
      }, '2026-01-01T00:00:00.000Z');

      const result = touchProjectInSet(doc, 'automerge:proj1', '2026-06-15T12:00:00.000Z');
      expect(result).toBe(true);
      expect(doc.projects['proj1'].lastAccessed).toBe('2026-06-15T12:00:00.000Z');
      // addedAt should not change
      expect(doc.projects['proj1'].addedAt).toBe('2026-01-01T00:00:00.000Z');
    });

    it('should return false for non-existent project', () => {
      const doc = emptyDoc();
      const result = touchProjectInSet(doc, 'automerge:nonexistent');
      expect(result).toBe(false);
    });
  });

  describe('setProjectSetName', () => {
    it('should set and change the collection name', () => {
      const doc = emptyDoc();
      expect(setProjectSetName(doc, 'Lab papers')).toBe(true);
      expect(doc.name).toBe('Lab papers');
      expect(setProjectSetName(doc, 'Lab papers')).toBe(false);
      expect(setProjectSetName(doc, 'Lab notebooks')).toBe(true);
      expect(doc.name).toBe('Lab notebooks');
    });
  });

  describe('updateProjectSummaryInSet', () => {
    const summary = {
      fileCount: 3,
      topFiles: ['index.qmd', 'notes.qmd', '_quarto.yml'],
      contributors: [{ name: 'Charlotte Wu', color: '#E8368F' }],
      asOf: '2026-06-15T12:00:00.000Z',
    };

    it('should write the summary onto an existing entry', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'Project',
      }, '2026-01-01T00:00:00.000Z');

      const result = updateProjectSummaryInSet(doc, 'automerge:proj1', summary);
      expect(result).toBe(true);
      expect(doc.projects['proj1'].summary).toEqual(summary);
      // Other fields untouched
      expect(doc.projects['proj1'].lastAccessed).toBe('2026-01-01T00:00:00.000Z');
    });

    it('should replace file-shape fields but union contributors', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'Project',
      });
      updateProjectSummaryInSet(doc, 'automerge:proj1', summary);
      // A different collaborator writes their own view later
      const newer = {
        fileCount: 5,
        topFiles: ['index.qmd'],
        contributors: [{ name: 'Saima Khan', color: '#00BCD4' }],
        asOf: '2026-06-16T12:00:00.000Z',
      };
      updateProjectSummaryInSet(doc, 'automerge:proj1', newer);
      const stored = doc.projects['proj1'].summary!;
      // File-shape fields take the newer writer's view
      expect(stored.fileCount).toBe(5);
      expect(stored.topFiles).toEqual(['index.qmd']);
      expect(stored.asOf).toBe('2026-06-16T12:00:00.000Z');
      // Contributors accumulate — neither author clobbers the other
      expect(stored.contributors.map((c) => c.name).sort()).toEqual(['Charlotte Wu', 'Saima Khan']);
    });

    it('should not duplicate a contributor already present', () => {
      const doc = emptyDoc();
      addProjectToSet(doc, {
        indexDocId: 'automerge:proj1',
        syncServer: 'wss://sync.example.com',
        description: 'Project',
      });
      updateProjectSummaryInSet(doc, 'automerge:proj1', summary);
      // Same author edits again with an updated color
      updateProjectSummaryInSet(doc, 'automerge:proj1', {
        ...summary,
        contributors: [{ name: 'Charlotte Wu', color: '#FF0000' }],
      });
      const stored = doc.projects['proj1'].summary!;
      expect(stored.contributors).toHaveLength(1);
      expect(stored.contributors[0]).toEqual({ name: 'Charlotte Wu', color: '#FF0000' });
    });

    it('should return false for non-existent project', () => {
      const doc = emptyDoc();
      const result = updateProjectSummaryInSet(doc, 'automerge:nonexistent', summary);
      expect(result).toBe(false);
    });
  });
});
