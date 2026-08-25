# Custom Listing Template Not Templated (Q-12-24) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `type: custom` listing whose template is not a doctemplate (a Quarto 1 EJS file, or any file with no `$` directives) warns with a new code Q-12-24 and skips the listing, instead of splicing the raw file into the page silently; Q-12-9 also catches plain `.ejs`; the docs stop claiming Q2 listing templates are EJS and gain a real custom-template section.

**Architecture:** The check is an AST inspection after `Template::compile_with_resolver` succeeds — `Template::nodes()` is already public, so "no directive" is `all nodes are Literal` — plus a `<%` source sniff for half-ported files. It lives in `crates/quarto-core/src/transforms/listing_render.rs` on the custom-template path only, between compile and render, and returns `None` so the existing "skip the listing" path (used by Q-12-10 compile errors) does the rest. Q-12-9 is one widened condition in `config.rs`. Docs are catalog + error pages + a new section in the Listings guide.

**Tech Stack:** Rust (`quarto-core`, `quarto-doctemplate`, `quarto-error-catalog`), Quarto docs site under `docs/` (rendered with `cargo run --bin q2 -- render docs/`, never Q1).

**Spec:** The braid strand bd-custom-template-not-templated-e5t6m0i0 (supersedes bd-oywyaouf) plus the decisions below. No separate spec document — the "Investigation context" section at the end of this file is the argued root cause.

**Braid:** bd-custom-template-not-templated-e5t6m0i0 — parent bd-61cd (listings epic); related bd-hzsi (L10 migration docs + LLM skill), bd-u4ow (custom-template reference page), bd-lu16jgxq (Q-12-7 wording); supersedes bd-oywyaouf.

**Worktree:** `.worktrees/workspace-2`, branch `braid/bd-custom-template-not-templated-e5t6m0i0-custom-template-not-templated`, based on `origin/main` @ `05b6fd75c`. Pre-flight `cargo xtask verify --skip-hub-build --skip-hub-tests` green at that base: 13380 Rust tests passed, 199 skipped.

## Decisions (settled with Gordon, 2026-08-25)

1. **Degraded behaviour: skip the listing** (Q-12-10 precedent), *not* fall back to the default built-in. This reverses the fallback decision recorded on bd-oywyaouf.
2. **Severity: warning**, like every other `Q-12-*` (exit 0).
3. **Comment-only templates count as doctemplates.** A `$-- comment` proves the author wrote doctemplate syntax; only a template whose top-level nodes are *all* `Literal` is "untemplated".
4. **Real custom-template docs**: a "Custom templates" section in `docs/guides/projects/listings.qmd` (doctemplate is the *only* listing template language in Q2, existing or planned — listings epic settled decision #3).
5. **Q-12-9 reframed as "Quarto 1 EJS template"** (title, catalog message, config message, docs page) and widened to `.ejs`.
6. **`<%` content sniff** is a second trigger for Q-12-24, so a half-ported template (real `$` directives plus leftover `<%=`) also warns and skips.
7. **New code is Q-12-24** (Q-12-15, proposed on bd-oywyaouf, has since been taken).

## Global Constraints

- **TDD, non-negotiable** (repo `CLAUDE.md`): write the test, run it and see it fail, implement, see it pass. Never implement before the failing run.
- **`cargo nextest run`, never `cargo test`; never pipe nextest through `tail`.**
- Per-task gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` and `cargo nextest run -p quarto-core` (plus `-p xtask` / `cargo xtask lint` where a task touches the catalog or docs). The controller runs `cargo nextest run --workspace` at phase boundaries.
- **Error-code lints** (`cargo xtask lint`): every catalog code needs `docs/errors/<subsystem>/<code>.qmd` with `docs_url` `https://quarto.org/docs/errors/listing/<code>`, **and** a sidebar entry in `docs/_quarto.yml` inside the `listing` section, entries ascending by code number. Catalog entry + page + sidebar entry land in the **same commit**.
- Error page front matter: `title` must equal the catalog `title`; `since` must equal the catalog `since_version` (`"99.9.9"`); `subsystem: listing`; `categories: [listing]`; body follows the template in `docs/errors/README.md` (`# \`Q-X-Y\` — title`, `> description`, `## What this means`, `## Why this happens`, `## How to fix`, `## Related`).
- All `Q-12-*` diagnostics in `listing_render.rs` go through the existing `push_diag(diags, code, message)` (warning severity). In `config.rs` the existing four-arg `push_diag(diagnostics, code, message, &entry.value)` blames the YAML value.
- Diagnostic message wording (verbatim, so tests and docs agree):
  - Q-12-24, no directives: ``Listing `{id}`: template `{path}` contains no doctemplate directives (`$var$`, `$for(…)$`, `$if(…)$`), so it would be copied into the page unchanged. Quarto 2 custom listing templates use doctemplate syntax; see the Listings guide, “Custom templates”. Listing skipped.``
  - Q-12-24, `<%` present (whether or not directives are also present): ``Listing `{id}`: template `{path}` contains `<% … %>` markup, which is Quarto 1 EJS syntax; Quarto 2 does not evaluate EJS. Quarto 2 custom listing templates use doctemplate syntax; see the Listings guide, “Custom templates”. Listing skipped.``
  - Q-12-9 (config.rs): ``` `{path}` has a Quarto 1 EJS template extension (`.ejs` / `.ejs.md`); Quarto 2 listing templates use doctemplate syntax — see the Listings guide, “Custom templates”. ```
- Doctemplate binding names used in docs and tests: inside `$for(items)$` the current item is `$it$` **or** `$items$` (`$it.title$` ≡ `$items.title$`); custom record fields are readable both flattened (`$items.icon$`) and nested (`$items.extra.icon$`). The existing docs text `$item.key$` is **wrong** and is corrected in Task 4.
- The docs site is Quarto 2: verify docs changes with `cargo run --bin q2 -- render docs/` from the worktree root, never the system `quarto`.
- No `external-sources/` references in anything compiled or embedded.
- Cross-platform: tests use `tempfile` + `Path` joins; no hardcoded separators.

---

### Task 1: Q-12-24 — detect an untemplated custom template, warn, skip

**Files:**
- Modify: `crates/quarto-core/src/transforms/listing_render.rs` (the `render_one` Custom arm at ~`:187-200`; `compile_and_render` at ~`:453-497`; tests module from ~`:499`; the existing test `custom_template_with_ejs_md_extension_attempts_load_and_fails_compile` at ~`:1220-1253`)
- Modify: `crates/quarto-error-catalog/error_catalog.json` (add `Q-12-24` after `Q-12-23`)
- Create: `docs/errors/listing/Q-12-24.qmd`
- Modify: `docs/_quarto.yml` (sidebar: add `- errors/listing/Q-12-24.qmd` directly after the `Q-12-23.qmd` line, ~`:213`)

**Interfaces:**
- Consumes: `quarto_doctemplate::{Template, TemplateNode}` (both re-exported at the crate root; `Template::nodes(&self) -> &[TemplateNode]` is public), `LoadedCustomTemplate { source, template_path, resolver }`, `push_diag`.
- Produces: `fn compile_template<R: PartialResolver>(listing_id: &str, source: &str, template_path: &Path, resolver: &R, diags: &mut Vec<DiagnosticMessage>) -> Option<Template>`, `fn render_template(listing_id: &str, template: &Template, template_ctx: &TemplateContext, diags: &mut Vec<DiagnosticMessage>) -> Option<String>`, `fn custom_template_is_templated(listing_id: &str, custom: &LoadedCustomTemplate, template: &Template, diags: &mut Vec<DiagnosticMessage>) -> bool`. Task 3's e2e test relies on the code string `"Q-12-24"` and on the skipped listing leaving no `<%` in the output.

- [x] **Step 1: Write the failing tests**

Add to the `tests` module of `listing_render.rs`, next to the other `custom_template_*` tests (they use the existing helpers `custom_template_project`, `make_custom_listing`, `make_item`, `run_transform_at`, `empty_pandoc`):

```rust
    // bd-custom-template-not-templated-e5t6m0i0 test #1:
    // A `$`-free template wrapped in a raw-HTML block compiles as a
    // single literal and re-parses cleanly, so before the fix it was
    // spliced into the page verbatim with no diagnostic at all.
    #[tokio::test]
    async fn custom_template_without_directives_emits_q_12_24_and_skips_listing() {
        let (_tmp, root, host) = custom_template_project(
            "welcome-card.ejs",
            "```{=html}\n\
             <div class=\"untemplated-marker\">\n\
             <% for (const item of items) { %>\n\
             <a href=\"<%= item.link %>\"><%= item.title %></a>\n\
             <% } %>\n\
             </div>\n\
             ```\n",
        );
        let listing = make_custom_listing("welcome-card.ejs");
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        let q1224 = diags
            .iter()
            .find(|d| d.code.as_deref() == Some("Q-12-24"))
            .unwrap_or_else(|| panic!("expected Q-12-24; got: {diags:?}"));
        assert!(
            q1224.title.contains("welcome-card.ejs") && q1224.title.contains("Quarto 1 EJS"),
            "message must name the template and say EJS; got: {}",
            q1224.title
        );
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("Q-12-10")),
            "no compile/re-parse diagnostic expected; got: {diags:?}"
        );
        let serialized = format!("{:?}", ast);
        assert!(
            !serialized.contains("untemplated-marker") && !serialized.contains("<%"),
            "expected the raw template NOT to be spliced; got: {serialized}"
        );
        assert!(
            !serialized.contains("quarto-listing-default"),
            "expected skip, not default fallback; got: {serialized}"
        );
    }

    // test #2: a template with no `<%` and no `$` at all — plain
    // static markdown — is also untemplated.
    #[tokio::test]
    async fn custom_template_plain_static_text_emits_q_12_24() {
        let (_tmp, root, host) = custom_template_project(
            "static.template",
            "::: {.static-marker}\nNothing here varies.\n:::\n",
        );
        let listing = make_custom_listing("static.template");
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        let q1224 = diags
            .iter()
            .find(|d| d.code.as_deref() == Some("Q-12-24"))
            .unwrap_or_else(|| panic!("expected Q-12-24; got: {diags:?}"));
        assert!(
            q1224.title.contains("contains no doctemplate directives"),
            "got: {}",
            q1224.title
        );
        assert!(!format!("{:?}", ast).contains("static-marker"));
    }

    // test #3: a half-ported template — real `$` directives plus
    // leftover EJS — warns too (the sniff catches what the AST check
    // cannot) and is skipped.
    #[tokio::test]
    async fn custom_template_half_ported_with_ejs_markup_emits_q_12_24() {
        let (_tmp, root, host) = custom_template_project(
            "half.template",
            "::: {.half-ported-marker}\n$for(items)$\n- <%= item.title %> / $it.title$\n$endfor$\n:::\n",
        );
        let listing = make_custom_listing("half.template");
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        let q1224 = diags
            .iter()
            .find(|d| d.code.as_deref() == Some("Q-12-24"))
            .unwrap_or_else(|| panic!("expected Q-12-24; got: {diags:?}"));
        assert!(q1224.title.contains("Quarto 1 EJS"), "got: {}", q1224.title);
        assert!(!format!("{:?}", ast).contains("half-ported-marker"));
    }

    // test #4: a template whose only directive is a comment still
    // counts as a doctemplate (decision 3) — no Q-12-24, rendered.
    #[tokio::test]
    async fn custom_template_with_only_a_comment_directive_is_not_flagged() {
        let (_tmp, root, host) = custom_template_project(
            "commented.template",
            "$-- deliberately static\n::: {.commented-marker}\nStatic body.\n:::\n",
        );
        let listing = make_custom_listing("commented.template");
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("Q-12-24")),
            "comment-only template must not be flagged; got: {diags:?}"
        );
        assert!(format!("{:?}", ast).contains("commented-marker"));
    }

    // test #5: a normal directive-bearing template is untouched by
    // the new check (regression guard for the refactor).
    #[tokio::test]
    async fn custom_template_with_directives_is_not_flagged() {
        let (_tmp, root, host) = custom_template_project(
            "cards.template",
            "::: {.cards-marker}\n$for(items)$\n- $it.title$\n$endfor$\n:::\n",
        );
        let listing = make_custom_listing("cards.template");
        let items = vec![make_item("Hello", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("Q-12-24")),
            "got: {diags:?}"
        );
        let serialized = format!("{:?}", ast);
        assert!(serialized.contains("cards-marker") && serialized.contains("Hello"));
    }
```

Then **replace** the existing test `custom_template_with_ejs_md_extension_attempts_load_and_fails_compile` (its comment claims a compile failure, but bare EJS compiles fine and only trips the *re-parse* — after this task the check fires first) with:

```rust
    // L8 / bd-rqgx test #14, revised for
    // bd-custom-template-not-templated-e5t6m0i0:
    // A `.ejs.md` template (genuine EJS syntax) emits Q-12-9 at
    // parse-time (covered in config.rs) and, at render time, Q-12-24:
    // the EJS contains no `$`, so it *compiles* as one literal — the
    // untemplated check, not a compile error, is what catches it.
    // The listing is skipped.
    #[tokio::test]
    async fn custom_template_with_ejs_md_extension_emits_q_12_24_and_skips_listing() {
        let (_tmp, root, host) = custom_template_project(
            "legacy.ejs.md",
            "<ul>\n<% items.forEach(function(item) { %>\n  \
             <li><%= item.title %></li>\n<% }); %>\n</ul>\n",
        );
        let listing = make_custom_listing("legacy.ejs.md");
        let items = vec![make_item("a", Some("2026-01-01"))];
        let resolved = vec![ResolvedListing { listing, items }];
        let (ast, diags) = run_transform_at(empty_pandoc(), resolved, &root, &host).await;
        assert!(
            diags.iter().any(|d| d.code.as_deref() == Some("Q-12-24")),
            "expected Q-12-24; got: {diags:?}"
        );
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("Q-12-10")),
            "the check runs before render/re-parse, so no Q-12-10; got: {diags:?}"
        );
        assert!(!format!("{:?}", ast).contains("<%"));
    }
```

- [x] **Step 2: Run the tests and verify they fail**

Run: `cargo nextest run -p quarto-core custom_template_`
Expected: the five new tests and the revised `.ejs.md` test FAIL (`expected Q-12-24; got: []` or a Q-12-10 where none is expected); the other `custom_template_*` tests still pass.

- [x] **Step 3: Implement — split `compile_and_render`, add the check**

In `listing_render.rs`, add `TemplateNode` to the `quarto_doctemplate` import, then replace `compile_and_render` with:

```rust
/// Compile a doctemplate source. Compile errors surface as `Q-12-10`
/// and return `None`; the caller skips the listing.
fn compile_template<R: PartialResolver>(
    listing_id: &str,
    source: &str,
    template_path: &Path,
    resolver: &R,
    diags: &mut Vec<DiagnosticMessage>,
) -> Option<Template> {
    match Template::compile_with_resolver(source, template_path, resolver, 0) {
        Ok(t) => Some(t),
        Err(e) => {
            push_diag(
                diags,
                "Q-12-10",
                format!(
                    "Listing `{listing_id}` template failed to compile: {e:?}. \
                     Listing skipped."
                ),
            );
            None
        }
    }
}

/// Render a compiled template against `template_ctx`. Render /
/// diagnostic-channel errors surface as `Q-12-10` and return `None`;
/// the caller skips the listing.
fn render_template(
    listing_id: &str,
    template: &Template,
    template_ctx: &TemplateContext,
    diags: &mut Vec<DiagnosticMessage>,
) -> Option<String> {
    let (rendered, render_diags) = template.render_with_diagnostics(template_ctx);
    let markdown = match rendered {
        Ok(s) => s,
        Err(()) => {
            push_diag(
                diags,
                "Q-12-10",
                format!("Listing `{listing_id}` template rendering failed; listing skipped."),
            );
            return None;
        }
    };
    if !render_diags.is_empty() {
        push_diag(
            diags,
            "Q-12-10",
            format!(
                "Listing `{}` doctemplate produced {} diagnostic(s); first: {}",
                listing_id,
                render_diags.len(),
                render_diags[0].title
            ),
        );
    }
    Some(markdown)
}

/// Compile a doctemplate source and render it against `template_ctx`.
/// Used by the built-ins; the custom path inserts
/// [`custom_template_is_templated`] between the two halves.
fn compile_and_render<R: PartialResolver>(
    listing_id: &str,
    source: &str,
    template_path: &Path,
    resolver: &R,
    template_ctx: &TemplateContext,
    diags: &mut Vec<DiagnosticMessage>,
) -> Option<String> {
    let template = compile_template(listing_id, source, template_path, resolver, diags)?;
    render_template(listing_id, &template, template_ctx, diags)
}

/// `Q-12-24` guard (bd-custom-template-not-templated-e5t6m0i0): a
/// custom template that compiled but contains no doctemplate
/// directive at all — every top-level node is a `Literal` — would be
/// spliced into the page unchanged, and a template that still holds
/// Quarto 1 EJS `<% … %>` markup would leak it. The doctemplate
/// grammar's only special character is `$`, so both cases compile
/// "successfully"; this is the check the compiler cannot make.
///
/// Comments (`$-- …`) count as directives: they prove the author
/// wrote doctemplate syntax. Returns `false` (after emitting the
/// diagnostic) when the listing must be skipped.
fn custom_template_is_templated(
    listing_id: &str,
    custom: &LoadedCustomTemplate,
    template: &Template,
    diags: &mut Vec<DiagnosticMessage>,
) -> bool {
    let has_directives = template
        .nodes()
        .iter()
        .any(|n| !matches!(n, TemplateNode::Literal(_)));
    let has_ejs_markup = custom.source.contains("<%");
    if has_directives && !has_ejs_markup {
        return true;
    }
    let why = if has_ejs_markup {
        "contains `<% … %>` markup, which is Quarto 1 EJS syntax; \
         Quarto 2 does not evaluate EJS"
    } else {
        "contains no doctemplate directives (`$var$`, `$for(…)$`, `$if(…)$`), \
         so it would be copied into the page unchanged"
    };
    push_diag(
        diags,
        "Q-12-24",
        format!(
            "Listing `{listing_id}`: template `{}` {why}. Quarto 2 custom listing \
             templates use doctemplate syntax; see the Listings guide, “Custom \
             templates”. Listing skipped.",
            custom.template_path.display()
        ),
    );
    false
}
```

Then change the `ListingType::Custom` arm in `render_one` to:

```rust
        ListingType::Custom => match load_custom_template(r, host_input, diags) {
            Some(custom) => compile_template(
                &r.listing.id,
                &custom.source,
                &custom.template_path,
                &custom.resolver,
                diags,
            )
            .filter(|t| custom_template_is_templated(&r.listing.id, &custom, t, diags))
            .and_then(|t| render_template(&r.listing.id, &t, &template_ctx, diags)),
            // No usable custom template — fall back to default. The
            // appropriate Q-12-* diagnostic was already emitted by
            // `load_custom_template`.
            None => render_builtin(&r.listing.id, ListingType::Default, &template_ctx, diags),
        },
```

Also update the module-level doc comment near the top of `listing_render.rs` (the `//! … Q-12-10` paragraph around `:29`) with one sentence: "`Q-12-24` fires when a custom template compiled to pure literal text or still contains EJS `<% … %>` markup; the listing is skipped."

- [x] **Step 4: Add the catalog entry, docs page, and sidebar entry**

In `crates/quarto-error-catalog/error_catalog.json`, after the `"Q-12-23"` object, add (keep the file's existing key ordering/indentation; run `jq . crates/quarto-error-catalog/error_catalog.json > /dev/null` afterwards to confirm it still parses):

```json
  "Q-12-24": {
    "subsystem": "listing",
    "title": "Custom Listing Template Is Not a Doctemplate",
    "message_template": "A `type: custom` listing template contains no doctemplate directives, or contains Quarto 1 EJS `<% … %>` markup. Quarto 2 listing templates use doctemplate syntax; the listing is skipped.",
    "docs_url": "https://quarto.org/docs/errors/listing/Q-12-24",
    "since_version": "99.9.9"
  }
```

Create `docs/errors/listing/Q-12-24.qmd`:

````markdown
---
title: "Custom Listing Template Is Not a Doctemplate"
description: "A `type: custom` listing template has no doctemplate directives or still contains Quarto 1 EJS markup, so Quarto 2 skipped the listing rather than copy the file into the page."
code: Q-12-24
subsystem: listing
status: complete
since: "99.9.9"
categories:
  - listing
---

# `Q-12-24` — Custom Listing Template Is Not a Doctemplate

> A `type: custom` listing template has no doctemplate directives or
> still contains Quarto 1 EJS markup, so Quarto 2 skipped the listing
> rather than copy the file into the page.

## What this means

Quarto 2 renders custom listings with **doctemplates** — the same
`$variable$` / `$if(…)$` / `$for(…)$` syntax Pandoc templates use.
The template you named in `template:` compiled, but either

- it contains **no directives at all** — every byte is literal text, so
  rendering it would just paste the file into your page, or
- it contains `<% … %>` — **Quarto 1 EJS** markup, which Quarto 2 does
  not execute. Left in place, it would appear verbatim to readers (and
  link checkers would report `href="<%= item.link %>"`).

Rather than publish the raw file, Quarto 2 emits this warning and
leaves the listing out of the page.

## Why this happens

- **A Quarto 1 custom listing template carried over unchanged.** Q1
  listing templates are EJS (`welcome-card.ejs`, `item.ejs.md`).
  Quarto 2 does not run EJS; see also [`Q-12-9`](Q-12-9.qmd), which
  fires on the `.ejs` / `.ejs.md` extension at configuration time.
- **A partially ported template** — some `<%= … %>` expressions were
  rewritten to `$…$`, others were missed.
- **A static file named by mistake** — `template:` points at a plain
  markdown or HTML file with nothing to interpolate.

## How to fix

Rewrite the template as a doctemplate. The common mappings:

| Quarto 1 (EJS)                          | Quarto 2 (doctemplate)              |
| --------------------------------------- | ----------------------------------- |
| `<%= item.title %>`                     | `$it.title$` (inside `$for(items)$`) |
| `<% for (const item of items) { %> … <% } %>` | `$for(items)$ … $endfor$`     |
| `<% if (item.image) { %> … <% } %>`     | `$if(it.image)$ … $endif$`          |
| `<%= item.myfield %>` (custom field)    | `$it.myfield$` or `$it.extra.myfield$` |
| JavaScript expressions                  | Pre-compute the value into a listing field |

Before:

```
<div class="cards">
<% for (const item of items) { %>
  <a href="<%= item.link %>"><%= item.title %></a>
<% } %>
</div>
```

After:

```
::: {.cards}
$for(items)$
[$it.title$]($it.path$)
$endfor$
:::
```

The [Listings guide](/guides/projects/listings.qmd#custom-templates)
documents the values a template can read and a complete card example.

## Related

- [`Q-12-9`](Q-12-9.qmd) — `template:` has a Quarto 1 `.ejs` /
  `.ejs.md` extension (configuration-time warning for the same
  migration).
- [`Q-12-10`](Q-12-10.qmd) — the template failed to compile, or its
  output failed to re-parse as markdown.
- [`Q-12-8`](Q-12-8.qmd) — the template file could not be read.
````

In `docs/_quarto.yml`, add `            - errors/listing/Q-12-24.qmd` on the line immediately after `            - errors/listing/Q-12-23.qmd` (same indentation).

- [x] **Step 5: Run the tests and lint; verify they pass**

Run: `cargo nextest run -p quarto-core custom_template_`
Expected: all `custom_template_*` tests PASS, including the five new ones and the revised `.ejs.md` test.

Run: `cargo clippy -p quarto-core --all-targets -- -D warnings`
Expected: clean.

Run: `cargo xtask lint --quiet`
Expected: no `error-docs-page-missing` / `error-docs-sidebar-unlisted` violations.

Run: `cargo nextest run -p quarto-core`
Expected: all pass (no other test asserted the verbatim-splice behaviour).

- [x] **Step 6: Commit**

```bash
git add crates/quarto-core/src/transforms/listing_render.rs crates/quarto-error-catalog/error_catalog.json docs/errors/listing/Q-12-24.qmd docs/_quarto.yml
git commit -m "listing: Q-12-24 — warn and skip a custom template that is not a doctemplate (bd-custom-template-not-templated-e5t6m0i0)

A \`type: custom\` template with no \`\$\` directives compiled as one
literal and was spliced into the page verbatim, silently; a raw-HTML
wrapped Quarto 1 EJS card template hit exactly this. After a
successful compile, check that some top-level node is not a Literal
and that the source has no \`<%\`; otherwise emit Q-12-24 and skip the
listing (same path as a Q-12-10 compile error)."
```

---

### Task 2: Widen Q-12-9 to `.ejs` and reframe it as "Quarto 1 EJS template"

**Files:**
- Modify: `crates/quarto-core/src/project/listing/config.rs` (`"template"` arm at ~`:515-528`; tests near `template_ejs_md_extension_emits_q_12_9` at ~`:1712`)
- Modify: `crates/quarto-error-catalog/error_catalog.json` (`Q-12-9` entry: `title` and `message_template`)
- Modify: `docs/errors/listing/Q-12-9.qmd` (full rewrite — the current page says "templates are EJS, full stop", the opposite of the truth)

**Interfaces:**
- Consumes: `push_diag(diagnostics, code, message, &entry.value)` in `config.rs`; test helpers `parse`, `map`, `s` in the `config.rs` tests module.
- Produces: the code string `"Q-12-9"` now fires for any `template:` value ending in `.ejs` or `.ejs.md`. Task 3's e2e test asserts Q-12-9 appears for `welcome-card.ejs`.

- [x] **Step 1: Write the failing tests**

In the `config.rs` tests module, directly after `template_ejs_md_extension_emits_q_12_9`, add:

```rust
    // bd-custom-template-not-templated-e5t6m0i0: plain `.ejs` — the
    // more common Q1 spelling, and the one the Positron site uses —
    // must warn too. Q-12-9 used to test only `.ejs.md`.
    #[test]
    fn template_ejs_extension_emits_q_12_9() {
        let (_listings, diags) = parse(map(vec![
            ("type", s("custom")),
            ("template", s("welcome-card.ejs")),
        ]));
        let q129 = diags
            .iter()
            .find(|d| d.code.as_deref() == Some("Q-12-9"))
            .unwrap_or_else(|| panic!("expected Q-12-9, got: {diags:?}"));
        assert!(
            q129.title.contains("Quarto 1 EJS") && q129.title.contains("welcome-card.ejs"),
            "message must say Quarto 1 EJS and name the file; got: {}",
            q129.title
        );
    }

    // A doctemplate-named template must not trip the extension check.
    #[test]
    fn template_doctemplate_extension_does_not_emit_q_12_9() {
        let (_listings, diags) = parse(map(vec![
            ("type", s("custom")),
            ("template", s("cards.template")),
        ]));
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("Q-12-9")),
            "got: {diags:?}"
        );
    }
```

- [x] **Step 2: Run the tests and verify the first one fails**

Run: `cargo nextest run -p quarto-core template_ejs_extension_emits_q_12_9 template_doctemplate_extension_does_not_emit_q_12_9 template_ejs_md_extension_emits_q_12_9`
Expected: `template_ejs_extension_emits_q_12_9` FAILS (`expected Q-12-9, got: []`); the other two pass.

- [x] **Step 3: Implement**

In the `"template"` arm of `config.rs`, replace the `if path.ends_with(".ejs.md") { … }` block with:

```rust
                    // Q1 listing templates are EJS and ship under both
                    // spellings (`sidebar.ejs`, `item-default.ejs.md`).
                    // The extension is only a naming convention — the
                    // content check is Q-12-24 in listing_render.rs —
                    // but it catches the Q1 → Q2 carry-over at config
                    // time, before anything renders.
                    if path.ends_with(".ejs") || path.ends_with(".ejs.md") {
                        push_diag(
                            diagnostics,
                            "Q-12-9",
                            format!(
                                "`{}` has a Quarto 1 EJS template extension (`.ejs` / `.ejs.md`); \
                                 Quarto 2 listing templates use doctemplate syntax — see the \
                                 Listings guide, “Custom templates”.",
                                path
                            ),
                            &entry.value,
                        );
                    }
```

Update the `Q-12-9` catalog entry in `error_catalog.json` to:

```json
  "Q-12-9": {
    "subsystem": "listing",
    "title": "Quarto 1 EJS Listing Template Extension",
    "message_template": "The `.ejs` / `.ejs.md` extensions mark a Quarto 1 EJS listing template. Quarto 2 does not run EJS; custom listing templates use doctemplate syntax. See the Listings guide, “Custom templates”.",
    "docs_url": "https://quarto.org/docs/errors/listing/Q-12-9",
    "since_version": "99.9.9"
  }
```

Rewrite `docs/errors/listing/Q-12-9.qmd` in full:

````markdown
---
title: "Quarto 1 EJS Listing Template Extension"
description: "A listing `template:` ends in `.ejs` or `.ejs.md` — the Quarto 1 EJS convention. Quarto 2 does not run EJS; custom listing templates are doctemplates."
code: Q-12-9
subsystem: listing
status: complete
since: "99.9.9"
categories:
  - listing
---

# `Q-12-9` — Quarto 1 EJS Listing Template Extension

> A listing `template:` ends in `.ejs` or `.ejs.md` — the Quarto 1
> EJS convention. Quarto 2 does not run EJS; custom listing templates
> are doctemplates.

## What this means

Quarto 1 custom listing templates were **EJS** — embedded JavaScript
(`<% for (const item of items) { %>`, `<%= item.title %>`) — under
either a `.ejs` or a `.ejs.md` extension. Quarto 2 deliberately does
not embed a JavaScript runtime: custom listing templates are
**doctemplates**, the `$variable$` / `$if(…)$` / `$for(…)$` syntax
Pandoc templates use, so a template can never execute code.

The extension alone is only a naming convention, so this warning fires
at configuration time as an early hint. If the file really is EJS, the
render-time check [`Q-12-24`](Q-12-24.qmd) also fires and the listing
is skipped rather than pasted into the page.

## Why this happens

The listing configuration was carried over from a Quarto 1 project (or
copied from a Quarto 1 example) together with its EJS template.

## How to fix

Port the template to doctemplate syntax and give it a neutral
extension such as `.template`:

```yaml
listing:
  type: custom
  template: welcome-card.template   # was welcome-card.ejs
```

`<%= item.title %>` becomes `$it.title$` inside `$for(items)$ …
$endfor$`; `<% if (item.image) { %>` becomes `$if(it.image)$`. The
[Listings guide](/guides/projects/listings.qmd#custom-templates)
lists the values a template can read and shows a complete card
template; [`Q-12-24`](Q-12-24.qmd) has a fuller mapping table.

If you only renamed the file without porting its contents, you will
see `Q-12-24` next.

## Related

- [`Q-12-24`](Q-12-24.qmd) — the template's *contents* are not a
  doctemplate (no directives, or EJS markup present).
- [`Q-12-8`](Q-12-8.qmd) — `template:` file missing entirely.
- [`Q-12-7`](Q-12-7.qmd) — `template:` set without `type: custom`.
````

- [x] **Step 4: Run the tests and lint; verify they pass**

Run: `cargo nextest run -p quarto-core template_ejs template_doctemplate q_12_7`
Expected: all PASS — including `q_12_7_underlines_the_template_key_not_a_sibling` (its fixture uses `../template.ejs` and now also emits Q-12-9, but it selects Q-12-7 by code) and the `template_set_with_non_custom_type_emits_q_12_7`-style test that asserts `diags.len() == 1` (its fixture is `custom.template`, so no Q-12-9). If any test in `config.rs` asserts an exact diagnostic count with a `.ejs` fixture, change the fixture name to `.template` and note it in the report.

Run: `cargo clippy -p quarto-core --all-targets -- -D warnings`
Expected: clean.

Run: `cargo xtask lint --quiet`
Expected: clean.

Run: `cargo nextest run -p quarto-core`
Expected: all pass.

- [x] **Step 5: Commit**

```bash
git add crates/quarto-core/src/project/listing/config.rs crates/quarto-error-catalog/error_catalog.json docs/errors/listing/Q-12-9.qmd
git commit -m "listing: Q-12-9 also fires on .ejs and says 'Quarto 1 EJS template' (bd-custom-template-not-templated-e5t6m0i0)

The trigger was a literal .ejs.md suffix test, so the more common
Q1 spelling sailed past the one warning that exists for a carried-
over EJS template. Widen to .ejs, reword the message, and rewrite
the docs page, which claimed Q2 listing templates are EJS."
```

---

### Task 3: End-to-end integration test through the project pipeline

**Files:**
- Create: `crates/quarto-core/tests/integration/listing_custom_template_diagnostics.rs`
- Modify: `crates/quarto-core/tests/integration/main.rs` (add `pub mod listing_custom_template_diagnostics;` in alphabetical order, between `listing_pipeline` neighbours — i.e. after `listing_inline_records` would be wrong; `listing_custom_template_diagnostics` sorts before `listing_glob_resolution`)

**Interfaces:**
- Consumes: `"Q-12-24"` and `"Q-12-9"` from Tasks 1–2; `quarto_core::project::orchestrator::{ProjectPipeline, project_type_for}`, `quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult}`, `quarto_system_runtime::{NativeRuntime, SystemRuntime}` — the same driving shape as `listing_inline_records.rs`.
- Produces: nothing downstream.

- [x] **Step 1: Write the failing test file**

Create `crates/quarto-core/tests/integration/listing_custom_template_diagnostics.rs`:

```rust
/*
 * tests/integration/listing_custom_template_diagnostics.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * End-to-end tests for bd-custom-template-not-templated-e5t6m0i0: a
 * `type: custom` listing whose template is a Quarto 1 EJS file must
 * warn (Q-12-9 at config time, Q-12-24 at render time) and must NOT
 * splice the raw template into the page. Mirrors the strand's repro
 * (`repro.qmd` with `welcome-card.ejs` vs. `control.qmd` with a real
 * doctemplate over the same items).
 */

use std::path::PathBuf;
use std::sync::Arc;

use tempfile::TempDir;

use quarto_core::format::Format;
use quarto_core::project::ProjectContext;
use quarto_core::project::orchestrator::{ProjectPipeline, project_type_for};
use quarto_core::render_to_file::{RenderToFileOptions, RenderToFileResult};
use quarto_system_runtime::{NativeRuntime, SystemRuntime};

const PROJECT: &str = "project:\n  type: website\n  render:\n    - \"*.qmd\"\n";

/// Copied byte-for-byte in shape from the Positron site's
/// `welcome-card.ejs`: raw-HTML-wrapped EJS with no `$` anywhere,
/// which compiles as one literal and re-parses cleanly — the exact
/// silent case.
const EJS_TEMPLATE: &str = "```{=html}\n\
<div class=\"custom-card-grid\">\n\
  <% for (const item of items) { %>\n\
    <a href=\"<%= item.link %>\" class=\"custom-card-wrapper\">\n\
      <h3 class=\"custom-card-title\"><%= item.title %></h3>\n\
    </a>\n\
  <% } %>\n\
</div>\n```\n";

const DOCTEMPLATE: &str = "::: {.custom-card-grid}\n\
$for(items)$\n\
::: {.custom-card}\n\
### [$it.title$]($it.path$)\n\
:::\n\
\n\
$endfor$\n\
:::\n";

fn listing_page(template: &str) -> String {
    format!(
        "---\ntitle: Cards\nlisting:\n  id: cards\n  type: custom\n  template: {template}\n  contents: \"item-*.qmd\"\n---\n\nBefore the listing.\n\n::: {{#cards}}\n:::\n"
    )
}

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn render_project(fixture: impl FnOnce(&std::path::Path)) -> (PathBuf, Vec<RenderToFileResult>) {
    let temp = TempDir::new().unwrap();
    let project_dir = temp
        .path()
        .canonicalize()
        .unwrap_or_else(|_| temp.path().to_path_buf());
    fixture(&project_dir);

    let runtime: Arc<dyn SystemRuntime> = Arc::new(NativeRuntime::new());
    let mut project = ProjectContext::discover(&project_dir, runtime.as_ref()).unwrap();
    let options = RenderToFileOptions::default();
    let project_type = project_type_for(&project);
    let mut pipeline = ProjectPipeline::new(
        &mut project,
        project_type,
        Format::html(),
        "html",
        &options,
        runtime.clone(),
    );
    let summary = pollster::block_on(pipeline.run()).expect("pipeline");
    assert!(
        summary.pass1_failures.is_empty() && summary.pass2_failures.is_empty(),
        "unexpected failures: pass1={:?} pass2={:?}",
        summary.pass1_failures,
        summary.pass2_failures,
    );
    std::mem::forget(temp);
    (project_dir, summary.outputs)
}

fn html_for(outputs: &[RenderToFileResult], relative_output: &str) -> String {
    let suffix: PathBuf = relative_output.split('/').collect();
    let out = outputs
        .iter()
        .find(|o| o.output_path.ends_with(&suffix))
        .unwrap_or_else(|| panic!("no output ending in `{relative_output}`"));
    std::fs::read_to_string(&out.output_path).unwrap()
}

fn all_diag_codes(outputs: &[RenderToFileResult]) -> Vec<String> {
    outputs
        .iter()
        .flat_map(|o| o.render_output.diagnostics.iter())
        .filter_map(|d| d.code.clone())
        .collect()
}

fn write_items(p: &std::path::Path) {
    write(&p.join("_quarto.yml"), PROJECT);
    write(
        &p.join("item-one.qmd"),
        "---\ntitle: First Item\ndescription: Description of the first item\n---\n\nOne.\n",
    );
    write(
        &p.join("item-two.qmd"),
        "---\ntitle: Second Item\ndescription: Description of the second item\n---\n\nTwo.\n",
    );
}

/// The strand's `repro.qmd`: a Quarto 1 EJS template must not reach
/// the reader, and both diagnostics must fire.
#[test]
fn ejs_custom_template_warns_and_is_not_spliced_into_the_page() {
    let (_dir, outputs) = render_project(|p| {
        write_items(p);
        write(&p.join("welcome-card.ejs"), EJS_TEMPLATE);
        write(&p.join("repro.qmd"), &listing_page("welcome-card.ejs"));
    });
    let html = html_for(&outputs, "repro.html");
    assert!(
        !html.contains("<%"),
        "raw EJS must not be spliced into the page: {html}"
    );
    assert!(
        !html.contains("custom-card-wrapper"),
        "the listing must be skipped, not rendered from the EJS file: {html}"
    );
    let codes = all_diag_codes(&outputs);
    assert!(codes.iter().any(|c| c == "Q-12-9"), "expected Q-12-9; got {codes:?}");
    assert!(codes.iter().any(|c| c == "Q-12-24"), "expected Q-12-24; got {codes:?}");
    assert!(
        !codes.iter().any(|c| c == "Q-12-10"),
        "no compile/re-parse diagnostic expected; got {codes:?}"
    );
}

/// The strand's `control.qmd`: a real doctemplate over the same items
/// renders cards and triggers neither diagnostic.
#[test]
fn doctemplate_custom_template_renders_cards_without_diagnostics() {
    let (_dir, outputs) = render_project(|p| {
        write_items(p);
        write(&p.join("card.template"), DOCTEMPLATE);
        write(&p.join("control.qmd"), &listing_page("card.template"));
    });
    let html = html_for(&outputs, "control.html");
    assert!(html.contains("custom-card-grid"), "{html}");
    assert!(html.contains("First Item") && html.contains("Second Item"), "{html}");
    assert!(html.contains("href=\"item-one.html\""), "{html}");
    let codes = all_diag_codes(&outputs);
    for code in ["Q-12-9", "Q-12-24", "Q-12-10"] {
        assert!(!codes.iter().any(|c| c == code), "unexpected {code}; got {codes:?}");
    }
}
```

Register it in `crates/quarto-core/tests/integration/main.rs` as `pub mod listing_custom_template_diagnostics;` in alphabetical position (before `pub mod listing_glob_resolution;`).

- [x] **Step 2: Run the tests and verify they pass (and that the guard is real)**

Run: `cargo nextest run -p quarto-core --test integration listing_custom_template_diagnostics`
Expected: both PASS on top of Tasks 1–2.

Then confirm the first test actually guards the bug: temporarily stash Task 1's `listing_render.rs` (`git stash push crates/quarto-core/src/transforms/listing_render.rs`), re-run the same command, and confirm `ejs_custom_template_warns_and_is_not_spliced_into_the_page` FAILS with `raw EJS must not be spliced`; then `git stash pop`. Record both outputs in the report.

- [x] **Step 3: Gate and commit**

Run: `cargo clippy -p quarto-core --all-targets -- -D warnings` — clean.
Run: `cargo nextest run -p quarto-core` — all pass.

```bash
git add crates/quarto-core/tests/integration/listing_custom_template_diagnostics.rs crates/quarto-core/tests/integration/main.rs
git commit -m "listing: e2e test — an EJS custom template warns (Q-12-9, Q-12-24) and never reaches the page (bd-custom-template-not-templated-e5t6m0i0)"
```

---

### Task 4: Docs — "Custom templates" section in the Listings guide; fix the EJS wording on the other listing error pages

**Files:**
- Modify: `docs/guides/projects/listings.qmd` (append a `## Custom templates` section after `### Not yet supported`; fix the `$item.key$` sentence in `### Records`)
- Modify: `docs/errors/listing/Q-12-10.qmd:20` ("a built-in or a custom EJS file")
- Modify: `docs/errors/listing/Q-12-7.qmd:20` ("typically an EJS or Handlebars file")
- Modify: `docs/errors/listing/Q-12-8.qmd:34,52` (`my-listng.ejs`; the Q-12-9 related line)
- Modify: `docs/errors/listing/Q-12-14.qmd:20,41` ("the EJS file"; `my-template.ejs`)

**Interfaces:**
- Consumes: the binding names in Global Constraints; the built-in templates at `crates/quarto-core/src/project/listing/templates/*.template` as the reference the docs point to.
- Produces: the anchor `#custom-templates` on the Listings guide, which the Q-12-9 / Q-12-24 pages (Tasks 1–2) and the diagnostic messages already reference.

- [x] **Step 1: Fix the `$item.key$` sentence in `### Records`**

In `docs/guides/projects/listings.qmd`, change

```
custom template can read as `$item.key$` (also `$item.extra.key$`) — which
```

to

```
custom template can read as `$it.key$` (also `$it.extra.key$`) — which
```

- [x] **Step 2: Append the Custom templates section**

Append to the end of `docs/guides/projects/listings.qmd`:

````markdown
## Custom templates

Beyond the built-in `default`, `grid` and `table` layouts, a listing can
render through a template you write:

```yaml
listing:
  id: cards
  type: custom
  template: card.template
  contents: "guides/*.qmd"
```

`template:` resolves relative to the page that declares the listing.
Quarto 2 custom listing templates are **doctemplates** — the
`$variable$` syntax Pandoc templates use — not the EJS templates of
Quarto 1. A doctemplate cannot run code; it interpolates values,
branches on them and loops over them. The output is markdown, parsed
back into the page, so raw HTML goes in a `` ```{=html} `` block just as
it would in a `.qmd` file.

### Syntax

| Directive | Meaning |
| --- | --- |
| `$listing.id$`, `$it.title$` | Interpolate a value; dotted paths walk into maps. |
| `$for(items)$ … $endfor$` | Loop. Inside the body the current item is `$it$` (also `$items$`). |
| `$sep$` | Inside a loop: emitted between items, not after the last. |
| `$if(it.image)$ … $else$ … $endif$` | Branch on truthiness (empty strings and missing keys are false). |
| `$it:item-default()$` | Apply a partial to a value — here the built-in `item-default` partial, or a same-named `.template` file next to your page. |
| `$-- text` | Comment; not rendered. |
| `$$` | A literal dollar sign. |

A template that contains none of these is not a template: Quarto 2
warns ([`Q-12-24`](/errors/listing/Q-12-24.qmd)) and skips the listing
rather than paste the file into the page.

### Values a template can read

- **`listing.*`** — the listing's own configuration: `id`, `type`,
  `fields`, `field-display-names`, `sort-ui`, `filter-ui`, `page-size`,
  `grid-columns`, `template-params` (anything under `template-params:`
  in the YAML, passed through for your template's own options).
- **`items`** — one entry per item, each with `title`, `subtitle`,
  `description`, `author`, `date`, `date-modified`, `image`,
  `image-alt`, `categories`, `path` (the link target, already
  page-relative), `filename`, `reading-time`, `word-count`, `order`,
  `draft`, plus `image-html` and `category-html` pre-rendered snippets.
  Custom fields from an inline record or from a document's
  `listing-item.extra` are readable directly (`$it.icon$`) and under
  `$it.extra.icon$`.
- **`project.*`** — `site-url` and `title` from the website
  configuration.

The built-in layouts are themselves doctemplates and make the best
reference: `listing-default.template`, `listing-grid.template`,
`listing-table.template` and the `item-default` partial live under
[`crates/quarto-core/src/project/listing/templates/`](https://github.com/quarto-dev/q2/tree/main/crates/quarto-core/src/project/listing/templates)
in the Quarto 2 repository.

### Example: a card grid

`card.template`:

```
::: {.custom-card-grid}
$for(items)$
::: {.custom-card}
### [$it.title$]($it.path$)

$if(it.description)$
$it.description$
$endif$
:::

$endfor$
:::
```

Style `.custom-card-grid` and `.custom-card` in your site's CSS.

### Migrating a Quarto 1 template

Quarto 1 listing templates were EJS (`.ejs` / `.ejs.md`). Quarto 2 does
not run EJS — a carried-over template warns with
[`Q-12-9`](/errors/listing/Q-12-9.qmd) at the extension and
[`Q-12-24`](/errors/listing/Q-12-24.qmd) at the contents. Rewrite it:

| Quarto 1 (EJS) | Quarto 2 (doctemplate) |
| --- | --- |
| `<%= item.title %>` | `$it.title$` |
| `<% for (const item of items) { %> … <% } %>` | `$for(items)$ … $endfor$` |
| `<% if (item.image) { %> … <% } else { %> … <% } %>` | `$if(it.image)$ … $else$ … $endif$` |
| `<%= item.myfield %>` | `$it.myfield$` |
| `<%= items.length %>`, string manipulation, `process.env` | No expressions: pre-compute the value into a listing field or a record key, or drop the branch. |

Give the ported file a neutral extension such as `.template` so the
`.ejs` warning stops firing.
````

- [x] **Step 3: Correct the EJS wording on the four other pages**

- `docs/errors/listing/Q-12-10.qmd` line 20: replace `template (a built-in or a custom EJS file) over the listing's` with `template (a built-in or a custom doctemplate) over the listing's`.
- `docs/errors/listing/Q-12-7.qmd` line 20: replace `(typically an EJS or Handlebars file)` with `(a doctemplate — see the [Listings guide](/guides/projects/listings.qmd#custom-templates))`. Re-read the sentence so it still parses.
- `docs/errors/listing/Q-12-8.qmd` line 34: `my-listng.ejs` → `my-listng.template`; line 52: replace the `Q-12-9` bullet text with ``- `Q-12-9` — `template:` has a Quarto 1 `.ejs` / `.ejs.md` extension.``
- `docs/errors/listing/Q-12-14.qmd` line 20: `at the EJS file that renders each item` → `at the doctemplate that renders the items`; line 41: `template: my-template.ejs` → `template: my-template.template`.

Then `grep -rn -i "ejs" docs/errors/listing/` and confirm every remaining hit is a deliberate "Quarto 1 EJS" reference (Q-12-9, Q-12-24, Q-12-8's related line).

- [x] **Step 4: Render the docs with Quarto 2 and inspect**

Run (from the worktree root): `cargo run --bin q2 -- render docs/ 2>&1 | grep -i "error\|Q-" ; echo "exit=${PIPESTATUS[0]}"`
Expected: exit 0; no new diagnostics attributable to the edited pages.

Run: `grep -c "custom-templates" docs/_site/guides/projects/listings.html && grep -o 'id="custom-templates"' docs/_site/guides/projects/listings.html`
Expected: the anchor exists.

Run: `ls docs/_site/errors/listing/Q-12-24.html docs/_site/errors/listing/Q-12-9.html`
Expected: both exist.

Run: `cargo xtask lint --quiet`
Expected: clean.

(If `docs/_site` is not the output directory, check `docs/_quarto.yml` for `output-dir` and adjust the paths.) Do not commit rendered output.

- [x] **Step 5: Commit**

```bash
git add docs/guides/projects/listings.qmd docs/errors/listing/Q-12-10.qmd docs/errors/listing/Q-12-7.qmd docs/errors/listing/Q-12-8.qmd docs/errors/listing/Q-12-14.qmd
git commit -m "docs: custom listing templates are doctemplates — add the Listings guide section, fix pages that said EJS (bd-custom-template-not-templated-e5t6m0i0)"
```

---

## Controller verification (after all tasks)

- [x] `cargo nextest run --workspace` before the final-review fix wave (Rust final; pre-squash history is kept on the local `history/` snapshot ref): **13389 passed (1 leaky), 199 skipped** vs the live baseline 13380 / 199 at `05b6fd75c` → **+9 passed, +0 skipped**, accounted for exactly: Task 1 +5 net in `listing_render.rs` (5 new, 1 revised), Task 2 +2 in `config.rs`, Task 3 +2 integration. The leaky test is `quarto-hub::integration session_auth::logout_clear_cookie_not_overridden_by_reissue` — a crate this branch does not touch; pre-existing. A second run after the fix-wave rename (`cf3506c3b`) is recorded below.
- [x] `cargo xtask verify` — **full**, hub/WASM leg included (`quarto-core` is in the WASM client's closure): all steps passed before the fix wave (Rust build/tests, tree-sitter, ts-packages 604+77+137 tests, hub-client build + 251 tests). Log: scratchpad `verify-full.log`.
- [x] **End-to-end through the binary** (output inspected). Invocation, in `/Users/gordon/src/q2-positron-docs/llms-info/repros/ejs-template-dumped`:
  ```
  rm -rf _site && <worktree>/target/debug/q2 render . --to html
  ```
  Observed:
  ```
  Warning: [Q-12-9] `welcome-card.ejs` has a Quarto 1 EJS template extension (`.ejs` / `.ejs.md`); Quarto 2 listing templates use doctemplate syntax — see the Listings guide, “Custom templates”.
  Warning [Q-12-24]: Listing `guide-sections`: template `…/welcome-card.ejs` contains `<% … %>` markup, which is Quarto 1 EJS syntax; Quarto 2 does not evaluate EJS. … Listing skipped.
  Rendered 4 of 4 files to …/_site — 2 warnings
  ```
  `grep -c '<%' _site/repro.html` → **0** (was 9 on 0.27.0); `<div id="guide-sections">` is empty (skipped); `_site/control.html` still has `custom-card-grid`, "First Item", "Second Item", `href="item-one.html"`. Exit 0.
- [x] Final whole-branch review (opus): no Critical; Important #3/#4 (two docs inaccuracies) + minors #5–#8 fixed in one wave (now folded into the docs commit); #10 (absolute `template:` path vs path-resolution contract, pre-existing) filed as **bd-o1meelim** (related bd-oejuizi9); the `<%` escape-hatch / partial-sniff gap filed as **bd-owflmojl**. Deferred minors (T1 edge tests, T1 test-module size, T1 table dashes, T2 `.EJS` case) stay deferred.
- [x] Reconcile this checklist against what landed; commit the plan.
- [ ] Ask Gordon before pushing.

## Investigation context (from the 2026-08-25 investigation; kept as the argued root cause)

### The silence, step by step (at `05b6fd75c`)

1. **Config parse** — `config.rs:515-528`. `template:` is recorded; the only content-adjacent check is `path.ends_with(".ejs.md")` → Q-12-9. `type: custom` is set, so Q-12-7 (`:593`) cannot fire.
2. **Load** — `load_custom_template`, `listing_render.rs:375-421`. Plain `fs::read_to_string`; only Q-12-8 for I/O failure.
3. **Compile** — `compile_and_render`, `listing_render.rs:453-497`. Succeeds: the doctemplate grammar's only special character is `$` (`tree-sitter-doctemplate/grammar/grammar.js`: `text: /[^$]+/`). A `$`-free file parses as a single `TemplateNode::Literal`.
4. **Render** — output ≡ input.
5. **Re-parse** — `listing_render.rs:222-265`. Bare EJS trips the qmd parser → Q-12-10 "re-parse failed" (what the old test at `:1229` was really observing). Wrapped in ```` ```{=html} ````, the re-parse is clean. Silence.

Silence boundary: *no bare `$`* **and** *re-parses cleanly*. The better the template, the likelier the silence.

> Note (final review, 2026-08-25): the *unit* fixture in `listing_render.rs` (raw-HTML-wrapped EJS) did emit a Q-12-10 re-parse warning pre-fix ("HTML element converted to raw HTML"), so it was not fully silent; the *project-level* repro (Task 3's integration test and the Positron fixture) was. The integration test, not the unit test, carries the silence evidence.

### Docs were wrong in the same direction

`Q-12-9.qmd` said "Quarto 2 simplifies the model: templates are EJS, full stop. The file should end in `.ejs`" — the opposite of the catalog's own message, routing a user who hit the one existing warning straight into the silent case. `Q-12-10.qmd`, `Q-12-7.qmd`, `Q-12-8.qmd`, `Q-12-14.qmd` also said EJS. No "Q1 → Q2 listing template migration guide" existed (`listings.qmd` had no custom-template section; `templates.qmd` is "TBD").

### Why doctemplate only

Listings epic settled decision #3 (2026-05-05): no JS runtime in the binary or WASM; a third-party template in a hub-client project cannot execute code in the browser; one template syntax across the product; source-tracked diagnostics. The escape hatch for logic is pre-computed fields (`listing-item.extra`) and the planned Lua-filter slot between generate and render (bd-0fd0), not another template language.

### Repro

`/Users/gordon/src/q2-positron-docs/llms-info/repros/ejs-template-dumped/` (outside this repo): `repro.qmd` + `welcome-card.ejs` → 9 literal `<%` in `_site/repro.html`, zero warnings; `control.qmd` + `control-card.template` → real cards. Task 3 reproduces the pair as an integration test.

### Relationship to bd-oywyaouf

Same bug, filed 2026-08-06 out of the Q-12-7 work. Closed as superseded on 2026-08-25; its content-sniff idea is decision 6 above, its fall-back-to-default decision is reversed by decision 1, and its proposed Q-12-15 became Q-12-24.
