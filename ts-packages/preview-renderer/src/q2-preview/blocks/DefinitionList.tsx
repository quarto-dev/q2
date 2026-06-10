import React, { useContext } from 'react';
import { Node } from '../../framework';
import type {
    BlockNode,
    DefinitionListBlock,
    InlineNode,
    NodeArgs,
} from '../../framework';
import { PreviewContext } from '../PreviewContext';

/**
 * DefinitionList → `<dl><dt>term</dt><dd>def</dd>...</dl>`. Pandoc
 * writes one `<dt>` per term and one `<dd>` per definition; multiple
 * definitions for the same term yield multiple sibling `<dd>`s
 * (no wrapping element per item, matching Pandoc's HTML writer).
 *
 * AST shape: `c: [InlineNode[], BlockNode[][]][]` — each item is a
 * (term, definitions) pair where definitions is an array of block
 * arrays (each block array is one definition).
 */
export const DefinitionList = (args: NodeArgs<DefinitionListBlock>) => {
    const ctx = useContext(PreviewContext);
    const poolId = (args.node as any).s as string | number | undefined;
    const resolved = ctx?.resolveSource ? ctx.resolveSource(args.node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;
    const affordanceAttr = isEditable ? { 'data-block-pool-id': poolId } : {};
    const { node, setLocalAst, onNavigateToDocument } = args;
    return (
        <dl {...affordanceAttr}>
            {node.c.flatMap(([term, defs], i) => {
                const dt = (
                    <dt key={`dt-${i}`}>
                        {term.map((inl, j) => (
                            <Node
                                key={j}
                                node={inl}
                                onNavigateToDocument={onNavigateToDocument}
                                setLocalAst={(newInl: BlockNode | InlineNode) => {
                                    const newTerm = term.slice();
                                    newTerm[j] = newInl as InlineNode;
                                    const newItems = node.c.slice();
                                    newItems[i] = [newTerm, defs];
                                    setLocalAst({ t: 'DefinitionList', c: newItems });
                                }}
                            />
                        ))}
                    </dt>
                );
                const dds = defs.map((blocks, k) => (
                    <dd key={`dd-${i}-${k}`}>
                        {blocks.map((b, m) => (
                            <Node
                                key={m}
                                node={b}
                                onNavigateToDocument={onNavigateToDocument}
                                setLocalAst={(newBlock: BlockNode | InlineNode) => {
                                    const newBlocks = blocks.slice();
                                    newBlocks[m] = newBlock as BlockNode;
                                    const newDefs = defs.slice();
                                    newDefs[k] = newBlocks;
                                    const newItems = node.c.slice();
                                    newItems[i] = [term, newDefs];
                                    setLocalAst({ t: 'DefinitionList', c: newItems });
                                }}
                            />
                        ))}
                    </dd>
                ));
                return [dt, ...dds] as React.ReactNode[];
            })}
        </dl>
    );
};
