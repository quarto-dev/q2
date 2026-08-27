---
name: ejs-listing-port
description: Port a Quarto 1 EJS custom listing template to a Quarto 2 doctemplate, or write a new one. Use when a render warns Q-12-7 ("template: was set but type: is not custom"), Q-12-9 (".ejs" / ".ejs.md" extension), Q-12-24 ("template contains no doctemplate directives"), or Q-12-10 ("Undefined variable"); when a listing renders with the built-in layout instead of the custom one, or dumps the template verbatim into the page; when listing links point at .qmd files or listing images 404; or when writing any custom listing template for Quarto 2.
---

# ejs-listing-port Skill

Quarto 1 custom listing templates were EJS — embedded JavaScript. Quarto 2
does not run EJS: custom listing templates are **doctemplates**, the
`$variable$` / `$if(…)$` / `$for(…)$` syntax Pandoc templates use, so a
template can never execute code.

The syntax translation is mechanical and self-announcing — a half-finished
port either warns or looks obviously wrong. **The two things that make ports
fail are not syntax**, and both are silent: no error, no warning, and no
visible difference in the template's text. State them out loud before you
start, or you will skip them.

## The two silent failure modes

### 1. Links and images must be markdown, never raw HTML

A template's output is markdown, re-parsed into the page. Only *then* are
paths resolved: `LinkRewriteTransform` rewrites `.qmd` link targets to output
URLs, and resource collection notes every image so the file gets copied.
Both walk **parsed markdown nodes**. `Block::RawBlock` and `Inline::RawInline`
are no-op leaves in both
(`crates/quarto-core/src/transforms/link_rewrite.rs`,
`.../resource_collector.rs`), so a path inside a raw-HTML attribute is
invisible to each.

`$it.path$` is deliberately a *source* path, which is what makes the markdown
form work:

```
[$it.title$]($it.path$)          ✔ becomes href="…​.html"
`<a href="$it.path$">`{=html}    ✘ ships a dead .qmd href
```

For images the same split costs **two** things — rewriting *and* copying:

```
![$it.image-alt$]($it.image$)     ✔ rewritten, and the file is copied
`<img src="$it.image$">`{=html}   ✘ neither: the src 404s in the deployed site
```

Raw HTML is fine *inside* the link text — the built-ins do exactly this with
``[`$image-html$`{=html}]($path$)``. **Anchor markdown, contents raw.**

This is settled design, not a gap: Quarto 2 emits HTML from the AST and will
not parse HTML you author (`claude-notes/plans/2026-04-24-websites-phase-6.md`
Decision 1; `claude-notes/plans/2026-08-13-site-root-relative-paths.md`
Case C). Do not propose an HTML post-processor. Make the markdown path
obvious instead.

**Why this survives review.** The image failure hides itself whenever the
image is an item *document's* own front-matter `image:` — that page's render
copies the file regardless, so a raw-`<img>` template looks perfect. It only
bites for inline-record fields and custom fields, which is exactly where a
ported gallery or card grid gets its thumbnails.

### 2. The description placeholder envelope must be unconditional

Quarto 1 auto-filled a missing item description from the first paragraph of
the rendered item page. Quarto 2 does the same, via a post-render
substitution — but only inside the
`description-placeholder-begin` / `-end` markers, and only if they are
emitted **outside** the `$if$`:

````
::: {.listing-description}
```{=html}
$it.description-placeholder-begin$
```

$if(it.description)$
$it.description$
$endif$

```{=html}
$it.description-placeholder-end$
```
:::
````

The markers delimit the *region* to substitute, so they must exist for
exactly the items the `$if$` skips. The built-in templates gate the whole
envelope on `$if(description)$` — so **copying the built-in shape loses
previews for precisely the items that need them.** A custom template can and
should do better.

## Read this first

`docs/guides/projects/listing-templates.qmd` in the q2 repo is the
authoritative treatment: both rules above, the doctemplate language subset,
what each built-in layout emits, the Q1 → Q2 mapping table, a full worked
before/after port, and what doctemplates cannot do. Its sibling
`docs/guides/projects/listings.qmd` (§"Custom templates") carries the syntax
table and the per-item value list.

Canonical spellings, since Q1 habits produce the wrong ones:

- `$it.<key>$` inside `$for(items)$`. (`$items.<key>$` is an accepted alias;
  prefer `$it$`.) Inside a partial applied to an item, keys are bare:
  `$title$`.
- `type: custom` **and** `template:` are both required. `template:` alone
  warns `Q-12-7` and silently uses the built-in layout.
- Give the file a neutral extension (`.template`) so `Q-12-9` stops firing.

## Worked examples

- **The built-ins** — `crates/quarto-core/src/project/listing/templates/`
  (`item-default`, `item-grid`, `item-table`, and the `listing-*` wrappers).
  Short, idiomatic, and the reference for class names a custom template must
  match to inherit the listing CSS.
- **`references/worked-examples.md`** in this skill — three annotated ports
  in increasing difficulty, including the phrasing-content constraint that
  bites card grids.
- **The in-repo contract tests** —
  `crates/quarto-core/tests/integration/listing_pipeline.rs`, the three
  `custom_template_*` tests. They pin both silent failure modes and the
  unconditional envelope, so they are executable documentation of the rules
  above.

## Procedure

1. **Read the Q1 template and inventory what it does** beyond
   interpolation: helper calls, JS prologues, expressions, nested loops,
   grouping. Those need decisions, not translation — see "What doctemplates
   cannot do" in the guide (JS constants → `template-params:`; string
   manipulation → pre-computed record key or `listing-item.extra`).
2. **Drop the outer `{=html}` fence.** A Q1 listing template is usually one
   big raw-HTML block; that fence is what makes every path inside it
   invisible. Removing it is most of the port.
3. **Convert `<div class="…">` to `::: {.…}`.** Same output element, but the
   contents are markdown, so links and images inside resolve.
4. **Convert every anchor and image to markdown** (rule 1). Keep raw HTML
   only as link *text* or as genuinely inert markup.
5. **Add the description envelope unconditionally** (rule 2) if the listing
   wants previews.
6. **Guard every optional read with `$if$`** — an absent value warns
   `Q-12-10` rather than rendering blank.
7. **Verify against the rendered output** (below). Do not stop at "it
   renders."

## Verification — required, and not optional

Neither silent failure produces a diagnostic, and neither changes the
template's text in a way review catches. A passing render proves nothing.
After porting, render the project and inspect the **output**:

```bash
cargo run --bin q2 -- render <project>     # in q2; `quarto render .` for users
```

1. **Read the `href` values** in the listing's host page. Every link to a
   project document must end in `.html`. A surviving `.qmd` means that anchor
   is still raw HTML.
2. **Read the `src` values, then confirm each file exists** under the output
   directory. A `src` that looks right with no file behind it is a raw
   `<img>` — this is the check that catches the masked case, and the one
   people skip.
3. **Check an item with no front-matter `description:`.** It should show a
   first-paragraph preview. Nothing shown means the envelope is inside the
   `$if$` instead of around it.
4. **Grep the render output for `Q-12-`.** Any warning here is real.

Report what you inspected, not just that the render succeeded. If you could
not check one of the four, say which.
