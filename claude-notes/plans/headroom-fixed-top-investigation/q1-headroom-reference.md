# Headroom.js in Quarto 1 — end-to-end reference (for bd-ersobfbt)

Source: `external-sources/quarto-cli` @ `45caede32` (1.10.15). All paths below
are relative to that tree. Collected 2026-08-21.

**Short version:** the library is vendored, conditionally added as an HTML
dependency script, and `#quarto-header` is *always* emitted with
`class="headroom fixed-top"`. Whether the scroll-away happens is decided
entirely by whether `headroom.min.js` was shipped — the client init is
guarded by `if (header && window.Headroom)`. `pinned: true` simply omits
the script; there is no runtime flag.

## 1. Shipping

- Vendored: `src/resources/projects/website/navigation/headroom.min.js`
  (4570 bytes, headroom.js **v0.12.0**, MIT, Nick Williams).
- Upstream defaults: `tolerance {up:0,down:0}`, `offset 0`, `scroller window`,
  classes `headroom--pinned / --unpinned / --top / --not-top / --bottom /
  --not-bottom / --frozen`, initial `headroom`.
- Predicates: `shouldUnpin = direction==="down" && !top && toleranceExceeded`;
  `shouldPin = (direction==="up" && toleranceExceeded) || top`.
- Dependency wiring, `src/project/types/website/website-navigation.ts:1500-1541`:

```ts
async function websiteHeadroom(project: ProjectContext) {
  const { navbar, sidebars } = await websiteNavigationConfig(project);
  if (navbar || sidebars?.length) {
    const navbarPinned = navbar?.pinned === true;
    const anySidebarPinned = sidebars &&
      sidebars.some((sidebar) => sidebar.pinned === true);
    return !navbarPinned && !anySidebarPinned;
  } else {
    return false;
  }
}
const kDependencyName = "quarto-nav";
async function websiteNavigationDependency(project: ProjectContext) {
  const scripts = [navigationDependency("quarto-nav.js")];
  if (await websiteHeadroom(project)) {
    scripts.push(navigationDependency("headroom.min.js"));
  }
  return { name: kDependencyName, scripts };
}
```

  Lands as `site_libs/quarto-nav/{quarto-nav.js,headroom.min.js}`.
- Other shippers: manuscript notebooks (`src/project/types/manuscript/manuscript.ts:586-598`)
  and the embedded-notebook viewer (`src/resources/formats/html/embed/template.html:69-119`,
  its own inline init, empty callbacks).

## 2. Template

`src/resources/projects/website/templates/nav-before-body.ejs:14-15`, close at `:99-100`:

```ejs
<% if (nav.navbar || nav.sidebar || nav.announcement) { %>
<header id="quarto-header" class="headroom fixed-top">
```

Unconditional literal classes. Contents in order: announcement
(above-navbar), `nav.navbar`, `nav.quarto-secondary-nav` (when
`nav.sidebar && nav.layout`), announcement (below-navbar).

Freeze hooks (`nav-before-body.ejs:66-84`, secondary-nav toggle button,
breadcrumb `<a>`, title `<a>`; and `navtoggle.ejs:1-5`, the navbar hamburger):

```html
onclick="if (window.quartoToggleHeadroom) { window.quartoToggleHeadroom(); }"
```

## 3. Client init — `src/resources/projects/website/navigation/quarto-nav.js`

NOT in `quarto.js`. Init block `:202-238`:

```js
  var header = window.document.querySelector("#quarto-header");
  if (header && window.Headroom) {
    const headroom = new window.Headroom(header, {
      tolerance: 5,
      onPin: function () {
        const sidebars = window.document.querySelectorAll(".sidebar, .headroom-target");
        sidebars.forEach((sidebar) => { sidebar.classList.remove("sidebar-unpinned"); });
        updateDocumentOffset();
      },
      onUnpin: function () {
        const sidebars = window.document.querySelectorAll(".sidebar, .headroom-target");
        sidebars.forEach((sidebar) => { sidebar.classList.add("sidebar-unpinned"); });
        updateDocumentOffset();
      },
    });
    headroom.init();

    let frozen = false;
    window.quartoToggleHeadroom = function () {
      if (frozen) { headroom.unfreeze(); frozen = false; }
      else        { headroom.freeze();   frozen = true;  }
    };
  }
```

Offset machinery `:105-200`:

```js
  function headerOffset() {
    const headerEl = window.document.querySelector("header.fixed-top");
    return headerEl ? headerEl.clientHeight : 0;
  }
  // footerOffset() :114, dashboardOffset() :123 analogous

  function updateDocumentOffset(animated) {
    const topOffset = headerOffset();
    const bodyOffset = topOffset + footerOffset() + dashboardOffset();
    const bodyEl = window.document.body;
    bodyEl.setAttribute("data-bs-offset", topOffset);
    bodyEl.style.paddingTop = topOffset + "px";

    const sidebars = window.document.querySelectorAll(".sidebar, .headroom-target");
    sidebars.forEach((sidebar) => {
      if (!animated) {
        sidebar.classList.add("notransition");
        setTimeout(function () { sidebar.classList.remove("notransition"); }, 201);
      }
      if (window.Headroom && sidebar.classList.contains("sidebar-unpinned")) {
        sidebar.style.top = "0";
        sidebar.style.maxHeight = "100vh";
      } else {
        sidebar.style.top = topOffset + "px";
        sidebar.style.maxHeight = "calc(100vh - " + topOffset + "px)";
      }
    });

    const mainContainer = window.document.querySelector(".quarto-container");
    if (mainContainer) {
      mainContainer.style.minHeight = "calc(100vh - " + bodyOffset + "px)";
    }

    // anchor-jump compensation: a dynamic <style id="quarto-target-style">
    //   section:target::before { content:""; display:block;
    //                            height:${topOffset}px; margin:-${topOffset}px 0 0; }
    if (init) { window.dispatchEvent(headroomChanged); }   // "quarto-hrChanged", :1-6
    init = true;
  }
```

Notes: `clientHeight` ignores the `translateY` transform, so the body
padding is constant across pin/unpin; `.headroom-target` is the opt-in
marker for non-`.sidebar` fixed elements (the sidebar-rollup toggle built in
`quarto.js:352-360`); the first `updateDocumentOffset` does not dispatch the
event.

Also in the file: `hashchange` listener (`:240-251`) that does
`window.scrollTo(0, pageYOffset - headerOffset())` when scroll-behavior is
not `smooth`; a `ResizeObserver` on `header.fixed-top` (`:252-269`, falls back
to throttled `resize`) → `updateDocumentOffsetWithoutAnimation`; initial
measurement via `setTimeout(..., 250)`.

`quarto.js` consumer: `:445-447` listens for `quarto-hrChanged` to invalidate
the cached rect used by `positionToggle()`.

## 4. `body.nav-fixed` postprocessor

`src/project/types/website/website-navigation.ts:546-554`:

```ts
    const headerEl = doc.body.querySelector("#quarto-header.fixed-top nav.navbar");
    if (headerEl) { doc.body.classList.add("nav-fixed"); }
```

Requires a `nav.navbar` descendant — sidebar-only sites get `fixed-top` but
not `nav-fixed` (no static padding pre-guess). Other consumer of the class:
`quarto-dashboard.scss:399-402` only.

## 5. SCSS — `src/resources/projects/website/navigation/quarto-nav.scss`

`:100-114`:

```scss
.headroom-target,
header.headroom {
  will-change: transform;
  transition: position 200ms linear;   // dead line
  transition: all 200ms linear;
}
header.headroom--pinned   { transform: translateY(0%); }
header.headroom--unpinned { transform: translateY(-100%); }
```

`:1-33` (functions layer):

```scss
@function navbar-default-offset($theme) {
  $offsets: (darkly: 82px, flatly: 82px, litera: 67px, lumen: 68px, lux: 105px,
             materia: 96px, pulse: 89px, quartz: 82px, sandstone: 63px,
             simplex: 80px, sketchy: 68px, slate: 66px, zephyr: 76px);
  $val: null;
  @if ($theme != null) { $val: quarto-map.get($offsets, $theme); }
  @if ($val != null) { @return $val; } @else { @return 64px; }
}
```

`:820-824`:

```scss
body.nav-fixed { padding-top: navbar-default-offset($theme-name); }
```

`:727-732`: `.notransition { transition: none !important; }` (+ vendor prefixes).

`:51-67`: `.quarto-container { min-height: calc(100vh - 132px); }`,
`body.hypothesis-enabled #quarto-header { margin-right: 16px; }`,
`footer.footer .nav-footer, #quarto-header > nav { padding: 0 1em }`
(the last already ported to q2 `_bootstrap-rules.scss:3068`).

`src/resources/formats/html/_quarto-rules.scss:744-758` print:
`.fixed-top { position: relative; }` (q2 explicitly skipped it — `_bootstrap-rules.scss:457`).

No `.sidebar-unpinned` rule exists in SCSS (JS-only state flag); no
`--quarto-header-height` custom property anywhere.

## 6. YAML `pinned`

`src/resources/schema/definitions.yml:1038-1041` (navbar: "Always show the
navbar (keeping it pinned).", default false) and `:1141-1144` (sidebar:
"When collapsed, pin the collapsed sidebar to the top of the page."). Both
normalised to booleans (`website-navigation.ts:1251`, `:1031`) and read only
by `websiteHeadroom()`. Build-time, project-wide; no per-page override.

q2 already parses both: `crates/quarto-navigation/src/navbar.rs:162` and
`sidebar.rs:404`; nothing consumes them yet.

## 7. Interplay

- Reader mode (`quarto.js:480-482`) forces the sidebar into the
  `.headroom-target` rollup; the header itself is never hidden by reader mode.
- Dark-mode toggle (`quarto-html-before-body.ejs:90-100`) shares the
  `.notransition` class vocabulary.
- Search: no coupling beyond the search button not freezing headroom.
- Title banner: `format-html-title.ts:277-281` adds `.quarto-banner` to the
  header (taller header — the static offset guess cannot predict it).
- Draft alert (`format-html.ts:919-925`) and announcement bar
  (`quarto-nav.js:8-39`) mutate header height post-load → ResizeObserver.
- TOC active-item (`quarto.js:149-184`) uses a flat 200 px `sectionMargin`,
  not header height.
- `llms.lua:33` strips `quarto-header` from llms.txt output.
