/*!
 * quarto-nav.js — fixed-header offset management + headroom.js wiring.
 *
 * q2 port of the header-machinery subset of Quarto 1's
 * `src/resources/projects/website/navigation/quarto-nav.js`
 * (bd-ersobfbt). See claude-notes/plans/headroom-fixed-top-investigation/
 * q1-headroom-reference.md for the Q1 original, quoted in full.
 *
 * The `#quarto-header` element is `position: fixed` (class `fixed-top`),
 * so this script pushes the page down by the header's measured height:
 * body padding, sticky-sidebar top/max-height, container min-height, and
 * an anchor-jump spacer. The SCSS `body.nav-fixed { padding-top: … }`
 * rule is only the pre-JS anti-flash guess; the values written here are
 * authoritative and re-computed on header resize.
 *
 * When `headroom.min.js` is also shipped (i.e. neither the navbar nor
 * the sidebar is `pinned`), the header additionally hides on scroll-down
 * and reappears on scroll-up, and `window.quartoToggleHeadroom` lets the
 * sidebar/navbar toggles freeze it while a collapse menu is open.
 *
 * Deliberate deviations from Q1 (each because q2 lacks the consumer;
 * revisit when porting the corresponding feature):
 * - no `.headroom-target` selector — q2 has no sidebar-rollup toggle;
 * - no `footerOffset()` / `dashboardOffset()` — no fixed footer or
 *   dashboard header exists in q2;
 * - no `quarto-hrChanged` CustomEvent — no consumer;
 * - no announcement-bar registration — feature not ported.
 *
 * NOTE (bd-pt1wxeq2): this whole file is slated to be replaced by a
 * `position: sticky` + `--quarto-header-height` design. Keep it
 * self-contained.
 */
window.document.addEventListener("DOMContentLoaded", function () {
  function headerOffset() {
    // Measure the fixed header. `clientHeight` ignores the headroom
    // `translateY` transform, so the value is stable across pin/unpin
    // and the body does not reflow when the header slides away.
    const headerEl = window.document.querySelector("header.fixed-top");
    if (headerEl) {
      return headerEl.clientHeight;
    } else {
      return 0;
    }
  }

  function updateDocumentOffset(animated) {
    const topOffset = headerOffset();
    const bodyEl = window.document.body;
    bodyEl.setAttribute("data-bs-offset", topOffset);
    bodyEl.style.paddingTop = topOffset + "px";

    // Sticky sidebars (nav sidebar, margin TOC) sit below the header
    // while it is pinned, and reclaim the space when it slides away.
    const sidebars = window.document.querySelectorAll(".sidebar");
    sidebars.forEach((sidebar) => {
      if (!animated) {
        // Suppress the `transition: top 200ms` while applying a
        // non-animated update (initial load, resize).
        sidebar.classList.add("notransition");
        setTimeout(function () {
          sidebar.classList.remove("notransition");
        }, 201);
      }

      if (window.Headroom && sidebar.classList.contains("sidebar-unpinned")) {
        sidebar.style.top = "0";
        sidebar.style.maxHeight = "100vh";
      } else {
        sidebar.style.top = topOffset + "px";
        sidebar.style.maxHeight = "calc(100vh - " + topOffset + "px)";
      }
    });

    // Keep the page at least viewport-height below the header.
    const mainContainer = window.document.querySelector(".quarto-container");
    if (mainContainer) {
      mainContainer.style.minHeight = "calc(100vh - " + topOffset + "px)";
    }

    // Anchor-jump compensation: a `section:target` pseudo-spacer the
    // height of the header, so jumped-to sections aren't hidden
    // beneath it.
    let linkStyle = window.document.querySelector("#quarto-target-style");
    if (!linkStyle) {
      linkStyle = window.document.createElement("style");
      linkStyle.setAttribute("id", "quarto-target-style");
      window.document.head.appendChild(linkStyle);
    }
    while (linkStyle.firstChild) {
      linkStyle.removeChild(linkStyle.firstChild);
    }
    if (topOffset > 0) {
      linkStyle.appendChild(
        window.document.createTextNode(`
      section:target::before {
        content: "";
        display: block;
        height: ${topOffset}px;
        margin: -${topOffset}px 0 0;
      }`)
      );
    }
  }

  function updateDocumentOffsetWithoutAnimation() {
    updateDocumentOffset(false);
  }

  function updateDocumentOffsetWithAnimation() {
    updateDocumentOffset(true);
  }

  // Initialize headroom (scroll-away header). Guarded: when
  // headroom.min.js is not shipped (`pinned: true`), the header stays
  // fixed and only the offset management above runs.
  const header = window.document.querySelector("#quarto-header");
  if (header && window.Headroom) {
    const headroom = new window.Headroom(header, {
      tolerance: 5,
      onPin: function () {
        const sidebars = window.document.querySelectorAll(".sidebar");
        sidebars.forEach((sidebar) => {
          sidebar.classList.remove("sidebar-unpinned");
        });
        updateDocumentOffsetWithAnimation();
      },
      onUnpin: function () {
        const sidebars = window.document.querySelectorAll(".sidebar");
        sidebars.forEach((sidebar) => {
          sidebar.classList.add("sidebar-unpinned");
        });
        updateDocumentOffsetWithAnimation();
      },
    });
    headroom.init();

    let frozen = false;
    window.quartoToggleHeadroom = function () {
      if (frozen) {
        headroom.unfreeze();
        frozen = false;
      } else {
        headroom.freeze();
        frozen = true;
      }
    };
  }

  // Non-smooth-scroll hash navigation lands under the fixed header;
  // compensate. (Smooth scrolling is handled by the CSS spacer above.)
  window.addEventListener(
    "hashchange",
    function () {
      if (
        getComputedStyle(document.documentElement).scrollBehavior !== "smooth"
      ) {
        window.scrollTo(0, window.pageYOffset - headerOffset());
      }
    },
    false
  );

  // Re-measure whenever the header's size changes (banner mode, wrap on
  // narrow viewports, fonts loading, …).
  const headerEl = window.document.querySelector("header.fixed-top");
  if (headerEl && window.ResizeObserver) {
    const observer = new window.ResizeObserver(() => {
      setTimeout(updateDocumentOffsetWithoutAnimation, 0);
    });
    observer.observe(headerEl, {
      attributes: true,
      childList: true,
      characterData: true,
    });
  } else {
    let resizeTimer = null;
    window.addEventListener("resize", function () {
      if (resizeTimer === null) {
        resizeTimer = setTimeout(function () {
          resizeTimer = null;
          updateDocumentOffsetWithoutAnimation();
        }, 50);
      }
    });
  }
  // Initial measurement, after layout settles.
  setTimeout(updateDocumentOffsetWithoutAnimation, 250);
});
