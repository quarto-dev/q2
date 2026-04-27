# Syntax highlighting — Phase 3.5: native user grammars + filter-authored spans + documentation fixtures

- **Parent plan**: `claude-notes/plans/2026-04-19-syntax-highlighting-design.md`
- **Beads**: bd-n7x2 (overall syntax-highlighting epic)
- **Status**: planned 2026-04-20

## Why this phase exists

Phase 3 landed browser built-ins and closed the native + browser loop for the 14 statically-linked grammars. Studying the plan + current test surface after Phase 3 closed surfaced four gaps not explicitly covered by any phase:

1. **Native user-grammar end-to-end**. Phase 1 added `UserGrammars::load_from_directory` + `highlight_with_user` plus pipeline plumbing in `code_highlight.rs` (`load_user_grammars` reads `<project>/_quarto/grammars/`). Library-level tests pass. We've **never driven `quarto render` end-to-end** on a `.qmd` whose code block is styled via a user grammar — so the CLI path could be silently broken (same failure mode as the Phase 2 post-mortem, where the CLI path bypassed the annotate stage entirely while all in-process tests stayed green).

2. **Browser user grammars**. All of Phase 4. Unchanged scope; we do it after this phase so we have a fixture pattern to lean on.

3. **Filter-authored `data-hl-spans`**. Decision 1 in the original plan says: *"a user filter producing the same encoding must work identically to the built-in stage."* The annotate walker has unit coverage for "skip if attribute is already set," but no end-to-end test where an actual user Lua filter emits spans and the HTML writer renders them. No phase lists this explicitly — an orphan feature.

4. **`theme: none` highlight behavior**. `theme: none` takes the static-`DEFAULT_CSS` path, which doesn't load `highlight.scss`. Highlight classes get emitted but aren't styled. This might be the intended semantics ("you opted out of styling") or a bug we need to fix. Either way, untested and unclear.

This phase fills those four gaps using fixtures that **double as tests and as future user-facing documentation**.

## Goals

1. Fill the four gaps above with end-to-end tests that exercise the CLI path (not in-process helpers with default config), per the Phase 2 post-mortem lesson.
2. Produce `.qmd` fixtures that are readable as examples — suitable for lifting into `docs/` when user-facing documentation is written.
3. Keep everything in `crates/quarto/tests/smoke-all/`'s existing pattern (`ensureFileRegexMatches` frontmatter) so we inherit its CI integration for free.
4. Settle the `theme: none` question explicitly.

Out of scope:
- Browser user grammars → Phase 4.
- Line-numbering / `hl_lines` directives → Phase 5.
- User override of built-in `highlights.scm` → Phase 5.
- Full hub-client user-grammar UX (upload, discovery, sync) → Phase 6.

## Test-first approach (per CLAUDE.md TDD rule)

Plan is to write each fixture's `ensureFileRegexMatches` assertions **before** running `cargo nextest run --workspace`. For features where the underlying code is expected to already work (built-in, user-grammar library path, filter-authored walker-skip), the fixture test should **pass on first run** — verifying that the CLI path actually exercises the library. For features where existing code might be missing something (`theme: none` + highlighting), the fixture's failure is the signal to design a fix.

Explicit TDD expectation per fixture:

| Fixture | Expected on first run | If it fails, the signal is: |
|---|---|---|
| 01-builtin-python | ✅ pass | regression in Phase 1/2/3 wiring — investigate before adding fixture |
| 02-inline-code | ✅ pass | same as 01 |
| 03-user-grammar-toml | **?** — library works, CLI path unverified | CLI path doesn't pick up user grammars; fix plumbing |
| 04-filter-authored-spans | **?** — walker-skip is tested, HTML writer renders attr, but filter-hub wiring unverified | filter ordering / pipeline / JSON-encoding issue; diagnose |
| 05-theme-none | ✅ pass (expected) — emit classes, no default CSS colors | resolved design: `theme: none` means user takes over theming; hl-* classes emitted but not styled |

### Actual first-run results (2026-04-20)

After fixing author error in the `ensureFileRegexMatches` YAML shape (the two-sub-array form is **must-match** vs **must-NOT-match**, not two groups of must-match as I initially wrote), results were:

| Fixture | Result | Notes |
|---|---|---|
| 01-builtin-python | ✅ pass | CLI path is properly wired through Phase 3. |
| 02-inline-code | ✅ pass | Inline `Code` highlighting works. |
| 03-user-grammar-toml | ✅ pass | **Native user-grammar end-to-end verified.** `load_user_grammars` finds `_quarto/grammars/toml/`, the WasmStore loader picks it up, spans render with the classes TOML's `highlights.scm` specifies. No latent bug. |
| 04-filter-authored-spans | **⚠️ pass after fix** | Initial run failed because Quarto 2's Lua bridge returns fresh copies of `cb.attr` (and of `cb.attr.attributes`) every read. In-place assignments like `cb.attr.attributes["k"] = v` silently don't persist — the mutation hits an ephemeral copy. See the follow-up-task section below for the structural fix. Workaround in the filter: rebuild the whole `attr` with `pandoc.Attr(...)` and assign to `cb.attr` as a single replacement. |
| 05-theme-none | ✅ pass | `theme: none` behavior matches the resolved design: hl-* classes emitted, default CSS does not color them. |

**Four of five fixtures revealed no latent bug** — the underlying code is correct and the CLI path is faithful to the library path for both built-in and user-grammar flows. The fifth fixture (04) revealed a real API gap we worked around and is tracked as a follow-up below.

## Fixture spec

Directory: `crates/quarto/tests/smoke-all/highlighting/`.

### 01-builtin-python.qmd

Covers Phase 1+2+3 in one place — Python code block + inline code, no theme, no filters. Reuses the spirit of the existing `claude-notes/fixtures/phase3-highlight-check.qmd` but lives in the test corpus so it runs in CI.

```yaml
---
title: Built-in syntax highlighting
format: html
_quarto:
  tests:
    html:
      noErrors: true
      ensureFileRegexMatches:
        - ["<pre class=\"sourceCode python\"", "hl-keyword\">def</span>", "hl-function-builtin\">print</span>"]
        # Inline code with {.python} opt-in
        - ["<code class=\"sourceCode python\"", "hl-function-builtin\">print</span>"]
        # Default CSS includes highlight colors even without a theme:
        - [".hl-keyword"]
---
```

### 02-inline-code.qmd

Focused demonstration of inline-code highlighting — the `` `foo()`{.python} `` case. Useful as a standalone example because it's the lesser-known feature.

### 03-user-grammar-toml.qmd

The important new coverage. Directory layout:

```
crates/quarto/tests/smoke-all/highlighting/03-user-grammar/
  03-user-grammar-toml.qmd
  _quarto.yml              # marks this as a project root so ctx.project.dir is correct
  _quarto/
    grammars/
      toml/
        toml.wasm          # symlinked or copied from the existing Phase 1 fixture
        highlights.scm     # same
```

Fixture content:

```yaml
---
title: User grammar — TOML
format: html
_quarto:
  tests:
    html:
      noErrors: true
      ensureFileRegexMatches:
        # TOML highlights `name = value` pairs with property/string/etc.
        # Exact capture names depend on tree-sitter-toml's highlights.scm;
        # smoke-test should match whichever classes the grammar actually
        # emits. We'll pick one or two stable ones after running it.
        - ["<pre class=\"sourceCode toml\"", "hl-"]
---

```toml
name = "example"
count = 42
```
```

The grammar and query come from the existing `crates/quarto-highlight/tests/fixtures/user-grammar-toml/` fixture — copied (not symlinked, since Cargo packaging handles copies more predictably).

### 04-filter-authored-spans.qmd

Demonstrates the Phase 0 decision-1 invariant: a user filter producing `data-hl-spans` works identically to the built-in stage. Layout:

```
crates/quarto/tests/smoke-all/highlighting/04-filter/
  04-filter-authored-spans.qmd
  highlight-words.lua
```

The Lua filter (`highlight-words.lua`) is small and readable — it targets a code block with class `log` and highlights the literal strings `ERROR` and `WARN` with capture names `error` and `warning`:

```lua
-- Filter that adds `data-hl-spans` to code blocks with class `log`.
-- Highlights the literal words ERROR and WARN via tree-sitter-style
-- capture names, which the HTML writer renders as `hl-error` /
-- `hl-warning` spans.
function CodeBlock(cb)
  if cb.classes[1] ~= "log" then return nil end
  local spans = {}
  local patterns = { ERROR = "error", WARN = "warning" }
  for needle, capture in pairs(patterns) do
    local init = 1
    while true do
      local s, e = cb.text:find(needle, init, true)
      if not s then break end
      -- Lua offsets are 1-based inclusive; our encoding is 0-based
      -- half-open, matching tree-sitter byte ranges.
      table.insert(spans, { s - 1, e, capture })
      init = e + 1
    end
  end
  -- Sort by start to match the usual depth-first emission order.
  table.sort(spans, function(a, b) return a[1] < b[1] end)
  cb.attributes["data-hl-spans"] = pandoc.json.encode(spans)
  return cb
end
```

Fixture:

```yaml
---
title: Filter-authored highlight spans
format: html
filters:
  - highlight-words.lua
_quarto:
  tests:
    html:
      noErrors: true
      ensureFileRegexMatches:
        - ["<pre class=\"sourceCode log\"", "hl-error\">ERROR</span>", "hl-warning\">WARN</span>"]
---

```log
2026-04-20 ERROR connection refused
2026-04-20 WARN high latency detected
2026-04-20 ERROR timeout
```
```

### 05-theme-none.qmd

Opens the design question explicitly. Fixture:

```yaml
---
title: theme none
format:
  html:
    theme: none
_quarto:
  tests:
    html:
      noErrors: true
      ensureFileRegexMatches:
        # hl-* spans should still be emitted (they're emit-time, not
        # theme-dependent)
        - ["hl-keyword\">def</span>"]
        # Open question: should .hl-keyword rules be in the static
        # DEFAULT_CSS too? Or is theme: none explicitly opt-out?
---

```python
def hi():
    pass
```
```

This fixture's test assertion covers the uncontroversial case (spans emitted). The question of whether `.hl-*` rules should be in the served CSS is decided during implementation. See **Design decisions** below.

## Design decisions to make during implementation

1. **Does `theme: none` serve `.hl-*` rules?** — **Resolved 2026-04-20: no.**
   - `theme: none` is historically an affirmative declaration that the user is taking over the entirety of theming (often via injected CSS on the DOM). The static `DEFAULT_CSS` must stay minimal-to-nonexistent in this case.
   - Users who opt out of Bootstrap AND want highlight colors: add `.hl-*` rules in their own stylesheet, or pick any theme other than `none` (e.g., `theme: cosmo`).
   - Fixture 05's assertion becomes: hl-* classes are emitted on the markup (emit-time, theme-independent), but no `.hl-*` rule is in the served CSS. User-facing example text should explain the trade-off.
   - May revisit if this turns out to be a footgun in practice.

2. **Where does the TOML grammar fixture live in the test tree?** Two options:
   - Copy `crates/quarto-highlight/tests/fixtures/user-grammar-toml/` contents into `crates/quarto/tests/smoke-all/highlighting/03-user-grammar/_quarto/grammars/toml/`. Plain and self-contained.
   - Symlink — bad for Windows, bad for Cargo packaging. Skip.
   - Recommendation: **copy at plan time; verify identical with `diff` or a checksum in CI.**

3. **Filter ordering**. The user filter must run BEFORE `CodeHighlightStage`. Looking at the pipeline: `UserFiltersStage::pre` → `AstTransforms` → `UserFiltersStage::post` → `CodeHighlightStage` → `RenderHtmlBody`. Both `pre` and `post` are before `CodeHighlightStage`. Either works. Smoke-all's `filters:` key in the frontmatter schedules a filter — which phase does it map to? Needs to be verified; if it routes to `post`, we're fine. If to `pre`, also fine. If neither, we need to wire it.

## Work items

### Phase 3.5.1 — Fixture wiring (TDD)

- [ ] Create `crates/quarto/tests/smoke-all/highlighting/` directory with a README explaining the set.
- [ ] Copy TOML grammar fixture from `crates/quarto-highlight/tests/fixtures/user-grammar-toml/` into `crates/quarto/tests/smoke-all/highlighting/03-user-grammar/_quarto/grammars/toml/`. Document the copy source in a `PROVENANCE.md` at the copy dest.
- [ ] Write the five `.qmd` fixtures + `highlight-words.lua` + `_quarto.yml` files per the spec above.
- [ ] Run `cargo nextest run --workspace`. Record per-fixture pass/fail against the expected table above.

### Phase 3.5.2 — Fix whichever TDD gaps surface

Depends on 3.5.1 results. Likely one of:

- [ ] **If 03-user-grammar-toml fails**: diagnose whether `load_user_grammars` is being reached on the CLI path; fix plumbing or path resolution.
- [ ] **If 04-filter-authored-spans fails**: inspect filter ordering (check the `filters:` frontmatter routing), verify `pandoc.json.encode`'s output is what the walker/writer expect, confirm the walker's "skip if attr present" path doesn't also re-encode or mis-handle.
- [ ] **If 05-theme-none fails on `.hl-*` rules**: implement Option A (include `highlight.scss` in static `DEFAULT_CSS`). Add a static-CSS test that the compiled fallback contains `.hl-keyword`.

### Phase 3.5.3 — Documentation lift

- [ ] Each fixture `.qmd` gets a leading comment block or `README.md` explaining what it demonstrates, in user-facing tone (not "this tests X" but "this shows how to Y"). That way when user-facing docs land, we just move the files.
- [ ] Update the parent plan's Phase 3.5 entry with the final pass/fail table and any design decisions recorded.

### Phase 3.5.4 — Wrap-up

- [ ] `cargo nextest run --workspace` green. `cargo xtask verify --skip-hub-tests --skip-hub-build` green (no hub-client changes in this phase).
- [ ] `npm run test:wasm` sanity check (nothing should have regressed).
- [ ] Stage and commit. Wait for push approval.

## Expected outcomes

After this phase:

- Four end-to-end fixtures exist in CI, exercising the four gaps.
- Any regression in the CLI-rendering path for either built-in or user grammars is caught automatically — same failure mode as the Phase 2 post-mortem can't recur silently.
- A filter-authored-highlight demo exists, validating Phase 0 decision 1 and serving as copy-paste reference for users who want to highlight something built-ins don't cover.
- `theme: none` behavior is either fixed or documented — no longer a grey area.
- When user-facing docs get written, the fixture directory is the obvious source of truth for examples.

## Open question for future phases

Phase 5 includes "User-override `highlights.scm` for built-in languages." That's adjacent to Phase 3.5.3's fixtures — a user wanting to customize Python's capture set could either write a new grammar (this phase) or override the built-in query (Phase 5). Once 3.5 lands, the override mechanism could be demonstrated with one more fixture following the same pattern. Track separately; not in scope here.

## Follow-up task: Lua attribute-mutation proxy

Quarto 2's Lua bridge returns fresh copies of `cb.attr` and of `cb.attr.attributes` on every read — see `crates/pampa/src/lua/types.rs:1734` (`LuaAttr::new(attr.clone())`) and the `get_field` branch at `:1591-1596` that creates a new Lua table populated from the cloned attributes on each access. As a result, direct mutation like `cb.attr.attributes["k"] = v` silently modifies an ephemeral copy and is discarded.

Pandoc's native Lua API handles this with proxy tables whose `__newindex` metamethod writes back to the underlying AST node. Our bridge doesn't. Anyone writing a Lua filter that tries to set an attribute (e.g. the `elem.attributes["loading"] = "lazy"` pattern from the type-doc example in `crates/pampa/resources/lua-types/pandoc/global.lua:17`) will hit this silently.

Two fix options:

1. **Proxy userdata for `attributes`**: instead of `LuaAttr::get_field` returning a fresh Lua table, return a userdata wrapper whose `__newindex` writes back through `&mut LuaAttr`. Requires some lifetime / borrow juggling since mlua userdata can't hold a `&mut` to another userdata's content easily — may need an `Rc<RefCell<Attr>>` pattern or similar.
2. **Document the current behavior + provide a helper**: e.g. `cb:set_attribute(k, v)` on `LuaBlock` that directly mutates `c.attr.2`. Narrower API surface, less like Pandoc's, but cheap to implement.

Filter 04's workaround (rebuild the whole `Attr` with `pandoc.Attr(...)` and assign to `cb.attr`) is acceptable for a test fixture but is a usability regression compared to Pandoc's API for end users. File as a beads issue post-Phase-3.5 and fix before we tell users "you can write Lua filters that add `data-hl-spans`" anywhere user-facing.
