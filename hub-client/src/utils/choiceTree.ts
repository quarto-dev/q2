/**
 * Build the tree the "＋ New" menu renders from the flat project-choice
 * registry (bd-q33ylfxf).
 *
 * Each choice carries a hierarchical `path`, an array of group labels set
 * by the Rust registry (`["Templates"]`, `["Examples"]`, or deeper). A
 * choice with no path is a root item. Nodes appear in order of first
 * appearance and choices keep registry order within a node, so the menu
 * mirrors the registry exactly and needs no ordering of its own.
 */

/** The slice of a project choice this module reads. */
export interface ChoiceLike {
  id: string;
  name: string;
  description: string;
  path?: string[];
  seed?: boolean;
}

export interface ChoiceTreeNode<C extends ChoiceLike = ChoiceLike> {
  /** The group label at this level of the path. */
  label: string;
  /** Choices whose path ends exactly here, in registry order. */
  choices: C[];
  /** Deeper groups, in order of first appearance. */
  children: ChoiceTreeNode<C>[];
}

export interface ChoiceTree<C extends ChoiceLike = ChoiceLike> {
  /** Choices with no path, in registry order. */
  roots: C[];
  /** Top-level groups, in order of first appearance. */
  nodes: ChoiceTreeNode<C>[];
}

export function buildChoiceTree<C extends ChoiceLike>(choices: C[]): ChoiceTree<C> {
  const tree: ChoiceTree<C> = { roots: [], nodes: [] };
  for (const choice of choices) {
    const path = choice.path ?? [];
    if (path.length === 0) {
      tree.roots.push(choice);
      continue;
    }
    let level = tree.nodes;
    let node: ChoiceTreeNode<C> | undefined;
    for (const label of path) {
      node = level.find((n) => n.label === label);
      if (!node) {
        node = { label, choices: [], children: [] };
        level.push(node);
      }
      level = node.children;
    }
    node!.choices.push(choice);
  }
  return tree;
}
