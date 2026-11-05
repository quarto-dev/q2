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
 * - no `dashboardOffset()`: Q1 adds the dashboard header\'s height on
 *   top of the site header that already contains it (double-counted);
 * - `footerOffset()` only on dashboard pages (see `updateDocumentOffset`);
 * - no `quarto-hrChanged` CustomEvent — no consumer.
 *
 * The announcement bar (`#quarto-announcement`,
 * bd-website-announcement-w1gtpzki) is Q1's `announceRegister` /
 * `announceDismiss`, with two q2 changes: a banner marked
 * `data-announcement-remember="false"` closes for the current page view
 * only (no localStorage read or write), and registration is keyed off
 * the banner *element* via the same MutationObserver as the header (see
 * below), so a banner the preview re-mounts is revealed again.
 *
 * One deliberate q2 EXTENSION over Q1: init is keyed off the header
 * *element*, re-checked via a MutationObserver on `document.body`. Q1
 * can bind once at DOMContentLoaded because its header is static
 * server markup; in the hub-client preview the same file is injected
 * into a persistent iframe where React mounts `#quarto-header` only
 * after the first UPDATE_AST arrives — and may unmount/remount it when
 * a `_quarto.yml` edit adds or removes the navbar. `initForHeader`
 * tears down the previous Headroom/observer and rebinds whenever the
 * header's element identity changes. On native pages the observer
 * fires rarely and the identity check is a single querySelector.
 *
 * NOTE (bd-pt1wxeq2): this whole file is slated to be replaced by a
 * `position: sticky` + `--quarto-header-height` design. Keep it
 * self-contained.
 */
(function () {
  function quartoNavMain() {
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

    // Keep the page at least viewport-height below the header. On a
    // dashboard page (a fill layout) the footer is subtracted too, as in
    // Q1, so it stays on screen below the dashboard instead of being
    // pushed past the fold (dashboard plan Phase 9). Other pages keep the
    // header-only offset (bd-f19dbajh).
    const mainContainer = window.document.querySelector(".quarto-container");
    if (mainContainer) {
      const footerEl = window.document.body.classList.contains("quarto-dashboard")
        ? window.document.querySelector("footer.footer")
        : null;
      const offset = topOffset + (footerEl ? footerEl.clientHeight : 0);
      mainContainer.style.minHeight = "calc(100vh - " + offset + "px)";
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

  // --- announcement bar ---------------------------------------------------
  // The banner ships with class `hidden`; reveal it unless a remembered
  // dismissal applies. localStorage access is guarded: it throws in some
  // privacy modes, and the banner must still work there.
  let currentAnnouncement = null;

  function announcementRemembers(el) {
    return el.getAttribute("data-announcement-remember") !== "false";
  }

  function announcementKey(el) {
    return "quarto-announce-" + el.getAttribute("data-announcement-id");
  }

  function announcementDismissed(el) {
    try {
      return !!window.localStorage.getItem(announcementKey(el));
    } catch (e) {
      return false;
    }
  }

  function dismissAnnouncement(el) {
    el.remove();
    if (announcementRemembers(el)) {
      try {
        window.localStorage.setItem(announcementKey(el), "true");
      } catch (e) {
        // Not persisted; the banner is still closed for this view.
      }
    }
  }

  function registerAnnouncement(el) {
    if (el === currentAnnouncement) {
      return;
    }
    currentAnnouncement = el;
    if (!el) {
      return;
    }
    if (announcementRemembers(el) && announcementDismissed(el)) {
      el.remove();
      return;
    }
    el.classList.remove("hidden");
    const actionEl = el.querySelector(".quarto-announcement-action");
    if (actionEl) {
      actionEl.addEventListener("click", function (e) {
        e.preventDefault();
        dismissAnnouncement(el);
      });
    }
  }

  // --- header binding -----------------------------------------------------
  // State for the currently-bound header element (see the q2-extension
  // note in the file header).
  let currentHeader = null;
  let headroom = null;
  let resizeObserver = null;

  function teardown() {
    if (headroom) {
      headroom.destroy();
      headroom = null;
      delete window.quartoToggleHeadroom;
    }
    if (resizeObserver) {
      resizeObserver.disconnect();
      resizeObserver = null;
    }
  }

  function initForHeader(header) {
    if (header === currentHeader) {
      return;
    }
    teardown();
    currentHeader = header;
    if (!header) {
      // Header removed (e.g. preview edit dropped the navbar): reset
      // the offsets so the page reclaims the padding.
      updateDocumentOffsetWithoutAnimation();
      return;
    }

    // Initialize headroom (scroll-away header). Guarded twice: when
    // headroom.min.js is not shipped (native `pinned: true`), and when
    // the header is tagged `data-headroom-pinned` (the hub-client
    // preview's pinned analogue — its bundle always contains headroom,
    // so PreviewDocument tags the header instead). Either way the
    // header stays fixed and only the offset management runs.
    if (window.Headroom && !header.hasAttribute("data-headroom-pinned")) {
      headroom = new window.Headroom(header, {
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
        if (!headroom) {
          return;
        }
        if (frozen) {
          headroom.unfreeze();
          frozen = false;
        } else {
          headroom.freeze();
          frozen = true;
        }
      };
    }

    // Re-measure whenever the header's size changes (banner mode, wrap
    // on narrow viewports, fonts loading, collapse menus opening, …).
    if (window.ResizeObserver) {
      resizeObserver = new window.ResizeObserver(() => {
        setTimeout(updateDocumentOffsetWithoutAnimation, 0);
      });
      resizeObserver.observe(header, {
        attributes: true,
        childList: true,
        characterData: true,
      });
    }

    // Initial measurement, after layout settles (Q1's 250ms).
    setTimeout(updateDocumentOffsetWithoutAnimation, 250);
  }

  registerAnnouncement(window.document.querySelector("#quarto-announcement"));
  initForHeader(window.document.querySelector("#quarto-header"));

  // Track header (and announcement) element identity: in the hub-client
  // preview the header mounts after DOMContentLoaded and can be replaced
  // by edits. Cheap on native pages (two querySelectors per mutation
  // batch).
  const headerWatcher = new MutationObserver(() => {
    registerAnnouncement(
      window.document.querySelector("#quarto-announcement")
    );
    const header = window.document.querySelector("#quarto-header");
    if (header !== currentHeader) {
      initForHeader(header);
    }
  });
  headerWatcher.observe(window.document.body, {
    childList: true,
    subtree: true,
  });

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

  // Viewport-resize fallback re-measure (the per-header ResizeObserver
  // covers header-content changes; this covers wrap-on-resize when
  // ResizeObserver is unavailable, and viewport-height changes that
  // affect the sidebar max-height math).
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

  // Native pages load this via a plain <script src> in <head>, before
  // DOMContentLoaded. The hub-client preview injects it from a
  // dynamically-imported module that typically runs AFTER
  // DOMContentLoaded — a listener registered then would never fire, so
  // run immediately once the DOM is ready either way.
  if (window.document.readyState === "loading") {
    window.document.addEventListener("DOMContentLoaded", quartoNavMain);
  } else {
    quartoNavMain();
  }
})();
