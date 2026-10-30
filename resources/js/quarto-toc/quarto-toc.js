/*!
 * quarto-toc.js — "On this page" scroll tracking for the HTML table of
 * contents.
 *
 * q2 port of the TOC subset of Quarto 1's
 * `src/resources/formats/html/quarto.js` (bd-toc-scrollspy-js-41e624d8).
 * See claude-notes/plans/toc-scrollspy-investigation/q1-toc-reference.md
 * for the Q1 original, quoted with line numbers.
 *
 * Behavior, unchanged from Q1:
 * - The TOC link whose section is the last one to have scrolled past
 *   `SECTION_MARGIN` px below the top of the viewport is `.active`; at
 *   the bottom of the page the last link is active. A change dispatches
 *   `quarto-sectionChanged` on `window` (after the initial pass).
 * - Nested `<ul>`s collapse (Bootstrap's `.collapse`) unless they are
 *   within the `data-toc-expanded` depth (default 1; `toc-expand: true`
 *   is 99, `toc-expand: false` is -1), hold the active link, or directly
 *   follow it. `data-toc-expanded` is read from `nav#TOC` or any
 *   descendant: q2 writes it on the top-level `<ul>` (one producer,
 *   `transforms/toc_render.rs`), Q1 wrote it on the nav.
 * - Both run at load and on throttled `scroll` (5 ms) and `resize`
 *   (10 ms) events.
 *
 * Deliberate deviations from Q1 (each because q2 lacks the consumer;
 * revisit when porting the corresponding feature):
 * - no anchor reveal on link click (`reveal-anchorjs-link`): q2 has no
 *   anchor-sections, so there is no `[data-anchor-id]` to reveal;
 * - no TOC overlap rollup (`manageSidebarVisiblity`) and no reader mode
 *   (`quartoToggleReader`): separate features, not ported yet.
 *
 * One deliberate q2 FIX over Q1: above the first section (the title
 * block, when the first heading sits more than `SECTION_MARGIN` px down
 * the page) Q1 finds no qualifying section and leaves whichever link was
 * last active; scrolling back to the top of a long page kept the last
 * entry highlighted. Here the first entry is active in that case, which
 * is also what the render-time `active` on the first link shows before
 * the script runs.
 *
 * Three deliberate q2 EXTENSIONS over Q1:
 * - The scroll container is resolved per pass rather than assumed to be
 *   the window: the nearest scrollable ancestor of the first section
 *   (`overflow-y: auto|scroll` with overflow), or the window when there
 *   is none, which is every native page. The hub-client preview scrolls
 *   `#root` inside a viewport-sized iframe, where Q1's
 *   `innerHeight + pageYOffset >= body.offsetHeight` is always true.
 *   Scroll events are observed on the document in the capture phase, so
 *   one listener sees both window and container scrolls.
 * - Binding is keyed off the TOC *elements*, re-checked via a
 *   MutationObserver on `document.body`. Q1 binds once at
 *   DOMContentLoaded because its TOC is static server markup; in the
 *   hub-client preview the same file is injected into a persistent
 *   iframe where React mounts `nav#TOC` after the first UPDATE_AST and
 *   replaces its `<ul>` on every heading edit. `sync` rebinds whenever
 *   the nav or its first list changes identity, tearing down the
 *   previous listeners first. On native pages the observer fires
 *   rarely and the identity check is two querySelectors.
 * - Section elements are looked up on every pass rather than once at
 *   bind time, so a body re-render in the preview (which replaces the
 *   `<section>` elements) cannot leave the tracker holding detached
 *   nodes.
 *
 * A `nav#TOC` that carries `data-quarto-component` is owned by a bound
 * component (`toc-component:` in metadata) and is left alone.
 */
(function () {
  const TOC_SELECTOR = 'nav.toc-active[role="doc-toc"]';
  const SECTION_MARGIN = 200;

  const sectionChanged = new CustomEvent("quarto-sectionChanged", {
    detail: {},
    bubbles: true,
    cancelable: false,
    composed: false,
  });

  // Q1's throttle without the `trailing` option (the TOC listeners did
  // not use it).
  function throttle(func, wait) {
    let waiting = false;
    return function () {
      if (!waiting) {
        func.apply(this, arguments);
        waiting = true;
        setTimeout(function () {
          waiting = false;
        }, wait);
      }
    };
  }

  // whether the element has a previous sibling that is an active link
  const prevSiblingIsActiveLink = (el) => {
    const sibling = el.previousElementSibling;
    if (sibling && sibling.tagName === "A") {
      return sibling.classList.contains("active");
    } else {
      return false;
    }
  };

  // Resolve a link's section: `#id` by id (decoded), anything else as a
  // selector. Looked up per pass, see the header.
  const sectionFor = (link) => {
    const target = link.getAttribute("data-scroll-target");
    if (!target) {
      return null;
    }
    if (target.startsWith("#")) {
      return window.document.getElementById(decodeURI(`${target.slice(1)}`));
    } else {
      return window.document.querySelector(decodeURI(`${target}`));
    }
  };

  // The nearest scrollable ancestor of `el`, or null for the window.
  const scrollContainerOf = (el) => {
    let node = el ? el.parentElement : null;
    while (node && node !== window.document.body) {
      const style = window.getComputedStyle(node);
      if (
        /(auto|scroll)/.test(style.overflowY) &&
        node.scrollHeight > node.clientHeight
      ) {
        return node;
      }
      node = node.parentElement;
    }
    return null;
  };

  // Where the reader is: the scroll offset, the viewport height, the
  // total height, and a function giving a section's top in the same
  // coordinate space. Q1's window-only arithmetic, generalized.
  const viewportOf = (sections) => {
    const first = sections.find((s) => s);
    const container = scrollContainerOf(first);
    if (!container) {
      return {
        top: window.pageYOffset,
        height: window.innerHeight,
        total: window.document.body.offsetHeight,
        topOf: (section) => section.offsetTop,
      };
    }
    const origin = container.getBoundingClientRect().top - container.scrollTop;
    return {
      top: container.scrollTop,
      height: container.clientHeight,
      total: container.scrollHeight,
      topOf: (section) => section.getBoundingClientRect().top - origin,
    };
  };

  // Bind the tracker to one TOC element; returns the teardown.
  function bind(tocEl) {
    const tocLinks = [...tocEl.querySelectorAll("a[data-scroll-target]")];
    const makeActive = (link) => tocLinks[link].classList.add("active");
    const removeActive = (link) => tocLinks[link].classList.remove("active");
    const removeAllActive = () =>
      [...Array(tocLinks.length).keys()].forEach((link) => removeActive(link));

    const expandedEl = tocEl.hasAttribute("data-toc-expanded")
      ? tocEl
      : tocEl.querySelector("[data-toc-expanded]");
    const tocOpenDepthStr = expandedEl
      ? expandedEl.getAttribute("data-toc-expanded")
      : null;
    const tocOpenDepth = tocOpenDepthStr ? Number(tocOpenDepthStr) : 1;

    let currentActive = 0;
    // track whether we've initialized state the first time
    let init = false;

    const updateActiveLink = () => {
      const sections = tocLinks.map(sectionFor);
      const view = viewportOf(sections);
      // The index from bottom to top (e.g. reversed list)
      let sectionIndex = -1;
      if (view.height + view.top >= view.total) {
        // This is the no-scroll case where last section should be the active one
        sectionIndex = 0;
      } else {
        // This finds the last section visible on screen that should be made active
        sectionIndex = [...sections].reverse().findIndex((section) => {
          if (section) {
            return view.top >= view.topOf(section) - SECTION_MARGIN;
          } else {
            return false;
          }
        });
      }
      // No section has scrolled past yet (we are above the first one):
      // the first entry is current. Q1 left the previous state here.
      const current =
        sectionIndex > -1 ? sections.length - sectionIndex - 1 : 0;
      if (sections.length > 0) {
        if (current !== currentActive || !init) {
          removeAllActive();
          currentActive = current;
          makeActive(current);
          if (init) {
            window.dispatchEvent(sectionChanged);
          }
          init = true;
        }
      }
    };

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

    const update = () => {
      updateActiveLink();
      walk(tocEl, 0);
    };

    update();
    const onScroll = throttle(update, 5);
    const onResize = throttle(update, 10);
    // Capture phase: scroll events do not bubble, and the preview's
    // scroll container is an element, not the window (see the header).
    window.document.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);

    return () => {
      window.document.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
    };
  }

  let boundNav = null;
  let boundList = null;
  let unbind = null;

  // Bind to the page's TOC if it (or its list) is not the one we are
  // bound to; tear down a previous binding first.
  function sync() {
    // The observer can fire after a test harness has torn the window
    // down (jsdom); there is nothing to bind to then.
    if (!window.document || !window.document.body) {
      return;
    }
    let nav = window.document.querySelector(TOC_SELECTOR);
    if (nav && nav.hasAttribute("data-quarto-component")) {
      nav = null;
    }
    const list = nav ? nav.querySelector("ul") : null;
    if (nav === boundNav && list === boundList) {
      return;
    }
    if (unbind) {
      unbind();
      unbind = null;
    }
    boundNav = nav;
    boundList = list;
    if (nav) {
      unbind = bind(nav);
    }
  }

  function main() {
    sync();
    if (window.MutationObserver) {
      new MutationObserver(() => sync()).observe(window.document.body, {
        childList: true,
        subtree: true,
      });
    }
  }

  if (window.document.readyState === "loading") {
    window.document.addEventListener("DOMContentLoaded", main, { once: true });
  } else {
    main();
  }
})();
