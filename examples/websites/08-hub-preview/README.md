# 08-hub-preview — Hub-client live preview against a website project

Demonstrates Phase 9 of the website epic: when you open a Quarto 2
website project in the hub-client, the live-preview iframe shows
the active page **with full project chrome** — sidebar, navbar,
prev/next, cross-doc links, shared theme CSS — refreshed live as
you edit.

This is a **manual recipe**. Unlike the other examples, you can't
fully exercise this one with `quarto render` alone; the payoff is
in the browser.

## What this demonstrates

- The hub-client invokes a WASM Quarto build whose
  `render_page_in_project` entry point runs the project pipeline
  end-to-end against the VFS, not just the active file.
- Pass 1 cache lives in IndexedDB inside the browser, so unchanged
  siblings don't re-extract their profile on every edit.
- Editing a file's body re-renders only that page's HTML.
- Editing a file's frontmatter `title:` flows to siblings'
  sidebars within the next render cycle.
- Theme CSS is flushed to a synthetic VFS path
  (`/.quarto/project-artifacts/...`) the rendered HTML can resolve
  through the iframe.

## Prerequisites

```bash
# 1. Build Quarto, including the WASM bundle.
cd hub-client
npm run build:all
cd ..

# 2. (Optional) Confirm the q2 binary is up to date.
cargo build --bin q2
```

## Recipe

### Step 1 — Start the hub server

From the repo root:

```bash
cargo run --bin q2 -- hub --project examples/websites/08-hub-preview
```

The server prints a URL (defaults to http://127.0.0.1:3000). Note
the URL.

### Step 2 — Open the hub UI in Chrome

Open the printed URL. The hub-client UI loads. The project view
should show the file tree on the left (`_quarto.yml`, `index.qmd`,
`about.qmd`, `posts/first.qmd`, `posts/second.qmd`).

### Step 3 — Open `index.qmd` and look at the preview

Click `index.qmd` in the file tree. The editor opens on the left;
the preview iframe loads on the right.

The preview should show:

1. The page body ("This example is shaped to be opened in...").
2. **A sidebar** with four entries — Home, About, First Post,
   Second Post — with "Home" highlighted.
3. **The shared theme CSS applied** (Bootstrap-styled, not bare
   browser default).

Before Phase 9 (or in `render_qmd` instead of
`render_page_in_project`), the preview would be the page body
alone: no sidebar, no theme. The presence of the sidebar is the
visible Phase 9 payoff.

### Step 4 — Edit body, watch live re-render

In the editor, change the body of `index.qmd`. Within ~500 ms the
preview iframe re-renders. The sidebar stays put; only the body
changes.

### Step 5 — Edit a sibling's title, watch sidebar update

Open `posts/second.qmd` in the editor. Change `title: Second Post`
to `title: Second Post (revised)`.

Switch back to `index.qmd`. The sidebar in the preview should now
read "Second Post (revised)" — even though `index.qmd` itself
didn't change. This is the dependency-graph payoff: editing
`second.qmd`'s frontmatter invalidates `index.qmd`'s sidebar
profile and forces the active page to re-render.

### Step 6 — Click a sidebar link in the preview

In the preview iframe, click the "About" sidebar entry. The
editor on the left should switch to `about.qmd`, and the preview
should reload to show the about page.

This is wired through `MorphIframe.onNavigateToDocument` —
sidebar / navbar / body links all post a navigation message back
to the hub-client, which switches editor focus and re-renders.

## What to do if something doesn't work

- **No sidebar in preview** — check that `_quarto.yml` is at the
  project root and declares `project.type: website`. The preview
  falls back to single-file mode if no `_quarto.yml` is found in
  any ancestor directory.
- **Theme styling missing** — open the iframe's network tab; the
  `<link rel="stylesheet">` should resolve to a path under
  `/.quarto/project-artifacts/`. If it 404s, the WASM build may be
  stale — re-run `npm run build:all` from `hub-client/`.
- **Sidebar text doesn't update on sibling edits** — confirm that
  the edited file is part of the project. If `_quarto.yml` is the
  thing being edited, hub-client may need a full reload to pick
  up the new sidebar config.

## Notes

- A browser smoke recording (a GIF or screen capture) of this
  recipe is a follow-up that the Phase 9 close-out flagged but
  didn't ship.
- The native equivalent of this code path is exercised by the
  test fixture
  `crates/quarto-core/tests/fixtures/websites/hub-smoke/` and the
  `render_page_in_project.rs` integration test, so the Rust side
  is regression-guarded even before the GIF lands.
