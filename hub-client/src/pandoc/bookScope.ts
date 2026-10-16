// The per-request metadata that asks for a book scope (FR5, FR13). The wire carries a generic
// `metadata` object, not a scope field: the server or the wasm entry turns it into the render's
// runtime layer, and Rust's one interpreter of `book-scope` reads it back. Nothing here decides a scope.

/** The two scopes a client can ask for. */
export type BookScope = 'book' | 'chapter';

/** A render's runtime-layer metadata, as the routes and the wasm entry take it. */
export type RenderMetadata = Record<string, unknown>;

/** The metadata key Rust reads (`BOOK_SCOPE_KEY`). */
export const BOOK_SCOPE_KEY = 'book-scope';

/** `{ 'book-scope': scope }`, or `undefined` for "the user did not choose" (the target's default applies). */
export function bookScopeMetadata(scope: BookScope | undefined): RenderMetadata | undefined {
  return scope ? { [BOOK_SCOPE_KEY]: scope } : undefined;
}

/** The explicit scope a metadata object asks for, if any; for the client's own capture decisions. */
export function requestedBookScope(metadata: RenderMetadata | undefined): BookScope | undefined {
  const value = metadata?.[BOOK_SCOPE_KEY];
  return value === 'book' || value === 'chapter' ? value : undefined;
}
