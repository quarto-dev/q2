/**
 * Unit tests for the QuickPick collection-label logic.
 *
 * The collections quick-pick list shows each collection's real name (read from
 * its ProjectSetDocument once loaded), falling back to a generic label while
 * the doc is still loading or when it has no name. The root collection is
 * tagged so it's distinguishable from the named ones.
 */

import { describe, it, expect } from 'vitest'
import { collectionLabel } from './QuickPick'

describe('collectionLabel', () => {
  it('uses the document name once loaded', () => {
    expect(collectionLabel({ name: 'Team docs' }, false)).toBe('Team docs')
  })

  it('tags the root collection', () => {
    expect(collectionLabel({ name: 'My projects' }, true)).toBe('My projects (root)')
  })

  it('falls back to a generic label while the doc is still loading (undefined)', () => {
    expect(collectionLabel(undefined, false)).toBe('Collection')
    expect(collectionLabel(undefined, true)).toBe('Collection (root)')
  })

  it('falls back when the doc has no name', () => {
    expect(collectionLabel({ projects: {} }, false)).toBe('Collection')
    expect(collectionLabel({ projects: {} }, true)).toBe('Collection (root)')
  })

  it('treats a blank/whitespace name as no name', () => {
    expect(collectionLabel({ name: '   ' }, false)).toBe('Collection')
  })

  it('is defensive against non-object docs', () => {
    expect(collectionLabel(null, false)).toBe('Collection')
    expect(collectionLabel('nope', false)).toBe('Collection')
  })
})
