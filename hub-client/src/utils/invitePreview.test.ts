/**
 * Tests for the invite preview payload codec (bd-fxdcxbpq).
 *
 * Invite URLs carry a display-only `preview=` payload (base64url JSON) and
 * collection invites a `start=` target. These tests pin the codec contract:
 * compact round-trips, hard caps on payload size, and tolerant decoding —
 * legacy or malformed payloads must decode to `undefined`, never throw.
 */

import { describe, it, expect } from 'vitest';
import {
  encodeInvitePreview,
  decodeInvitePreview,
  encodeInviteStart,
  decodeInviteStart,
  MAX_PREVIEW_PROJECTS,
  MAX_PREVIEW_FILES,
} from './invitePreview';
import type { CollectionInvitePreview, DocumentInvitePreview, InviteStart } from './invitePreview';

const collectionPreview: CollectionInvitePreview = {
  kind: 'collection',
  projects: [
    { name: 'Quarterly report', topFiles: ['report.qmd'], fileCount: 12, contributorInitials: ['CS', 'JL'] },
    { name: 'Methods paper', topFiles: ['paper.qmd'], fileCount: 7, contributorInitials: ['JL'] },
  ],
  totalProjects: 4,
  memberFirstNames: ['Carlos', 'Jenny', 'Mine'],
};

const documentPreview: DocumentInvitePreview = {
  kind: 'document',
  fileName: 'report.qmd',
  topFiles: ['figures/', 'data.csv'],
  fileCount: 12,
  contributorInitials: ['CS', 'JL'],
};

describe('invite preview codec', () => {
  it('round-trips a collection preview', () => {
    const encoded = encodeInvitePreview(collectionPreview);
    expect(decodeInvitePreview(encoded)).toEqual(collectionPreview);
  });

  it('round-trips a document preview', () => {
    const encoded = encodeInvitePreview(documentPreview);
    expect(decodeInvitePreview(encoded)).toEqual(documentPreview);
  });

  it('produces URL-safe output (base64url, no padding/escaping needed)', () => {
    for (const p of [collectionPreview, documentPreview]) {
      const encoded = encodeInvitePreview(p);
      expect(encoded).toMatch(/^[A-Za-z0-9_-]+$/);
      // Survives a URLSearchParams round-trip unchanged.
      const params = new URLSearchParams();
      params.set('preview', encoded);
      expect(new URLSearchParams(params.toString()).get('preview')).toBe(encoded);
    }
  });

  it(`caps encoding at ${MAX_PREVIEW_PROJECTS} projects x ${MAX_PREVIEW_FILES} files`, () => {
    const oversized: CollectionInvitePreview = {
      kind: 'collection',
      projects: Array.from({ length: 6 }, (_, i) => ({
        name: `Project ${i}`,
        topFiles: ['a.qmd', 'b.qmd', 'c.qmd', 'd.qmd'],
        fileCount: 20,
        contributorInitials: ['AA', 'BB'],
      })),
      totalProjects: 6,
      memberFirstNames: ['Ann', 'Bo'],
    };
    const decoded = decodeInvitePreview(encodeInvitePreview(oversized));
    expect(decoded?.kind).toBe('collection');
    if (decoded?.kind === 'collection') {
      expect(decoded.projects).toHaveLength(MAX_PREVIEW_PROJECTS);
      for (const p of decoded.projects) {
        expect(p.topFiles.length).toBeLessThanOrEqual(MAX_PREVIEW_FILES);
      }
      // The total survives the cap so "+ N more projects" stays truthful.
      expect(decoded.totalProjects).toBe(6);
    }
  });

  it('decodes absent values to undefined', () => {
    expect(decodeInvitePreview(null)).toBeUndefined();
    expect(decodeInvitePreview(undefined)).toBeUndefined();
    expect(decodeInvitePreview('')).toBeUndefined();
  });

  it('decodes garbage to undefined without throwing', () => {
    expect(decodeInvitePreview('%%%not-base64%%%')).toBeUndefined();
    // Valid base64url of a non-JSON string.
    expect(decodeInvitePreview(btoa('hello world').replace(/=+$/, ''))).toBeUndefined();
    // Valid JSON, wrong shape.
    expect(decodeInvitePreview(btoa(JSON.stringify({ foo: 1 })).replace(/=+$/, ''))).toBeUndefined();
    // Wrong/unknown version marker.
    expect(decodeInvitePreview(btoa(JSON.stringify({ v: 99, k: 'c' })).replace(/=+$/, ''))).toBeUndefined();
  });
});

describe('invite start codec', () => {
  const start: InviteStart = {
    indexDocId: '2Agx7kENjysHSujsVgirvykVKECf',
    filePath: 'report.qmd',
  };

  it('round-trips a start target', () => {
    expect(decodeInviteStart(encodeInviteStart(start))).toEqual(start);
  });

  it('is URL-safe', () => {
    expect(encodeInviteStart(start)).toMatch(/^[A-Za-z0-9_-]+$/);
  });

  it('decodes absent or malformed values to undefined', () => {
    expect(decodeInviteStart(null)).toBeUndefined();
    expect(decodeInviteStart(undefined)).toBeUndefined();
    expect(decodeInviteStart('')).toBeUndefined();
    expect(decodeInviteStart('!!!')).toBeUndefined();
    expect(decodeInviteStart(btoa(JSON.stringify({ nope: true })).replace(/=+$/, ''))).toBeUndefined();
  });
});
