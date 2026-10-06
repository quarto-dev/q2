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

/** A registry description of one group, keyed by its full path. */
export interface ChoiceGroupLike {
  path: string[];
  description: string;
}

export interface ChoiceTreeNode<C extends ChoiceLike = ChoiceLike> {
  /** The group label at this level of the path. */
  label: string;
  /** The registry's explanation of the group, when it has one. */
  description?: string;
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

export function buildChoiceTree<C extends ChoiceLike>(
  choices: C[],
  groups: ChoiceGroupLike[] = [],
): ChoiceTree<C> {
  const describe = (path: string[]): string | undefined =>
    groups.find((g) => g.path.length === path.length && g.path.every((s, i) => s === path[i]))
      ?.description;
  const tree: ChoiceTree<C> = { roots: [], nodes: [] };
  for (const choice of choices) {
    const path = choice.path ?? [];
    if (path.length === 0) {
      tree.roots.push(choice);
      continue;
    }
    let level = tree.nodes;
    let node: ChoiceTreeNode<C> | undefined;
    for (let depth = 0; depth < path.length; depth += 1) {
      const label = path[depth];
      node = level.find((n) => n.label === label);
      if (!node) {
        node = { label, choices: [], children: [] };
        const description = describe(path.slice(0, depth + 1));
        if (description) node.description = description;
        level.push(node);
      }
      level = node.children;
    }
    node!.choices.push(choice);
  }
  return tree;
}
