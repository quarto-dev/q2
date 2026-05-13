import { Node } from '@quarto/preview-renderer/framework';
import type { BlockNode, InlineNode, LineBlockBlock, NodeArgs } from '@quarto/preview-renderer/framework';

/**
 * LineBlock → `<div class="line-block">` with each line as a `<div>`
 * containing its inlines. Pandoc HTML writer produces this exact shape;
 * theme CSS targets `.line-block`.
 */
export const LineBlock = (args: NodeArgs<LineBlockBlock>) => {
    const { node, setLocalAst, onNavigateToDocument } = args;
    return (
        <div className="line-block">
            {node.c.map((line, i) => (
                <div key={i}>
                    {line.map((inl, j) => (
                        <Node
                            key={j}
                            node={inl}
                            onNavigateToDocument={onNavigateToDocument}
                            setLocalAst={(newInl: BlockNode | InlineNode) => {
                                const newLine = line.slice();
                                newLine[j] = newInl as InlineNode;
                                const newLines = node.c.slice();
                                newLines[i] = newLine;
                                setLocalAst({ t: 'LineBlock', c: newLines });
                            }}
                        />
                    ))}
                </div>
            ))}
        </div>
    );
};
