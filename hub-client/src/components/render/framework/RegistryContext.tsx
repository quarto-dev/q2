import { createContext } from 'react';
import type { SourceInfoPool } from '@quarto/preview-renderer/types/sourceInfo';

/**
 * Context that carries the active format's registry to the dispatchers,
 * along with the optional source-info pool from the serialized AST.
 *
 * The default value is an empty registry rather than `null`. Dispatchers
 * read `useContext(RegistryContext).registry` directly. In normal flow the
 * <Ast> component above the dispatchers replaces the default with the
 * format's full registry; the empty default applies only if a dispatcher
 * is mounted outside an <Ast> ancestor (no current consumer does this).
 *
 * `sourceInfoPool` is optional — q2-debug doesn't read it today, but
 * Plan 2B's atomic-aware gate (in `dispatch.tsx`'s `Node`) will consume
 * it to no-op `setLocalAst` over atomic content. Both formats benefit
 * automatically once the gate lands.
 */
export const RegistryContext = createContext<{
    registry: Record<string, (props: any) => React.ReactNode>;
    sourceInfoPool?: SourceInfoPool;
}>({ registry: {} });
