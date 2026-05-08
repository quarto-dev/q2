import { createContext } from 'react';

/**
 * Context that carries the active format's registry to the dispatchers.
 *
 * The default value is an empty registry rather than `null`. Dispatchers
 * read `useContext(RegistryContext).registry` directly. In normal flow the
 * <Ast> component above the dispatchers replaces the default with the
 * format's full registry; the empty default applies only if a dispatcher
 * is mounted outside an <Ast> ancestor (no current consumer does this).
 */
export const RegistryContext = createContext<{
    registry: Record<string, (props: any) => React.ReactNode>;
}>({ registry: {} });
