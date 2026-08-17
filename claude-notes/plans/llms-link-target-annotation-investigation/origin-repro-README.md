# llms-txt companions occupy the source-path namespace in the output tree

Observed with **q2 0.22.0** (`q2 (quarto 2) 0.22.0`).

`q2 render` in this directory, then look at the output tree.

## What happens

With `website.llms-txt: true`, q2 writes each page's markdown companion to
`<page>.md` — the same relative path the *source* file occupies. For a
project whose pages are `.md` files (like the Connect docs), the output
tree ends up carrying a file at every source path:

```
_site/index.html          _site/index.md
_site/guide/index.html    _site/guide/index.md
```

This is deliberate: q2's design plan (`claude-notes/plans/2026-08-14-llms-txt-website-support.md`,
decision 2) chose `<page>.md` over Quarto 1's `<page>.llms.md` because it
is the ecosystem convention, gated on a collision-safe write mechanism.
That mechanism exists and works — see "What is already guarded" below.

The unguarded consequence is on the *serve* side. A link that names a
source path and does not go through q2's link rewriter now resolves to
the companion instead of 404ing. `index.md` in this repro has three links
to the same page:

| link as authored | href emitted |
|---|---|
| `[guide](guide/index.md)` (markdown) | `guide/index.html` ✅ rewritten |
| `[guide](guide/index.html)` (markdown) | `guide/index.html` ✅ |
| `<a href="guide/index.md">` (raw HTML) | `guide/index.md` ⚠️ verbatim |

The rewriter itself is not confused — it drives off the source index, and
every markdown link is rewritten correctly. But q2 does not parse HTML, so
the raw-HTML href passes through untouched, and `_site/guide/index.md` now
exists. The reader who clicks it gets raw markdown (served as
`text/markdown` or downloaded, depending on the host) instead of the page.

Before llms-txt existed, that same link was a 404: loud, and catchable by
any link checker. Now it is a 200 serving the wrong content — the failure
mode q2's own Q-5-24 rationale ("a silently wrong file is worse than
failing") exists to avoid, moved from the write side to the serve side.

Raw HTML is the form this repro pins because it is self-contained. The
same shadowing applies to any other href the rewriter does not reach.

**How much does that matter?** Measured on the Connect docs, very little:
1670 in-body markdown `.md` links are all rewritten correctly, as are the
161 config `file:` entries (a navbar `href: index.md` renders as
`../../index.html` on a depth-2 page), and there are zero raw-HTML `.md`
hrefs in the sources. Exactly one link escapes — `quarto-tiers.url:
"/admin/licensing/index.md#product-tiers"` in `_quarto.yml`, a *custom
metadata key* read by the posit-docs project type, which q2 has no way to
recognize as a link. That one entry accounts for all 49 affected pages,
and it is already `br-root-absolute-assets-1o6yy4mx`'s subject.

So q2 rewrites every link it owns. The cost of the shadowing is not wrong
rendering — it is lost *detectability*: that link used to 404.

## Scope check: only `.md` sources are exposed

With `.qmd` sources the source-path namespace and the companion namespace
do not overlap, so nothing is shadowed. Verified: in a `.qmd` project, a
raw-HTML link to `guide/index.qmd` renders verbatim and **404s** — the
loud failure you want — while only the `.md` spelling finds a companion.
The exposure is exactly `.md` sources + `llms-txt`.

## Expected

Given the measured blast radius, the rendering behavior is defensible as
is. What is worth fixing is detection:

1. **Tooling (this repo).** `llms-info/tools/site-asset-check.py --links`
   currently reports the shadowed references as resolving. It should treat
   a target that is both a page source path and a companion path as
   suspect. This is where the real fix belongs.
2. **Docs.** Point the one escaping `quarto-tiers.url` at the `.html` path
   — tracked in `br-root-absolute-assets-1o6yy4mx`.
3. **q2, optional.** A distinct `.llms.md` namespace would restore 404-ing
   at the cost of the ecosystem convention. Not obviously worth it.

Rejected: requiring authors to decorate ambiguous links with an attribute
(`link-format="html"` vs `"llms"`) and warning on undecorated ones. Pass-2
detection is feasible — `LinkRewriteTransform` holds the `ProjectIndex`,
the website config, and each resolved target — but the mechanism aims at
the wrong population. The 1670 links that could carry an attribute are the
ones already correct; the single escapee is custom metadata that cannot
carry a Pandoc attribute. The warning would fire at ~100% false positive.

Worth pursuing separately: there is no way to author a link that
*deliberately* targets a companion, nor to opt out of
`LlmsCaptureTransform`'s blanket `.html`→`.md` retarget (its only escapes
are drafts, 404, external URLs, and non-page resources). An attribute
would fit that architecture cleanly — as an expressiveness feature, not as
a fix for this.

## Quarto 1 for comparison

Q1 writes `<page>.llms.md`, so the source-path namespace stays empty in the
output tree and a source-path link 404s. Q1 also rewrote raw-HTML hrefs,
because it parsed the rendered HTML — q2 deliberately does not, which is
settled (see `br-website-image-resources-vwy2548v`) and not what this
strand contests.

## What is already guarded

The write side. A user file at a companion path is not clobbered — q2
keeps a `.quarto/llms-manifest.json` ledger and errors on any planned
write it cannot prove is its own:

```
Error [Q-5-28]: `about.md` already exists in the output directory and was
not generated by `llms-txt`
```

Verified separately (page `about.qmd` plus a hand-written `about.md`
declared in `project.resources`). That guard is working as designed; this
strand is about the namespace overlap it does not address.
