# Website cross-document link navigation in hub-client

## Status

Investigation + design. **No implementation in this plan** — awaiting
go-ahead.

## Bug

In hub-client, clicking a cross-document link inside the preview
iframe should switch the active editor file to the link's target.
This works in **non-website projects** but is broken in
**website projects**.

### Reproduced 2026-05-01 against the dev server

Both projects exist on `http://localhost:5173/` ("No website" and
"test website") under the same project set.

**Project: "No website"** (`b8f5bc56-bc36-…`, two files
`index.qmd` + `another.qmd`).
- `index.qmd` body: `[page](./another.qmd)`.
- Rendered href in the iframe: `./another.qmd`.
- Click → URL hash changes from
  `…/file/index.qmd` → `…/file/another.qmd`. ✅ works.

**Project: "test website"** (`e9491a05-…`, copy of
`examples/websites/08-hub-preview/`).
- `index.qmd` body: `[About](about.qmd)` and `[first post](posts/first.qmd)`.
- Rendered hrefs in the iframe (after the website pipeline):
  - `/.quarto/project-artifacts/about.html`
  - `/.quarto/project-artifacts/posts/first.html`
- Click → URL hash unchanged
  (`…/file/index.qmd` stays). ❌ broken.

The body links *and* the (currently below-`lg`-hidden) sidebar
links share the same form: artifact-rooted `.html` URLs. Same bug
applies to both.

## Root cause

The website pipeline rewrites cross-document QMD links to their
output-side `.html` form:

| Source                | Rewritten to                                     |
|-----------------------|--------------------------------------------------|
| `[A](about.qmd)`      | `<a href="/.quarto/project-artifacts/about.html">`        |
| `[F](posts/first.qmd)`| `<a href="/.quarto/project-artifacts/posts/first.html">`  |

That rewrite happens in
`crates/quarto-core/src/transforms/navigation_href.rs` (sidebar /
nav contexts) and the body-link counterpart for body-link
resolution (`body_link_resolution.rs` / similar). The rewrite is
correct: a deployed website needs `.html` URLs, and the hub-client
preview shares the same renderer (`RenderToHtmlRenderer::new(
"/.quarto/project-artifacts")`), so the iframe sees the same
`.html` URLs.

The hub-client click handler in
`hub-client/src/utils/iframePostProcessor.ts:140-158` only
intercepts links whose href ends in `.qmd`:

```ts
doc.querySelectorAll('a[href*=".qmd"]').forEach((anchor) => {
  …
  if (parsed.path && parsed.path.endsWith('.qmd')) {
    …
    options.onQmdLinkClick!({ path, anchor: parsed.anchor });
  }
});
```

So in the non-website case (raw `./another.qmd` URLs preserved
from source), the handler matches and intercepts. In the website
case (rewritten to `.html`), the handler doesn't match, and the
default link behavior kicks in — which inside an `about:srcdoc`
iframe is effectively no-op (no real navigation happens, the
hash doesn't change).

Concretely, three things conspired:

1. **Website pipeline output is correct** — it produces
   deployed-website-shaped URLs.
2. **Hub-client preview shares that renderer** — by design,
   so artifact (CSS / image) resolution works uniformly.
3. **Click interception was written before the website pipeline
   existed** — only knows about source-shape `.qmd` links.

## Fix space

Two reasonable directions.

### Option A: Reverse-map `.html` → `.qmd` at click time

In `iframePostProcessor.ts`, add interception for hrefs that look
like artifact-rooted `.html` URLs. Strategy:

```ts
// After the existing .qmd handler:
doc.querySelectorAll(`a[href*="/.quarto/project-artifacts/"][href$=".html"]`)
   .forEach((anchor) => {
     const href = anchor.getAttribute('href');
     // Strip artifact root, swap .html → .qmd, look up in files[]
     const projectPath = href
        .replace(/^\/.quarto\/project-artifacts\//, '')
        .replace(/\.html(#.*)?$/, '.qmd$1');
     const parsed = parseLink(projectPath);
     if (parsed.path && filesContains(parsed.path)) {
       anchor.addEventListener('click', e => {
         e.preventDefault();
         options.onQmdLinkClick!({
           path: parsed.path,
           anchor: parsed.anchor,
         });
       });
     }
   });
```

The post-processor currently doesn't have a `filesContains`
predicate; the file-existence check has to either be passed in
through `PostProcessOptions` or the path can be unconditionally
mapped and the existing `Preview.handleNavigateToDocument`
handle the "no such file → open new-file dialog" branch.

**Pros**:
- Self-contained in hub-client. No engine changes.
- The artifact-root invariant stays intact (CSS / images keep
  resolving from `/.quarto/project-artifacts/...`).
- Works for any future link the pipeline rewrites under that
  root, regardless of source extension nuances.

**Cons**:
- Hard-codes the artifact root (`/.quarto/project-artifacts/`)
  in click logic. If the root ever changes, two places have to
  be updated.
- Reverse-mapping `.html` → `.qmd` assumes the source is
  always a `.qmd`. Mostly true today (Q2 only renders `.qmd`),
  but `.md` / `.ipynb` exist in TS Quarto and may exist in Q2
  one day. The fix should look up the source by checking for
  any matching project file with a known renderable extension,
  not blindly substitute `.qmd`.
- Doesn't help links pointing to *outside* the artifact root
  (e.g., asset downloads that land elsewhere).

### Option B: Render preview-mode QMD-shaped URLs

Add a renderer flag that emits the source-shape link
(`./about.qmd`) instead of the output-shape (`about.html`) when
the consumer is the hub-client preview. The existing `.qmd`
click handler then matches.

**Pros**:
- Conceptually cleanest from the hub-client's perspective:
  links are about *source* navigation in the preview context,
  and `.qmd` is the source.
- Click handler stays simple.

**Cons**:
- Breaks the "preview and disk render share the renderer"
  invariant — they'd diverge on link form. We'd be
  re-introducing the very fork we eliminated when we unified
  on `render_page_in_project`.
- Needs a new flag through `RenderToHtmlRenderer` /
  resolver context. Touches engine code that's currently
  policy-free for "what URL form to emit".
- Doesn't fix the case where a user opens a deployed site URL
  in the hub-client preview iframe with the file picked from
  history — those'd still arrive as `.html` URLs.

### Recommendation

Option A. It localizes the fix in the hub-client (the consumer
that knows it's a preview context), keeps the renderer
artifact-shape consistent, and is a smaller change. The
preview-only divergence proposed in B is a step backwards from
the renderer-unification work.

Within Option A, the look-up-by-source-file approach is the
robust form: don't blindly replace `.html` → `.qmd`, instead try
each known renderable extension and intercept iff the resulting
project path is a real `FileEntry`. This also covers a future
day when `.md` / `.ipynb` join the renderable set.

## Proposed fix shape (Option A)

### `iframePostProcessor.ts`

Extend `PostProcessOptions` with the project file list (or a
predicate), and add a new pass that intercepts artifact-rooted
`.html` links:

```ts
export interface PostProcessOptions {
  currentFilePath: string;
  /**
   * Project file paths (no leading slash). Used to reverse-map
   * artifact-rooted .html URLs to their source-side .qmd
   * (or future .md / .ipynb) so cross-document clicks switch
   * the active editor file.
   */
  projectFilePaths: readonly string[];
  onQmdLinkClick?: (arg: { path: string, anchor: string | null } | { anchor: string }) => void;
}
```

A new helper inside the post-processor:

```ts
const ARTIFACT_ROOT = '/.quarto/project-artifacts/';
const RENDERABLE_EXTS = ['.qmd'];   // future: '.md', '.ipynb'

function reverseMapArtifactHref(
  href: string,
  filePaths: readonly string[],
): { path: string; anchor: string | null } | null {
  if (!href.startsWith(ARTIFACT_ROOT)) return null;
  const stripped = href.slice(ARTIFACT_ROOT.length);
  const { path: stem, anchor } = parseLink(stripped);
  if (!stem || !stem.endsWith('.html')) return null;
  const base = stem.slice(0, -'.html'.length);
  for (const ext of RENDERABLE_EXTS) {
    const candidate = base + ext;
    if (filePaths.includes(candidate)) {
      return { path: candidate, anchor };
    }
  }
  return null;
}
```

The new `forEach` runs alongside the existing `.qmd` handler; a
matched link gets the same `e.preventDefault()` +
`onQmdLinkClick(...)` treatment.

### `Preview.tsx`

`postProcessIframe(...)` is invoked from
`hub-client/src/hooks/useIframePostProcessor.ts` (or wherever the
iframe lifecycle lives). Plumb `projectFilePaths` through —
`Preview.tsx` already has `files: FileEntry[]` and computes
`projectFilePaths = files.map(f => f.path)` for user-grammar
discovery; reuse that.

`handleNavigateToDocument` already does the right thing —
matches `targetPath` against `files`, switches the active file.
No change needed there.

### Tests

- **Unit test for `reverseMapArtifactHref`**: artifact-rooted
  hrefs map back to their source path; non-artifact hrefs pass
  through; missing source files return `null`; anchors are
  preserved; subdirectory paths (`posts/first.html` →
  `posts/first.qmd`) work.
- **Integration test against `iframePostProcessor`**: build a
  fake iframe document with both `.qmd` (source) and `.html`
  (rewritten) anchors, post-process it with both kinds of
  project files in `projectFilePaths`, simulate clicks, assert
  the right `onQmdLinkClick` calls fire.

End-to-end (manual, via Chrome DevTools MCP):

- "test website" → click `[About]` body link → editor switches
  to `about.qmd`.
- "test website" → click `[first post]` body link → editor
  switches to `posts/first.qmd`.
- "test website" with full-preview view (sidebar visible) →
  click sidebar entries → editor switches accordingly.
- "No website" (sanity check) → existing `.qmd` link still
  works (no regression).

## Open questions for the user

1. **Renderable-extension list.** Today only `.qmd` exists. I
   propose making the click handler iterate a small list (`.qmd`
   for now) so adding `.md` / `.ipynb` in the future is a
   one-line change. Acceptable, or do you want to keep it as a
   bare `.qmd` substitution?
2. **Scope.** Option A intercepts artifact-rooted `.html` links.
   Future website features may emit links to other
   artifact-rooted resources (e.g. `index.html` for a
   sub-listing, or `404.html`). Should the click handler treat
   *all* artifact-rooted `.html` links the same way (reverse-map
   if a source exists, otherwise fall back to default
   behavior), or be stricter and only intercept links that map
   to a known source file?
3. **Anchor handling within rewritten links.** A future case:
   a body link `[Section](about.qmd#intro)` gets rewritten to
   `about.html#intro`. Current `parseLink` splits on `#`; the
   `reverseMapArtifactHref` sketch above preserves the anchor
   correctly. Just confirming that the existing
   `handleNavigateToDocument(path, anchor)` path handles
   anchor scrolling after the file switch — I believe it does
   (the comment on line 207-208 says "DoubleBufferedIframe will
   handle the anchor scrolling after swap"), but the cross-doc
   anchor case may not be exercised yet. Want me to add a test
   for this even if it's not the main fix?
4. **Hard-coded artifact root.** The `/.quarto/project-artifacts/`
   prefix is hard-coded today in
   `wasm-quarto-hub-client/src/lib.rs:1417` (passed to
   `RenderToHtmlRenderer::new`). If we want to avoid duplicating
   it on the hub-client side, we could either (a) export it as
   a constant from the WASM bridge, or (b) add a `vfs_root`
   field to the WASM render response so hub-client knows where
   the artifact root currently is. Probably overkill for this
   bug; flagging it as a code-smell to potentially address
   alongside the JS-loading work (`bd-e7b7`).

## Work items

- [ ] Resolve open questions above.
- [ ] Implement `reverseMapArtifactHref` + `PostProcessOptions`
      extension in `iframePostProcessor.ts`.
- [ ] Plumb `projectFilePaths` through `useIframePostProcessor`
      / `Preview.tsx` / `MorphIframe`.
- [ ] Unit tests for the helper.
- [ ] Integration test against `iframePostProcessor` exercising
      both `.qmd` and `.html` link forms.
- [ ] In-browser verification on "test website" and "No website"
      via Chrome DevTools MCP.

## References

- Repro hashes (test-website project ID prefix `25a55Noy`,
  no-website project ID prefix `3ZcKVsWZ`).
- Click interception:
  `hub-client/src/utils/iframePostProcessor.ts:140-158`.
- Navigate handler:
  `hub-client/src/components/render/Preview.tsx:200-218`.
- Body-link / nav-href rewriting (engine):
  `crates/quarto-core/src/transforms/navigation_href.rs`,
  `body_link_resolution.rs`.
- Artifact root constant:
  `crates/wasm-quarto-hub-client/src/lib.rs` (search
  `/.quarto/project-artifacts`).
- Renderer unification context:
  `claude-notes/plans/2026-04-27-websites-phase-9.md`.
