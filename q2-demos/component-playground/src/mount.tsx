import { StrictMode, type ReactNode } from 'react';
import { createRoot } from 'react-dom/client';

/**
 * Render a page into #root. No hub-client stylesheet or provider is added
 * here: each page brings what it needs.
 */
export function mount(page: ReactNode): void {
  createRoot(document.getElementById('root')!).render(<StrictMode>{page}</StrictMode>);
}
