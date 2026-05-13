import type { FormatRegistry } from '@quarto/preview-renderer/framework';
import { Block, Inline } from './dispatchers';
import {
    BlockComponents,
    InlineComponents,
    AstRenderer,
} from './components';

export { Block, Inline };
export { BlockComponents, InlineComponents };

/**
 * q2-debug format registry. The framework reserves the keys 'Ast',
 * 'Block', and 'Inline'; each format must register all three. q2-debug's
 * Block/Inline are the bordered dispatchers; 'Ast' is the bordered
 * document-root wrapper.
 */
export const q2DebugRegistry: FormatRegistry = {
    ...BlockComponents,
    ...InlineComponents,
    Block,
    Inline,
    Ast: AstRenderer,
};
