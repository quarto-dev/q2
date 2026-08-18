# Fix quarto-dev/q2#128 — hub-client preview links fail in Safari

## Overview

Clicking links in the hub-client HTML preview fails in Safari: the preview iframe
navigates away and goes blank. Console shows:

1. `Blocked script execution in 'https://quarto-hub.com/' because the document's
   frame is sandboxed and the 'allow-scripts' permission is not set.`
2. `Refused to display 'https://quarto-hub.com/team-sync/2026-04-15.qmd' in a
   frame because it set 'X-Frame-Options' to 'DENY'.`

### Root cause (confirmed)

The default `format: html` preview is `MorphIframe`
(`ts-packages/preview-renderer/src/iframe/MorphIframe.tsx:486`), a `srcdoc`
iframe with `sandbox="allow-same-origin allow-popups"` — **no `allow-scripts`**.
Link interception is done by `postProcessIframe`
(`ts-packages/preview-renderer/src/utils/iframePostProcessor.ts`), which attaches
`click` listeners from the *parent* realm onto elements inside the iframe
document.

Safari (WebKit bug [218086](https://bugs.webkit.org/show_bug.cgi?id=218086),
open since 2020, still present 2025) **blocks parent-attached event listeners on
a sandboxed frame that lacks `allow-scripts`**. Chrome/Firefox run them. So in
Safari the `preventDefault()` in the click handler never runs, the iframe
follows the raw href (resolved against the parent origin because `srcdoc` +
`allow-same-origin`), the hub responds `X-Frame-Options: DENY`, and Safari shows
a blank frame. Exactly the reported symptoms.

The same root cause also breaks the other parent-attached listeners in Safari:
scroll sync, click-to-position, and selection sync (`MorphIframe.tsx:414-480`),
plus the Cmd+S keydown handler (`iframePostProcessor.ts:303-308`). This fix
repairs all of them.

### Fix strategy

Add `allow-scripts` to the sandbox **and** neutralize in-document scripts with a
CSP meta tag injected into every preview HTML payload (initial `srcdoc` and
every morphdom update):

```
sandbox="allow-same-origin allow-scripts allow-popups"
<meta http-equiv="Content-Security-Policy" content="script-src 'none'">
```

This is the workaround recommended in the WebKit bug itself (comment #5).
`allow-scripts` clears WebKit's sandbox check so parent-attached listeners run;
the CSP then blocks all script execution *inside* the document (script elements,
inline `onclick` handlers, `javascript:` URLs) — which is precisely what the
sandbox withholds today. CSP does not block `addEventListener`-registered
listeners, so our handlers keep working.

Security posture is preserved:

- The `allow-scripts` + `allow-same-origin` escape vector requires a script
  running in the iframe's realm; CSP `script-src 'none'` prevents that, and the
  parent never injects `<script>` elements into the iframe (the script-inlining
  block in `iframePostProcessor.ts:176-202` stays disabled).
- The CSP meta must precede every script in document order, so injection is
  specified as: insert immediately after the `<!DOCTYPE>` if present, else at
  byte 0 — the parser places a leading `<meta>` in the implied `<head>`. This
  is robust against head-like markup inside comments/titles/scripts and
  uppercase tags, which a "first child of `<head>`" string search is not.
  Post-fix the CSP is the *only* script mitigation, so an injection miss is a
  same-origin escape; this contract is the highest-risk part of the
  implementation.
- The CSP meta is injected into *every* html payload — the initial `srcdoc`
  assignment and every morphdom update — so it stays present in `<head>` for
  the document's lifetime. Enforcement therefore never depends on whether a
  parsed meta-CSP survives removal of the meta element (cross-browser
  behavior the specs leave murky). User content can only add *stricter* CSPs,
  never loosen ours.
- Injection happens only in the hub-client preview path, never in the
  `quarto-core` HTML writer — CLI renders keep working scripts.
- Sandbox flags propagate to nested browsing contexts, so adding
  `allow-scripts` lets third-party embedded iframes in preview content (e.g.
  video embeds) run scripts — the CSP does not inherit into cross-origin
  nested documents. This is normal web behavior (and unbreaks currently
  scriptless embeds), but it is a posture change: noted, accepted, and
  spot-checked manually in Phase 3.
- Nested *same-origin* navigables (`<iframe src="/...">`, `<object>`,
  framesets pointing at hub-origin pages) also gain script execution, and the
  CSP does not follow into non-srcdoc nested documents either. The hub app
  shell is frameable from same origin: nothing in this repo sets
  `X-Frame-Options` on any route (the `DENY` on the `.qmd` route — error
  #2 — comes from production nginx, which lives outside this repo), and the
  issue's console error #1 shows `https://quarto-hub.com/` itself loading in
  the frame in production. So preview content can embed hub pages that then
  run with scripts, same-origin as the hub app. The nested content is the
  hub's own code, so
  practical exploitability is limited, but it weakens preview isolation.
  Mitigations: (a) follow-up strand for CSP `frame-ancestors` on hub HTML
  routes — covers `object`/`embed`, which XFO does not, and also fixes the
  clickjacking exposure error #1 implies (filed in Phase 4); (b) optionally
  add `object-src 'none'` to the injected CSP, decided by the PDF-embed
  spot-check in Phase 3. Note the CSP *does* inherit into
  srcdoc/about:blank nested documents, so that vector stays closed.
- This change converts script-blocking from fail-closed to fail-open: today a
  code path that forgets protection is still saved by the sandbox; after the
  fix, one missed injection silently executes user scripts same-origin.
  Mitigations: both `MorphIframe` payload paths route through the single
  `injectPreviewCsp` call; a dev-mode tripwire in `postProcessIframe` warns
  if the settled document lacks the meta (Phase 2); the disabled
  script-inlining comment is updated so a future reader doesn't strip the
  CSP; and any future srcdoc/innerHTML preview path must route through
  `injectPreviewCsp`. Current srcdoc sites are all covered or correctly
  excluded: `MorphIframe.tsx:269`, `DoubleBufferedIframe.tsx:345,353`, and
  `AboutTab.tsx:191` (keeps no-`allow-scripts`).

Rejected alternatives:

- **`allow-scripts` without CSP** — turns the sandbox into a no-op (classic
  escape combination) and would execute user-content scripts same-origin as the
  hub app. Security regression.
- **Navigation-as-message fallback** (detect the iframe's `load` event in the
  parent, parse `contentWindow.location`, restore content) — works around the
  bug without `allow-scripts`, but is a second, Safari-only code path with
  uncertain behavior after XFO-blocked navigations. Kept as backup if real
  Safari testing shows the CSP workaround insufficient.

### Scope

- **Affected / fixed:** `MorphIframe` (the live HTML preview). Also apply the
  same one-line sandbox change + CSP injection to `DoubleBufferedIframe`
  (`ts-packages/preview-renderer/src/iframe/DoubleBufferedIframe.tsx:347,355`),
  an exported legacy component with the identical pattern.
- **Unaffected:** `Q2PreviewIframe` (q2-preview path) already has
  `allow-scripts`; its `installLinkHandlers` runs inside the iframe's own React
  app. `Q2SandboxedPreviewIframe` renders a JSON dump (no links).
- **Out of scope (follow-up strand):** relative links to non-renderable files
  (e.g. `data.csv`) are intercepted by nobody and blank the iframe in *all*
  browsers. Pre-existing separate bug.

## Work items

### Phase 0 — bookkeeping

- [x] `braid create` a P1 bug strand referencing GitHub issue #128; link this plan
- [x] Copy this plan to `claude-notes/plans/2026-08-17-safari-preview-links.md` per repo convention

### Phase 1 — failing tests first (TDD)

- [x] Unit tests for new util `injectPreviewCsp(html)` in
      `ts-packages/preview-renderer/src/utils/previewCsp.test.ts`:
      injects `<meta http-equiv="Content-Security-Policy" content="script-src 'none'">`
      immediately after the `<!DOCTYPE>` if present, else at byte 0 (the
      parser places a leading `<meta>` in the implied `<head>`); **never
      inserts before `<!DOCTYPE html>`** — anything preceding the DOCTYPE
      triggers Quirks Mode (see the srcdoc comment block in
      `MorphIframe.tsx`); idempotent; leaves existing meta CSPs intact (they
      can only restrict further). Adversarial cases (a "first child of
      `<head>`" string search is spoofable — do not implement it that way):
      head-like markup inside comments/`title`/`script`/`textarea`, uppercase
      `<HEAD>`, no-`<head>` fragment starting with `<script>` — in every case
      the meta must end up as the first element in document order
- [x] Integration test (jsdom, pattern of `iframePostProcessor.integration.test.ts`):
      rendering `MorphIframe` produces an iframe whose `sandbox` contains
      `allow-scripts`, `allow-same-origin`, `allow-popups`, whose `srcdoc`
      has the CSP meta as the first element in document order (before any
      `<script>` in the payload), and whose `<head>` still contains the meta
      after a morphdom content update
- [x] Confirm the new unit/integration tests fail pre-fix (sandbox attribute /
      srcdoc assertions)
- [x] Playwright e2e regression spec `hub-client/e2e/preview-link-navigation.spec.ts`:
      project with `index.qmd` linking to `other.qmd`; click the rendered link
      inside the preview iframe; assert the editor switches to `other.qmd` and
      the preview is not blank. Enable the commented-out `webkit` project in
      `playwright.config.ts` (line ~74) scoped to this spec (per-project
      `testMatch`) so added test time stays minimal. **Verify this spec fails
      on WebKit before the fix** (requires `npx playwright install webkit`)
- [x] Negative security e2e spec `hub-client/e2e/preview-script-blocking.spec.ts`:
      a qmd whose rendered HTML contains a `<script>`, an inline `on*`
      handler, a `javascript:` URL, and a nested `<iframe srcdoc="…">` with a
      script — each attempting to mutate the parent (e.g. set
      `top.document.title`); assert none execute. Extend the webkit project's
      `testMatch` to cover this spec too, and run it under chromium as well:
      jsdom enforces neither the sandbox nor CSP, so the no-scripts guarantee
      can only be pinned in real browsers. This is the regression test for
      the escape combination in "Rejected alternatives"
- [x] Update `.github/workflows/hub-client-e2e.yml` to also install webkit
      (`npx playwright install --with-deps webkit`, alongside chromium at line
      ~165) — the webkit project fails in CI without it; the browser install
      adds ~1-2 min to CI even though test time stays flat

### Phase 2 — implementation

- [x] Add `ts-packages/preview-renderer/src/utils/previewCsp.ts` exporting
      `injectPreviewCsp(html: string): string` and the meta-tag constant,
      implementing the after-DOCTYPE/byte-0 injection contract from Fix
      strategy (not a `<head>` string search)
- [x] `MorphIframe.tsx`: sandbox → `'allow-same-origin allow-scripts allow-popups'`;
      apply `injectPreviewCsp` to `html` once at the top of the content effect
      so **both** the initial `iframe.srcdoc = html` (line 269) and the
      morphdom path (`tempContainer.innerHTML = html`, line 292) carry the
      meta — morphdom then keeps it in `<head>`. Add a comment citing WebKit
      bug 218086 and the inject-on-every-payload rationale
- [x] `DoubleBufferedIframe.tsx`: same sandbox change + CSP injection at its
      content-set site, for consistency of the exported API
- [x] `iframePostProcessor.ts`: update the disabled script-inlining comment
      (lines 176-202) — it currently says re-enabling only needs
      `allow-scripts` in the sandbox; after this fix `allow-scripts` is
      present but the CSP still blocks inlined scripts. Note the CSP so a
      future reader doesn't uncomment the block and strip the CSP in confusion
- [x] `iframePostProcessor.ts`: dev-mode tripwire in `postProcessIframe` — if
      the settled document's `<head>` lacks the injected CSP meta, log a loud
      warning (a missed injection now means user scripts execute same-origin;
      the sandbox no longer catches it). Fail-open guard only; no production
      behavior change
- [x] Re-assert/defensive: in `postProcessIframe`, no change expected — verify
      during implementation that morphdom preserves the injected meta in
      `<head>` (it should: both the settled document and each new payload
      carry it)

### Phase 3 — verification

- [x] `npm run test:ci` in `hub-client/` (unit + integration + wasm)
- [x] `npm run build:all` in `hub-client/` (stricter than tsc --noEmit; required by CLAUDE.md)
- [x] `npx playwright test preview-link-navigation` under chromium (no regression)
      and webkit (fix confirmed); `npx playwright test
      preview-script-blocking` under chromium and webkit (no script
      execution)
- [x] `cargo xtask verify` (full — hub-client build leg affected; ts-packages are
      bundled from source)
- [ ] End-to-end in a real browser per CLAUDE.md: `npm run local-prod`, open
      http://127.0.0.1:8080 in **real Safari**, click a cross-doc link, confirm
      file switch; also confirm scroll sync now works in Safari, and
      spot-check a third-party embed (nested iframe) for the posture change
      noted in Fix strategy, and a PDF `<object>` embed to decide whether
      adding `object-src 'none'` to the injected CSP is safe. If real Safari
      can't be driven from this session,
      say so explicitly and rely on Playwright-WebKit evidence + user
      verification on production
      → **2026-08-18: real Safari could not be driven from the dev session
      (headless). Relying on Playwright-WebKit evidence (both specs green
      on webkit-2287). The third-party-embed / PDF-`<object>` spot-checks
      and the `object-src 'none'` decision remain for user verification on
      production.**
- [x] Note expected cosmetic change: Chrome may log an "allow-scripts +
      allow-same-origin can escape sandboxing" console warning (already present
      for the q2-preview iframe); script tags in rendered HTML now log CSP
      violations instead of sandbox violations

### Phase 4 — repo process

- [x] hub-client `changelog.md` entry (two-commit workflow: change commit, then
      changelog commit with the hash) — the fix lands in `ts-packages/` but
      changes hub-client behavior and touches `hub-client/e2e/`
      (change commit `7bb2d80f`, changelog commit `bf211a03`)
- [x] File follow-up braid strand: unintercepted relative links (non-.qmd)
      blank the preview iframe in all browsers → `bd-ddfyqmfm`
- [x] File follow-up braid strand: send CSP `frame-ancestors` on hub HTML
      routes (production nginx lives outside this repo) — covers `object`/
      `embed` framing, which XFO does not, and closes the same-origin framing
      exposure noted in Fix strategy (also a pre-existing clickjacking fix)
      → `bd-23es17uh`; also filed `bd-rpc3skz0` for the pre-existing
      Q2PreviewIframe inline-handler exposure from the security audit
- [x] `braid close` the strand with a summary (bd-sxx1az83 closed);
      GitHub issue #128 comment skipped per user decision
- [x] Stage and commit; report snapshot/test status; **do not push without
      explicit permission** — commits `7bb2d80f` + `bf211a03` are local on
      `main`, not pushed

## Details

### Security audit

This plan was security-audited on 2026-08-17; findings F1–F4 are incorporated
above (injection-point contract, negative script-blocking e2e, same-origin
nested-frame analysis, fail-open tripwire). One pre-existing issue was flagged
but is out of scope: `Q2PreviewIframe` already ships
`sandbox="allow-scripts allow-same-origin"` with no CSP while rendering user
raw HTML via `dangerouslySetInnerHTML` — script elements inserted via
innerHTML don't execute, but inline event handlers do, same-origin as the hub
app. Candidate for its own follow-up strand.

### Evidence chain

| Symptom | Cause |
|---|---|
| `Blocked script execution … 'allow-scripts' … not set` | Sandbox-without-`allow-scripts` blocking script execution tied to the frame. Note the message names `https://quarto-hub.com/`, whereas WebKit 218086's repro reports `about:srcdoc` — so this line most likely shows scripts on a hub page the frame had *already navigated to* being blocked (which is also the same-origin frameability evidence used in Fix strategy). The listener blocking itself is confirmed by WebKit 218086 and by "works in Chrome", not by this console line |
| `Refused to display …/2026-04-15.qmd … X-Frame-Options DENY` | Un-intercepted click → iframe navigates to href resolved against parent origin; hub sends XFO DENY |
| Blank frame | XFO-blocked navigation replaces the `srcdoc` document |
| Works in Chrome | Chrome runs parent-attached listeners → `preventDefault` → SPA file switch via `onQmdLinkClick` |

### Key files

- `ts-packages/preview-renderer/src/iframe/MorphIframe.tsx` — sandbox attr (line 486), srcdoc assignment (line 269)
- `ts-packages/preview-renderer/src/iframe/DoubleBufferedIframe.tsx` — legacy, same pattern (lines 347, 355)
- `ts-packages/preview-renderer/src/utils/iframePostProcessor.ts` — link handlers installed from parent realm
- `ts-packages/preview-renderer/src/utils/previewCsp.ts` — **new** util
- `hub-client/playwright.config.ts` — enable webkit project (currently commented out, line ~74)
- `hub-client/e2e/` — new regression spec; follow patterns in `project-loading.spec.ts`, helpers in `e2e/helpers/`
- `.github/workflows/hub-client-e2e.yml` — install webkit alongside chromium for CI

### Test-environment caveat

jsdom enforces neither WebKit's sandbox script blocking nor CSP at all, so
vitest can only assert the mechanism (sandbox tokens, CSP meta
presence/position). Both behavioral tests — link navigation and script
blocking — must be Playwright (WebKit is real WebKit; the script-blocking
spec also runs under chromium so the guarantee is pinned in both engines).
