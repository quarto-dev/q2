- Try hard to avoid "TODO" comments in the code base. If are running low on context and you do have to add it, make sure there's a beads task (even if low-priority) to track the TODO, and add the issue id to the TODO line.

## hub-client (TypeScript/React)

When making changes to `hub-client/`:

1. **After making TypeScript changes**, run preflight checks:
   ```bash
   cd hub-client && npm run preflight
   ```
   This builds WASM and type-checks with Vite-compatible settings.

2. **Type imports**: Use `import type` for type-only imports (interfaces, type aliases). Vite's esbuild transformer requires this due to `verbatimModuleSyntax: true`.
   ```typescript
   // Correct
   import { useCallback } from 'react';
   import type { RefObject } from 'react';

   // Wrong - will fail at runtime in Vite
   import { useCallback, RefObject } from 'react';
   ```

3. **Don't use plain `tsc --noEmit`** - it uses different settings and misses errors. Always use `npm run typecheck` or `npm run preflight`.
