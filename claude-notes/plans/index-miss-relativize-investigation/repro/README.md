# Repro: index-miss href relativization

Fixture for bd-tef2lm9j (nav hrefs to static files) and
bd-root-absolute-dir-link-58eh8834 (body links to directories). Render
with:

```bash
cargo run --bin q2 -- render claude-notes/plans/index-miss-relativize-investigation/repro
```

then inspect `_site/deep/deeper/index.html`.

Before the fix (q2 ≤ 0.21.0, main @ 3ac596e0), from the deep page:

| construct | emitted | correct |
|---|---|---|
| navbar `href: assets/report.pdf` | `assets/report.pdf` | `../../assets/report.pdf` |
| `[dir slash](/section/)` | `/section/` | `../../section/` |
| `[dir bare](/section)` | `/section` | `../../section` |
| `[root](/index.qmd)` (control) | `../../index.html` | ✓ already correct |

After the fix all four emit the page-relative form (trailing slash
preserved on the directory link). Note `/section/index.md` here is an
index *miss* (this fixture doesn't opt `.md` into the render list), so
it now emits `../../section/index.md` — relativized as a static
resource, silently, per bd-6d2wj4zp D6.

The original four-row repro against Quarto 1 lives in the Connect-docs
porting project
(`.../q2-connect-docs/llms-info/repros/root-absolute-dir-link/`); this
in-tree copy exists so the e2e check is reproducible from this repo
alone.
