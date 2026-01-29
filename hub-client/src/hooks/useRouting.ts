/**
 * React hook for URL-based routing in hub-client.
 *
 * This hook manages:
 * - Parsing the initial URL on mount
 * - Listening for hashchange events (browser back/forward)
 * - Providing functions to update the URL
 *
 * URL Scheme:
 *   #/                                    → Project selector
 *   #/project/<local-id>                  → Project with default file
 *   #/project/<local-id>/file/<path>      → Specific file
 *   #/project/<local-id>/file/<path>#<a>  → Specific file + anchor
 */
import { useState, useEffect, useCallback, useRef } from 'react';
import {
  type Route,
  type FileRoute,
  parseHashRoute,
  updateUrl,
  routesEqual,
} from '../utils/routing';

export interface UseRoutingOptions {
  /**
   * Called when the route changes due to browser navigation (back/forward).
   * This is NOT called for programmatic route changes via navigateTo*.
   */
  onRouteChange?: (route: Route) => void;
}

export interface UseRoutingResult {
  /** The current route parsed from the URL */
  route: Route;

  /**
   * Navigate to the project selector.
   * @param options.replace - If true, don't add to browser history
   */
  navigateToProjectSelector: (options?: { replace?: boolean }) => void;

  /**
   * Navigate to a project (with default file).
   * @param projectId - The local IndexedDB project ID
   * @param options.replace - If true, don't add to browser history
   */
  navigateToProject: (projectId: string, options?: { replace?: boolean }) => void;

  /**
   * Navigate to a specific file within the current project.
   * @param filePath - The file path (e.g., "index.qmd" or "docs/intro.qmd")
   * @param options.anchor - Optional section anchor
   * @param options.replace - If true, don't add to browser history
   */
  navigateToFile: (
    projectId: string,
    filePath: string,
    options?: { anchor?: string; replace?: boolean }
  ) => void;

  /**
   * Update just the anchor in the current route (for scroll sync, etc).
   * Only works if currently on a file route.
   * Always uses replaceState to avoid polluting history.
   */
  updateAnchor: (anchor: string | undefined) => void;
}

/**
 * Hook for managing URL-based routing.
 */
export function useRouting(options: UseRoutingOptions = {}): UseRoutingResult {
  const { onRouteChange } = options;

  // Parse initial route from URL
  const [route, setRoute] = useState<Route>(() => parseHashRoute(window.location.hash));

  // Keep a ref to the callback to avoid re-registering the listener
  const onRouteChangeRef = useRef(onRouteChange);
  useEffect(() => {
    onRouteChangeRef.current = onRouteChange;
  }, [onRouteChange]);

  // Listen for hashchange events (browser back/forward)
  useEffect(() => {
    const handleHashChange = () => {
      const newRoute = parseHashRoute(window.location.hash);
      setRoute((prevRoute) => {
        // Only update if route actually changed (avoid unnecessary re-renders)
        if (routesEqual(prevRoute, newRoute)) {
          return prevRoute;
        }
        // Notify callback of browser-initiated navigation
        onRouteChangeRef.current?.(newRoute);
        return newRoute;
      });
    };

    window.addEventListener('hashchange', handleHashChange);
    return () => window.removeEventListener('hashchange', handleHashChange);
  }, []);

  // Also listen for popstate (handles some edge cases)
  useEffect(() => {
    const handlePopState = () => {
      const newRoute = parseHashRoute(window.location.hash);
      setRoute((prevRoute) => {
        if (routesEqual(prevRoute, newRoute)) {
          return prevRoute;
        }
        onRouteChangeRef.current?.(newRoute);
        return newRoute;
      });
    };

    window.addEventListener('popstate', handlePopState);
    return () => window.removeEventListener('popstate', handlePopState);
  }, []);

  const navigateToProjectSelector = useCallback((opts?: { replace?: boolean }) => {
    const newRoute: Route = { type: 'project-selector' };
    updateUrl(newRoute, { replace: opts?.replace });
    setRoute(newRoute);
  }, []);

  const navigateToProject = useCallback((projectId: string, opts?: { replace?: boolean }) => {
    const newRoute: Route = { type: 'project', projectId };
    updateUrl(newRoute, { replace: opts?.replace });
    setRoute(newRoute);
  }, []);

  const navigateToFile = useCallback((
    projectId: string,
    filePath: string,
    opts?: { anchor?: string; replace?: boolean }
  ) => {
    const newRoute: FileRoute = {
      type: 'file',
      projectId,
      filePath,
      ...(opts?.anchor && { anchor: opts.anchor }),
    };
    updateUrl(newRoute, { replace: opts?.replace });
    setRoute(newRoute);
  }, []);

  const updateAnchor = useCallback((anchor: string | undefined) => {
    setRoute((prevRoute) => {
      if (prevRoute.type !== 'file') {
        // Can only update anchor on file routes
        return prevRoute;
      }

      const newRoute: FileRoute = {
        ...prevRoute,
        anchor,
      };

      // Always replace (don't add history entries for anchor changes)
      updateUrl(newRoute, { replace: true });
      return newRoute;
    });
  }, []);

  return {
    route,
    navigateToProjectSelector,
    navigateToProject,
    navigateToFile,
    updateAnchor,
  };
}
