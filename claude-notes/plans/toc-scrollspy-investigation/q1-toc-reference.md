---
title: 'Q1 TOC scroll-tracking reference (bd-toc-scrollspy-js-41e624d8)'
date: 2026-10-30
description: 'The Quarto 1 source the q2 TOC scroll-tracking port is checked against: the render-time TOC decoration in format-html-bootstrap.ts and the TOC subset of quarto.js, quoted with line numbers.'
---

Quoted from `external-sources/quarto-cli` at `fd40c946f` on 2026-10-30. The
port is `resources/js/quarto-toc/quarto-toc.js`; the plan is
`claude-notes/plans/2026-10-30-toc-scrollspy-js.md`.

## Render time: `src/format/html/format-html-bootstrap.ts:398-425`

```ts

      // activate selection behavior for this
      toc.classList.add("toc-active");

      const expanded = format.metadata[kTocExpand];
      if (expanded !== undefined) {
        if (expanded === true) {
          toc.setAttribute("data-toc-expanded", 99);
        } else if (expanded) {
          toc.setAttribute("data-toc-expanded", expanded);
        } else {
          toc.setAttribute("data-toc-expanded", -1);
        }
      }
      // add nav-link class to the TOC links
      const tocLinks = doc.querySelectorAll('nav#TOC[role="doc-toc"] > ul a');

      for (let i = 0; i < tocLinks.length; i++) {
        // Mark the toc links as nav-links
        const tocLink = tocLinks[i] as Element;
        tocLink.classList.add("nav-link");
        if (i === 0) {
          tocLink.classList.add("active");
        }

        // move the raw href to the target attribute (need the raw value, not the full path)
        if (!tocLink.hasAttribute("data-scroll-target")) {
          tocLink.setAttribute(
```

## Browser: `src/resources/formats/html/quarto.js`

### Event and TOC element (lines 4-9, 58)

```js
const sectionChanged = new CustomEvent("quarto-sectionChanged", {
  detail: {},
  bubbles: true,
  cancelable: false,
  composed: false,
});

  const tocEl = window.document.querySelector('nav.toc-active[role="doc-toc"]');
```

### `prevSiblingIsActiveLink` (lines 64-72)

```js
  // function to determine whether the element has a previous sibling that is active
  const prevSiblingIsActiveLink = (el) => {
    const sibling = el.previousElementSibling;
    if (sibling && sibling.tagName === "A") {
      return sibling.classList.contains("active");
    } else {
      return false;
    }
  };
```

### Active-link tracking (lines 110-186)

Lines 121-143 (the anchor reveal on click) are **not ported**: q2 has no
anchor-sections, so there is no `[data-anchor-id]` to reveal.

```js
  // Track scrolling and mark TOC links as active
  // get table of contents and sidebar (bail if we don't have at least one)
  const tocLinks = tocEl
    ? [...tocEl.querySelectorAll("a[data-scroll-target]")]
    : [];
  const makeActive = (link) => tocLinks[link].classList.add("active");
  const removeActive = (link) => tocLinks[link].classList.remove("active");
  const removeAllActive = () =>
    [...Array(tocLinks.length).keys()].forEach((link) => removeActive(link));

  // activate the anchor for a section associated with this TOC entry
  tocLinks.forEach((link) => {
    link.addEventListener("click", () => {
      if (link.href.indexOf("#") !== -1) {
        const anchor = link.href.split("#")[1];
        const heading = window.document.querySelector(
          `[data-anchor-id="${anchor}"]`
        );
        if (heading) {
          // Add the class
          heading.classList.add("reveal-anchorjs-link");

          // function to show the anchor
          const handleMouseout = () => {
            heading.classList.remove("reveal-anchorjs-link");
            heading.removeEventListener("mouseout", handleMouseout);
          };

          // add a function to clear the anchor when the user mouses out of it
          heading.addEventListener("mouseout", handleMouseout);
        }
      }
    });
  });

  const sections = tocLinks.map((link) => {
    const target = link.getAttribute("data-scroll-target");
    if (target.startsWith("#")) {
      return window.document.getElementById(decodeURI(`${target.slice(1)}`));
    } else {
      return window.document.querySelector(decodeURI(`${target}`));
    }
  });

  const sectionMargin = 200;
  let currentActive = 0;
  // track whether we've initialized state the first time
  let init = false;

  const updateActiveLink = () => {
    // The index from bottom to top (e.g. reversed list)
    let sectionIndex = -1;
    if (
      window.innerHeight + window.pageYOffset >=
      window.document.body.offsetHeight
    ) {
      // This is the no-scroll case where last section should be the active one
      sectionIndex = 0;
    } else {
      // This finds the last section visible on screen that should be made active
      sectionIndex = [...sections].reverse().findIndex((section) => {
        if (section) {
          return window.pageYOffset >= section.offsetTop - sectionMargin;
        } else {
          return false;
        }
      });
    }
    if (sectionIndex > -1) {
      const current = sections.length - sectionIndex - 1;
      if (current !== currentActive) {
        removeAllActive();
        currentActive = current;
        makeActive(current);
        if (init) {
          window.dispatchEvent(sectionChanged);
        }
```

### Expand / collapse walk and the listeners (lines 771-850)

```js
  const tocOpenDepthStr = tocEl?.getAttribute("data-toc-expanded");
  const tocOpenDepth = tocOpenDepthStr ? Number(tocOpenDepthStr) : 1;

  // Walk the TOC and collapse/expand nodes
  // Nodes are expanded if:
  // - they are top level
  // - they have children that are 'active' links
  // - they are directly below an link that is 'active'
  const walk = (el, depth) => {
    // Tick depth when we enter a UL
    if (el.tagName === "UL") {
      depth = depth + 1;
    }

    // It this is active link
    let isActiveNode = false;
    if (el.tagName === "A" && el.classList.contains("active")) {
      isActiveNode = true;
    }

    // See if there is an active child to this element
    let hasActiveChild = false;
    for (const child of el.children) {
      hasActiveChild = walk(child, depth) || hasActiveChild;
    }

    // Process the collapse state if this is an UL
    if (el.tagName === "UL") {
      if (tocOpenDepth === -1 && depth > 1) {
        // toc-expand: false
        el.classList.add("collapse");
      } else if (
        depth <= tocOpenDepth ||
        hasActiveChild ||
        prevSiblingIsActiveLink(el)
      ) {
        el.classList.remove("collapse");
      } else {
        el.classList.add("collapse");
      }

      // untick depth when we leave a UL
      depth = depth - 1;
    }
    return hasActiveChild || isActiveNode;
  };

  // walk the TOC and expand / collapse any items that should be shown
  if (tocEl) {
    updateActiveLink();
    walk(tocEl, 0);
  }

  // Throttle the scroll event and walk peridiocally
  window.document.addEventListener(
    "scroll",
    throttle(() => {
      if (tocEl) {
        updateActiveLink();
        walk(tocEl, 0);
      }
      if (!isReaderMode()) {
        hideOverlappedSidebars();
      }
    }, 5)
  );
  window.addEventListener(
    "resize",
    throttle(() => {
      if (tocEl) {
        updateActiveLink();
        walk(tocEl, 0);
      }
      if (!isReaderMode()) {
        hideOverlappedSidebars();
      }
    }, 10)
  );
  hideOverlappedSidebars();
  highlightReaderToggle(isReaderMode());
```

### `throttle` (lines 854-882)

```js

// With `trailing`, a call dropped during the wait runs once the wait ends, so
// the last event of a burst is never lost.
function throttle(func, wait, trailing = false) {
  let waiting = false;
  let pending = null;
  const release = function () {
    if (pending) {
      const [context, args] = pending;
      pending = null;
      func.apply(context, args);
      setTimeout(release, wait);
    } else {
      waiting = false;
    }
  };
  return function () {
    if (waiting) {
      if (trailing) {
        pending = [this, arguments];
      }
      return;
    }
    func.apply(this, arguments);
    waiting = true;
    setTimeout(release, wait);
  };
}

```

## Not ported (separate features)

- `manageSidebarVisiblity` / `hideOverlappedSidebars` (lines 357-520,
  580-600, 690-705): the TOC overlap rollup. Its CSS
  (`.quarto-sidebar-toggle*`) is present but unreferenced in q2.
- Reader mode (`window.quartoToggleReader`, lines 708-770): q2 renders no
  reader toggle.
