import type { NodeArgs, StrInline } from '../../framework';

export const Str = ({ node }: NodeArgs<StrInline>) => <>{node.c}</>;
