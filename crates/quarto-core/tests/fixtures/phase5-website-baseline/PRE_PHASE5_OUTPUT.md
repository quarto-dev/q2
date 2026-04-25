# Phase 5 Website Baseline

Captured 2026-04-24 against commit `7881178e` (pre-Phase-5 refactor).
Documents the pre-Phase-5 output shape so we can verify the *intentional*
diff after the refactor (single-doc behavior is identity-tested
separately in `phase5-single-doc-baseline/`).

## Pre-Phase-5 file layout

```
_site/
├── about.html
├── about_files/
│   └── styles.css        (sha256 3536a93eba680c9bd74acd4efba42fcd643981b1bc9d4128a3c09c8b278bbd15)
├── docs/
│   ├── api.html
│   └── api_files/
│       └── styles.css    (sha256 3536a93eba680c9bd74acd4efba42fcd643981b1bc9d4128a3c09c8b278bbd15)
├── index.html
└── index_files/
    └── styles.css        (sha256 3536a93eba680c9bd74acd4efba42fcd643981b1bc9d4128a3c09c8b278bbd15)
```

Three identical 312530-byte CSS files (note the matching sha256). This is
exactly the duplication Phase 5 eliminates.

## Pre-Phase-5 `<link>` hrefs

| Page | href |
|------|------|
| `index.html` | `index_files/styles.css` |
| `about.html` | `about_files/styles.css` |
| `docs/api.html` | `api_files/styles.css` |

## Post-Phase-5 expected layout

After the refactor, the same fixture should produce:

```
_site/
├── about.html
├── docs/
│   └── api.html
├── index.html
└── site_libs/
    └── quarto/
        └── quarto-theme-<HASH>.css   (one shared copy)
```

## Post-Phase-5 expected `<link>` hrefs

| Page | href |
|------|------|
| `index.html` | `site_libs/quarto/quarto-theme-<HASH>.css` |
| `about.html` | `site_libs/quarto/quarto-theme-<HASH>.css` |
| `docs/api.html` | `../site_libs/quarto/quarto-theme-<HASH>.css` |

The `<HASH>` is the theme fingerprint (Decision 9). Per-page
`{stem}_files/` directories are still emitted today even when empty;
empty-dir cleanup is deferred (Open question 5 / follow-up).

## Notes on out-of-scope hrefs

The pre-Phase-5 sidebar and page-nav `href`s in `docs/api.html`
point at bare `index.html`, `about.html` (without `../`), which is
incorrect from a nested page. Phase 6's body-link rewriter fixes
this; Phase 5 is concerned only with `<link>` / `<script>` tags
in `<head>`.
