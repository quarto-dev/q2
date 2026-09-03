# KaTeX playbook

KaTeX's version is deliberately coupled across **four surfaces** so `q2 render`
and the preview surfaces can never render math differently. The coupling is
enforced by `katex_cdn_version_matches_npm_pin`
(`crates/quarto-core/src/stage/stages/math_js.rs`, ~line 1021; strand
bd-4b7f1hr7). Snyk bumps only surface 2 — every katex Snyk PR arrives red.

## The four surfaces

1. **Root `package.json`** — `"katex": "X.Y.Z"` (exact pin, no caret) + root
   `package-lock.json`. Bump from the **repo root**:

   ```bash
   npm install katex@X.Y.Z --save-exact
   ```

2. **`hub-client/quarto-hub-sandboxed-preview/package.json`** + its
   `package-lock.json`. Snyk bumps this pair, **but writes `^X.Y.Z` into the
   lockfile's root dependency mirror** while package.json says `X.Y.Z`.
   hub-client's postinstall runs `npm install` in this sub-project, which
   rewrites the caret away — merging without normalizing means every
   colleague's next install produces a dirty tree. Normalizing happens as a
   side effect of the bundle rebuild (surface 4); commit the lockfile delta.

3. **`DEFAULT_KATEX_URL_BASE`** in
   `crates/quarto-core/src/stage/stages/math_js.rs`:

   ```rust
   pub const DEFAULT_KATEX_URL_BASE: &str = "https://cdn.jsdelivr.net/npm/katex@X.Y.Z/dist/";
   ```

4. **`hub-client/public/q2-sandboxed-preview.html`** — a **committed** ~1.8 MB
   single-file bundle with KaTeX inlined. The guard test checks the three
   version *declarations* above, not these bytes, and (as of 2026-09) there is
   no freshness gate for this artifact. This is the surface PR #571 missed
   (repaired in follow-up PR #573). Regenerate:

   ```bash
   cd hub-client && npm run build:sandboxed
   ```

   The rebuild is deterministic. Inspect the diff: a pure version bump changes
   only the embedded version strings (~2 bytes). A larger delta means the
   bundle was already stale — still commit it, but say so explicitly in the
   commit message and to the user.

## Verification

```bash
cargo nextest run -p quarto-core -E 'test(katex_cdn_version_matches_npm_pin)'

# every embedded version string in the bundle is the new one
grep -o 'X\.Y\.[0-9]*' hub-client/public/q2-sandboxed-preview.html | sort | uniq -c

# no stray old-version pins anywhere
grep -rn '"katex":' --include='*.json' . | grep -v node_modules | grep -v '\.worktrees'

# dirty-tree trap: fresh root install must leave the tree clean
npm install && git status --porcelain
```

Then the workspace battery from the main skill (Rust changed → workspace build
+ nextest; hub-client changed → `npm run build:all` + changelog two-commit
workflow — the regenerated bundle lives under `hub-client/`, so the changelog
always applies here).

## Reference commits

- `ccaa8cc9` (PR #634) — the complete playbook in one commit, with rationale.
- `3642d362` (PR #571) / `c0958658` (PR #471) — earlier partial fixes
  (surfaces 1+3 only); #571's miss of surface 4 is why this file lists it.
