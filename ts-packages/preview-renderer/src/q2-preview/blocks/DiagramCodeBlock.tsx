import type { CodeBlock as CodeBlockType, NodeArgs } from '../../framework';
import { GraphvizCodeBlock } from './GraphvizCodeBlock';
import { MermaidCodeBlock } from './MermaidCodeBlock';

/**
 * bd-ijlapt70 (prototype): the registry's `CodeBlock` entry.
 *
 * Picks the diagram renderer by fence class: ```dot goes to Graphviz,
 * everything else goes to the mermaid-aware wrapper, which in turn
 * delegates non-mermaid blocks to the plain built-in `CodeBlock`.
 * User `render-components` overrides of `CodeBlock` still win over
 * this component via the `mergedPreviewRegistry` layering.
 */
export const DiagramCodeBlock = (args: NodeArgs<CodeBlockType>) => {
    const classes = args.node.c[0][1];
    return classes.includes('dot') ? <GraphvizCodeBlock {...args} /> : <MermaidCodeBlock {...args} />;
};
