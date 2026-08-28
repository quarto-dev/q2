import { createContext, useContext, useState, useCallback, useEffect, type ReactNode } from 'react';

export type ViewMode = 'both' | 'markup' | 'preview';

interface ViewModeContextType {
  viewMode: ViewMode;
  setViewMode: (mode: ViewMode) => void;
  /** Navigate left: preview -> both -> markup */
  goLeft: () => void;
  /** Navigate right: markup -> both -> preview */
  goRight: () => void;
}

const ViewModeContext = createContext<ViewModeContextType | null>(null);

const STORAGE_KEY = 'qh-view-mode';

export function ViewModeProvider({ children }: { children: ReactNode }) {
  const [viewMode, setViewModeState] = useState<ViewMode>(() => {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === 'markup' || saved === 'preview' || saved === 'both') {
      return saved;
    }
    return 'both';
  });

  // Persist to localStorage
  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, viewMode);
  }, [viewMode]);

  const setViewMode = useCallback((mode: ViewMode) => {
    setViewModeState(mode);
  }, []);

  const goLeft = useCallback(() => {
    setViewModeState((current) => {
      switch (current) {
        case 'preview': return 'both';
        case 'both': return 'markup';
        case 'markup': return 'markup'; // Already at leftmost
      }
    });
  }, []);

  const goRight = useCallback(() => {
    setViewModeState((current) => {
      switch (current) {
        case 'markup': return 'both';
        case 'both': return 'preview';
        case 'preview': return 'preview'; // Already at rightmost
      }
    });
  }, []);

  return (
    <ViewModeContext.Provider value={{ viewMode, setViewMode, goLeft, goRight }}>
      {children}
    </ViewModeContext.Provider>
  );
}

export function useViewMode(): ViewModeContextType {
  const context = useContext(ViewModeContext);
  if (!context) {
    throw new Error('useViewMode must be used within a ViewModeProvider');
  }
  return context;
}
