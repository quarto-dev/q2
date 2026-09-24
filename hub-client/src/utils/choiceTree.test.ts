/**
 * buildChoiceTree (bd-q33ylfxf): turn the flat project-choice list into the
 * tree the New menu renders, keyed on each choice's hierarchical `path`.
 * Registry order is preserved within a node and nodes appear in order of
 * first appearance. Choices with no path are roots.
 */
import { describe, it, expect } from 'vitest';
import { buildChoiceTree, type ChoiceLike } from './choiceTree';

const c = (id: string, path?: string[]): ChoiceLike => ({ id, name: id, description: '', path });

describe('buildChoiceTree', () => {
  it('groups one-level paths into nodes in order of first appearance', () => {
    const tree = buildChoiceTree([
      c('default', ['Templates']),
      c('website', ['Templates']),
      c('welcome', ['Examples']),
      c('blog', ['Templates']),
      c('example-website', ['Examples']),
    ]);
    expect(tree.roots).toEqual([]);
    expect(tree.nodes.map((n) => n.label)).toEqual(['Templates', 'Examples']);
    expect(tree.nodes[0].choices.map((x) => x.id)).toEqual(['default', 'website', 'blog']);
    expect(tree.nodes[1].choices.map((x) => x.id)).toEqual(['welcome', 'example-website']);
    expect(tree.nodes[0].children).toEqual([]);
  });

  it('keeps choices with no path as roots, ahead of the nodes', () => {
    const tree = buildChoiceTree([c('a'), c('b', ['Group']), c('d', [])]);
    expect(tree.roots.map((x) => x.id)).toEqual(['a', 'd']);
    expect(tree.nodes.map((n) => n.label)).toEqual(['Group']);
  });

  it('nests deeper paths as child nodes', () => {
    const tree = buildChoiceTree([
      c('default', ['Templates']),
      c('deck', ['Templates', 'Decks']),
      c('lightning', ['Templates', 'Decks', 'Short']),
      c('site', ['Templates', 'Sites']),
    ]);
    const templates = tree.nodes[0];
    expect(templates.choices.map((x) => x.id)).toEqual(['default']);
    expect(templates.children.map((n) => n.label)).toEqual(['Decks', 'Sites']);
    const decks = templates.children[0];
    expect(decks.choices.map((x) => x.id)).toEqual(['deck']);
    expect(decks.children[0].label).toBe('Short');
    expect(decks.children[0].choices.map((x) => x.id)).toEqual(['lightning']);
  });

  it('lets two choices share a name when they live in different nodes', () => {
    const tree = buildChoiceTree([
      { id: 'website', name: 'Website', description: 'skeleton', path: ['Templates'] },
      { id: 'example-website', name: 'Website', description: 'populated', path: ['Examples'] },
    ]);
    expect(tree.nodes[0].choices[0].name).toBe('Website');
    expect(tree.nodes[1].choices[0].name).toBe('Website');
    expect(tree.nodes[0].choices[0].id).not.toBe(tree.nodes[1].choices[0].id);
  });

  it('returns an empty tree for no choices', () => {
    expect(buildChoiceTree([])).toEqual({ roots: [], nodes: [] });
  });
});
