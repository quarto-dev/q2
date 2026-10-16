# Plan F3c: `Page` becomes `none`

Part of the epic [`2026-10-15-format-resolution-epic.md`](2026-10-15-format-resolution-epic.md) (FR9); the execution model and landing procedure are there. A mechanical rename with no prerequisite; it lands before F3b so that F3b rebases onto it.

## Execution

- **Lane:** B (`.worktrees/workspace-6`), binding: F2, F3c, F4, F6, F5b, F8. This plan is number 2 of 6.
- **Autonomy:** there is no orchestrator process. This plan's agent knows its lane, the plan before it and the plan after it from the Lane, Start gate and Next in lane lines of this section, develops it with no sub-agents, and runs at most one plan per session: finish, report, stop (the next plan is a new session with its own gate). The Handoff log is the only channel to later plans. Nothing here waits for Gordon: where a step used to ask, this plan states a default. If a start gate or a precondition fails, the agent reports exactly what is missing in its final message and stops; it does not ask mid-session.
- **Waits on:** nothing but the planning files being on the integration branch (lane B runs it right after F2, or first if F2 is not done: it has no prerequisite, so it starts whenever its gate prints `ready`).
- **Start gate:** the plan's own file (and the epic) on the integration branch, and no tracked changes in the lane's tree (untracked build output such as `q2-preview-spa/public/` is not a problem; anything else means the previous plan did not finish: stop and report it in the final message).

  ```bash
  cd "$(git rev-parse --path-format=absolute --git-common-dir)/../.worktrees/workspace-6"
  miss=0
  for f in format-resolution-epic fr-f3c-none-rename; do
    git cat-file -e "feature/q2-preview-hub-parity:claude-notes/plans/2026-10-15-$f.md" 2>/dev/null || { echo "missing: $f is not on feature/q2-preview-hub-parity yet; stop and report"; miss=1; }
  done
  [ -z "$(git status --short | grep -v '^?? q2-preview-spa/public/')" ] || { echo "missing: workspace-6 tree is not clean; stop and report"; git status --short; miss=1; }
  [ "$miss" = 0 ] && echo ready
  ```

- **Branch:** `format-resolution/f3c-none-rename`, cut from the tip of `feature/q2-preview-hub-parity`:

  ```bash
  git switch -c format-resolution/f3c-none-rename feature/q2-preview-hub-parity
  npm install        # always: cheap when nothing changed; a stale node_modules after a branch switch caused bd-13gnwplg. The TS unions are checked with tsc
  git diff --stat    # if npm install touched package-lock.json and the diff is only deletions of optional platform entries (observed on macOS: 703 deletions of @esbuild/* entries), restore it: git checkout -- package-lock.json
  ```

  The lane worktrees (workspace-3 and workspace-6) were pre-staged with `npm install` on 2026-10-15 and the dependencies do not change before the lanes start, so the install here normally finds nothing to do. Node comes from `.nvmrc` (`fnm --use-on-cd` selects it on `cd`).
- **Merges to:** `feature/q2-preview-hub-parity`: `git rebase feature/q2-preview-hub-parity`, conflict-marker sweep, `git -C ../workspace-7 merge --ff-only format-resolution/f3c-none-rename`, then `git tag fr-landed/f3c format-resolution/f3c-none-rename`.
- **Unlocks:** F3b.
- **Next in lane:** F4 (`fr-f4-scope-boundary`), which waits on F3b.

## Goal

`ResolvedBookScope::Page` and its wire string `page` become `none` everywhere: Rust, the native `stats.book.scope` echo, the wasm mapping, the three TypeScript unions, the tests and their comments. `none` means "no book mode: the project is not a book, or the file is not in the chapter list"; it is a resolution result and an echo, never an input.

## What exists (verified 2026-10-15 and re-checked in round 5; the full site list below was grepped, nothing else was found)

- `ResolvedBookScope { Book, Chapter, Page }` in `project/book/scope.rs` (the `Page` variant is at `:68`; its doc comment at `:94-99` and the three returns at `:120, :137, :145, :151` say `Page`); `resolve_book_scope` returns `Page` for a non-book project (silently) and for a non-member (Q-5-41).
- **Rust sites that name `Page` as a book scope** (and only these; `ResolvedBookScope::Page` appears nowhere else): `scope.rs` (above), the native echo `quarto-preview/src/render_helper.rs:132` (`ResolvedBookScope::Page => "page"`, in the match at `~128-133`), the wasm mapping `wasm-quarto-hub-client/src/lib.rs:1564` (in the match at `~1561-1564`) and the doc comment above `render_pandoc_request` at `lib.rs:1601-1608` (`'page'` at `:1605`), and the tests `crates/quarto-core/tests/integration/pandoc_request_books.rs:659, 684, 738` and `crates/quarto-preview/tests/integration/book_downloads.rs:179`. **Unrelated hits that stay:** `ActivePage`, `ResourceScope::Page` (`quarto-preview/src/config.rs:491`), "Page-scoped" artifact comments (`wasm lib.rs:2500`), `diagnostics.rs` "Page-keyed", and the `page` URL query parameter in `q2-preview-spa/src/PreviewApp.tsx:720` and `pickInitialPage.ts:52`.
- **TS sites:** the three unions at `hub-client/src/pandoc/downloadController.ts:91`, `hub-client/src/pandoc/nativeRender.ts:107` and `ts-packages/preview-runtime/src/wasm-quarto-hub-client.d.ts:108` (with prose at `:122-123`); the TypeScript only ever compares against `'book'`. Tests and fixtures: `hub-client/src/pandoc/nativeRender.test.ts:100`, `hub-client/src/pandoc/downloadController.test.ts:583`, `hub-client/src/services/pandocBook.wasm.test.ts:28, 234`. A missed wasm test fails only at `npm run test:wasm`, not at compile. `crates/wasm-quarto-hub-client/pkg/` is untracked generated output: it is not edited (the build regenerates it).
- **Docs:** no docs page contains the wire word `page` (grepped `docs/` for `stats.book` and `book.scope`: `docs/errors/project/Q-5-41.qmd`, `Q-5-42.qmd` and `docs/guides/projects/book.qmd`, none of which spell the wire value). `Q-5-41.qmd` has "Page Is Not Part of the Book" in its title and `:24` mentions `--book-scope chapter`; `Q-5-42.qmd:18` mentions `--book-scope`. **F3b rewrites both pages** (the notice's meaning, the removed flag, the new warning), so **F3c edits no docs page**; its only docs task is the grep that confirms this.
- Tests naming `Page` or `"page"` also include the parity epic's scope tests (`scope.rs` unit tests, the route tests, `book_scope_cli_e2e.rs`): port whatever the grep in 3c.2 and 3c.3 finds.

## Decisions

- **F3c-D1 The Rust variant name.** Choose it now and record it in the Handoff log. A bare `None` is legal but reads like `Option::None` next to `Option<BookScope>` (and `ResolvedBookScope::None` must then always be written qualified); prefer a name that cannot be mistaken for it (for example `NoBook` or `Standalone`), unless the agent finds the qualified `None` clearer in practice. The wire string is `none` regardless.
- **F3c-D2 The wire is a rename, not a compatibility shim.** The `stats.book.scope` echo emits `none`; all three TS unions become `'book' | 'chapter' | 'none'`; nothing accepts `page` any more (it is output only, and the same-plan changes keep client and server in step). The input value `none` for `book-scope` stays invalid (FR5; F3a owns the diagnostic text, which says `none` is output only).
- **F3c-D3 Rust, wasm and TS change together.** The Rust echo, the wasm mapping and the TS unions land in this one plan, so F3c landing before F3b is safe: no client can ever see a string its types do not know, and F3b rebases onto a tree where the rename is already complete.

## Tasks

Per task: `cargo clippy -p <crate> --all-targets -- -D warnings` plus `cargo nextest run -p <crate>` for quarto-core and quarto-preview (and quarto if a test there names it); for the wasm crate `cd crates/wasm-quarto-hub-client && CC_wasm32_unknown_unknown=clang CFLAGS_wasm32_unknown_unknown="-isystem $PWD/wasm-sysroot" cargo check --target wasm32-unknown-unknown` (the wasm crate is excluded from the workspace, `Cargo.toml:11-12`, and has its own `.cargo/config.toml`; derived from `hub-client/scripts/build-wasm.js:150-170` and `dev-docs/wasm.md`, not run when written); `tsc` plus `vitest` in hub-client and `ts-packages/preview-runtime`; the rebuild chain, in this order: **`npm run build:all` from `hub-client/`** first (it already runs `fetch:pandoc && build:wasm && build:sandboxed && build && check:sw-precache`, `hub-client/package.json:14`, so one release+wasm-opt wasm build; `fetch:pandoc` needs the network), then `npm run test:wasm` (it runs against the **prebuilt** artifact `build:wasm` just produced), then `cargo xtask build-hub-client-embed` (the unions are shared with the editor bundle's download code; it re-runs `build:preview-embed`, which rebuilds wasm again: that second build is inherent); `cargo xtask lint` at the end.

- [x] 3c.1 Baseline: look up the ledger entry for the tip you branched from; do not run the workspace (the global rule: it is recommended, never run unprompted); if there is no ledger entry for the tip, say so in the Handoff log and continue.
- [x] 3c.2 Rename the Rust variant (F3c-D1) and the echo strings: `scope.rs` (the variant, its doc comment at `:94-99` and the returns), `render_helper.rs:132`, the wasm mapping at `lib.rs:1564` and the comment at `lib.rs:1605`, every doc comment that says "Page" as a scope. `grep -rn "Page" crates/quarto-core/src/project/book/ crates/quarto-core/src/pandoc_request crates/quarto-preview/src crates/wasm-quarto-hub-client/src` and read each hit: `ActivePage`, `ResourceScope::Page` and the other unrelated uses listed above stay.
- [x] 3c.3 The three TypeScript unions and every TS comparison, mock and fixture that names `page` (`grep -rn "'page'" hub-client/src ts-packages q2-preview-spa/src` and read each hit; only the scope echo changes: the `page` URL parameter in the SPA stays), the `.d.ts` prose at `:122-123`, and the tests listed above.
- [x] 3c.4 Docs: `grep -rn "stats.book\|book.scope\|'page'" docs/` to confirm no docs page spells the wire value (expected: none). **Edit no docs page**: `Q-5-41.qmd` and `Q-5-42.qmd` are rewritten whole by F3b (3b.3), and `docs/guides/projects/book.qmd` is updated by F3b (3b.3, 3b.5). If the grep finds a page that does spell the wire value, change that word here and say so in the Handoff log.
- [x] 3c.5 Tests: port every test that names the old variant or string (do not weaken them); add one assertion per layer that the wire string is `none` for a non-book project and for a non-member (native echo, wasm response, TS type). Run the audit grep from `.claude/rules/integration-tests.md` only if a test file is moved (none should be).
- [x] 3c.6 `cargo xtask lint`; record the wire change in the Handoff log for Gordon's changelog pass; plan end: nextest on the touched crates (and, in the final summary, the recommendation: workspace nextest or `nextest -p` on the direct dependents from `cargo tree -i <crate>`; do not run the workspace unprompted); restore `package-lock.json` if `npm install` only pruned optional entries (see the branch step); reconcile this checklist; write the Handoff log (the final variant name); land per the epic.

## Risks

- `scope.rs`, `render_helper.rs`, wasm `lib.rs` and the TS files are hot (F3b and F4 also edit them); keep this plan's commits to the rename so F3b's rebase is mechanical.
- Cross-language: the Rust echo and the TS unions change in the same plan or a client may see an unknown string.
- A stale embedded SPA or wasm artifact makes a manual check meaningless; run the rebuild chain above before looking at anything in a browser.

## Handoff log

**Headline:** `ResolvedBookScope::Page` is now `ResolvedBookScope::NoBook`; `stats.book.scope` emits `none` (native echo and wasm); the three TS unions are `'book' | 'chapter' | 'none'`. Wire change for the changelog: `stats.book.scope` is `none` where it said `page` (non-member page in a book).

- **Variant name (F3c-D1):** `NoBook` (a bare `None` would read like `Option::None` next to `Option<BookScope>`). F3b: use `ResolvedBookScope::NoBook`.
- **3c.2 grep:** Rust hits were exactly the listed sites (`scope.rs` variant, doc comments and four returns; `render_helper.rs` `scope_name` and its doc; wasm `lib.rs` mapping and the doc comment above `render_pandoc_request`; `pandoc_request_books.rs` x3; `book_downloads.rs`). `ActivePage`, `ResourceScope::Page`, "Page-scoped/keyed" comments untouched.
- **3c.3 grep:** the three unions, `.d.ts` prose, and `nativeRender.test.ts`, `downloadController.test.ts`, `pandocBook.wasm.test.ts` changed. The SPA `page` URL parameter stays. No other `'page'` scope hits.
- **3c.4 docs grep:** no docs page spells the wire value; no docs edited (F3b rewrites Q-5-41/Q-5-42 and book.qmd).
- **3c.5 tests:** ported, not weakened. The existing non-member assertions now check `none` at each layer: native echo (`book_downloads.rs`), wasm response (`pandocBook.wasm.test.ts`), TS type (`nativeRender.test.ts`, `downloadController.test.ts`). A non-book project echoes `book: null`, not a scope, so no extra assertion there.
- **package-lock.json:** `npm install` pruned 703 optional entries only; restored with `git checkout`.
- **Gates run:** clippy (quarto-core, quarto-preview) clean; wasm `cargo check` ok; hub-client tsc + vitest (src/pandoc) ok; `npm run build:all` ok; `npm run test:wasm` 390 passed, 28 skipped; `cargo xtask build-hub-client-embed` ok; `cargo xtask lint` ok. nextest quarto-core + quarto-preview: all pass once the preview SPA dist is fresh. Three quarto-preview tests (`pdfjs_serving::serves_the_viewer_tree_with_module_mime_types`, `pdfjs_serving::pruned_locales_get_the_en_us_file_and_other_misses_404`, `editor_ui::editor_ui_serves_pdfjs_from_the_viewer_bundle`) 404 against a stale `q2-preview-spa/dist/` that lacks `pdfjs/`; `cargo xtask build-q2-preview-spa` (which bundles `hub-client/public/pdfjs/`, written by `build:all`\'s `fetch:pandoc`) fixes it, and then all 272 pass. **Gate gap for later plans:** add `cargo xtask build-q2-preview-spa` before `nextest -p quarto-preview` whenever the SPA dist may be stale (for example after a branch switch). `dist-preview-embed` is meant to have `pdfjs/` pruned; that is not the dist these tests read.
- **Ledger:** no entry for the branch tip's lineage beyond `Ledger: ae2887867: 16134 passed, 202 skipped`; workspace not run. Recommend `nextest -p quarto` (direct dependent) or the workspace run before push.
