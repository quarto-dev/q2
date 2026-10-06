export type { SearchProvider, SearchResult, SearchOptions } from './types';
export { InMemorySearchProvider } from './inMemorySearchProvider';
export { useProjectSearch } from './useProjectSearch';
export type { SearchFiles } from './useProjectSearch';
export { buildSnippet, findFirstMatch } from './snippet';
export type { SnippetSegment, SnippetOptions, MatchRange } from './snippet';
