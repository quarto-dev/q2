import type { NodeArgs, StrInline } from '@quarto/preview-renderer/framework';

export const Str = ({ node }: NodeArgs<StrInline>) => <>{node.c}</>;
