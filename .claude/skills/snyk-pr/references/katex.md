# KaTeX playbook

KaTeX's version is deliberately coupled across **three surfaces** so `q2 render`
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
   colleague's next install produces a dirty tree. Normalize it by running
   `npm install` inside `hub-client/quarto-hub-sandboxed-preview/` (what the
   postinstall does), then commit the one-line lockfile delta.

3. **`DEFAULT_KATEX_URL_BASE`** in
   `crates/quarto-core/src/stage/stages/math_js.rs`:

   ```rust
   pub const DEFAULT_KATEX_URL_BASE: &str = "https://cdn.jsdelivr.net/npm/katex@X.Y.Z/dist/";
   ```

**Formerly a fourth surface:** `hub-client/public/q2-sandboxed-preview.html`,
a committed single-file bundle with KaTeX inlined (the surface PR #571 missed,
repaired in PR #573). Since `5684cfead` (2026-09-01) the sandboxed preview
builds into the gitignored `hub-client/public/q2-sandboxed-preview/` and is
deployed by `.github/workflows/deploy-sandboxed-preview.yml`, so there is no
committed bundle to regenerate. If a committed copy ever reappears, add it back
here.

## Verification

```bash
cargo nextest run -p quarto-core -E 'test(katex_cdn_version_matches_npm_pin)'

# no stray old-version pins anywhere
grep -rn '"katex":' --include='*.json' . | grep -v node_modules | grep -v '\.worktrees'

# dirty-tree trap: fresh root install must leave the tree clean
npm install && git status --porcelain
```

Then the workspace battery from the main skill (Rust changed → workspace build
+ nextest; hub-client changed → `npm run build:all` + changelog two-commit
workflow — the sandboxed-preview lockfile lives under `hub-client/`, so the
changelog always applies here).

## Reference commits

- `ccaa8cc9` (PR #634) — the complete playbook in one commit, with rationale.
- `3642d362` (PR #571) / `c0958658` (PR #471) — earlier partial fixes
  (surfaces 1+3 only); #571 missed the since-removed committed bundle.
- PR #711 (katex 0.18.4→0.18.5) — first remediation after the committed
  bundle was removed; surfaces 1–3 only.
