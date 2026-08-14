// Quarto color-mode runtime (light/dark theme toggle).
//
// Ported from Quarto 1's quarto-html-before-body.ejs (de-EJS'd:
// configuration arrives via data attributes on this script's own tag
// instead of template interpolation). Injected INLINE as the first
// child of <body> so it runs synchronously before first paint — the
// initial variant selection must happen before any content renders
// (FOUC avoidance is the hard constraint; see
// claude-notes/plans/2026-08-14-light-dark-theme-epic.md D5).
//
// Mechanism: all theme stylesheets are emitted rel="stylesheet" in
// FOUC-safe order (light, dark, and for author-default-light a
// trailing light copy). This script flips link.rel between
// "stylesheet" and "disabled-stylesheet" and keeps body.quarto-light /
// body.quarto-dark plus the root color-scheme in sync with the active
// sheet's data-mode. "alternate" refers to the .quarto-color-alternate
// (dark) sheets; the persisted sentinel value "alternate"/"default" in
// localStorage["quarto-color-scheme"] matches Quarto 1's key and
// values, so preferences carry over between Q1 and Q2 sites on the
// same origin.
//
// Deliberate divergences from Q1 (documented in the plan):
// - No Safari scrollbar-recolor hack: the compiled CSS declares
//   :root{color-scheme:...} per variant (D1a), which is the standard
//   fix the hack approximated.
// - No giscus handling (Q2 has no comments support yet).
// - The floating top-right fallback toggle (Q1's after-body script)
//   is folded into this file's DOMContentLoaded handler.
(function () {
  const script = document.currentScript;
  const authorPrefersDark = script.dataset.authorPrefersDark === "true";
  const respectUserColorScheme =
    script.dataset.respectUserColorScheme === "true";

  const isFileUrl = () => window.location.protocol === "file:";

  const toggleBodyColorMode = (bsSheetEl) => {
    const mode = bsSheetEl.getAttribute("data-mode");
    const bodyEl = window.document.querySelector("body");
    if (mode === "dark") {
      bodyEl.classList.add("quarto-dark");
      bodyEl.classList.remove("quarto-light");
    } else {
      bodyEl.classList.add("quarto-light");
      bodyEl.classList.remove("quarto-dark");
    }
    // Belt-and-braces for UA-drawn chrome: the enabled stylesheet
    // declares the same value, but setting it on the root applies it
    // even in the instant before that sheet's rules take effect.
    document.documentElement.style.colorScheme =
      mode === "dark" ? "dark" : "light";
  };

  const toggleBodyColorPrimary = () => {
    const bsSheetEl = window.document.querySelector(
      "link#quarto-bootstrap:not([rel=disabled-stylesheet])"
    );
    if (bsSheetEl) {
      toggleBodyColorMode(bsSheetEl);
    }
  };

  const disableStylesheet = (stylesheets) => {
    for (let i = 0; i < stylesheets.length; i++) {
      stylesheets[i].rel = "disabled-stylesheet";
    }
  };

  const enableStylesheet = (stylesheets) => {
    for (let i = 0; i < stylesheets.length; i++) {
      // Guard against re-setting rel to its current value — some
      // browsers re-fetch/re-apply on assignment, causing a flash.
      if (stylesheets[i].rel !== "stylesheet") {
        stylesheets[i].rel = "stylesheet";
      }
    }
  };

  // Suppress CSS transitions on elements that would otherwise animate
  // during the swap (margin-sidebar links animate color).
  const manageTransitions = (selector, allowTransitions) => {
    const els = window.document.querySelectorAll(selector);
    for (let i = 0; i < els.length; i++) {
      els[i].style.transition = allowTransitions ? null : "none";
    }
  };

  const setColorSchemeToggle = (alternate) => {
    const toggles = window.document.querySelectorAll(
      ".quarto-color-scheme-toggle"
    );
    for (let i = 0; i < toggles.length; i++) {
      if (alternate) {
        toggles[i].classList.add("alternate");
      } else {
        toggles[i].classList.remove("alternate");
      }
    }
  };

  const toggleColorMode = (alternate) => {
    // The trailing default copies (.quarto-color-scheme-extra) are
    // deliberately NOT matched by either selector: once this runtime
    // owns the swap they stay disabled.
    const primaryStylesheets = document.querySelectorAll(
      "link.quarto-color-scheme:not(.quarto-color-alternate)"
    );
    const alternateStylesheets = document.querySelectorAll(
      "link.quarto-color-scheme.quarto-color-alternate"
    );
    manageTransitions("#quarto-margin-sidebar .nav-link", false);
    if (alternate) {
      // Note: dark is layered on top of light — the primary sheets are
      // not disabled, the dark CSS only needs to override.
      enableStylesheet(alternateStylesheets);
      for (const sheetNode of alternateStylesheets) {
        if (sheetNode.id === "quarto-bootstrap") {
          toggleBodyColorMode(sheetNode);
        }
      }
    } else {
      disableStylesheet(alternateStylesheets);
      enableStylesheet(primaryStylesheets);
      toggleBodyColorPrimary();
    }
    manageTransitions("#quarto-margin-sidebar .nav-link", true);
    setColorSchemeToggle(alternate);
  };

  // file:// URLs have no reliable localStorage — fall back to a
  // page-lifetime variable (Q1 behavior).
  let localAlternateSentinel;

  const setStyleSentinel = (alternate) => {
    const value = alternate ? "alternate" : "default";
    if (!isFileUrl()) {
      window.localStorage.setItem("quarto-color-scheme", value);
    } else {
      localAlternateSentinel = value;
    }
  };

  const getColorSchemeSentinel = () => {
    if (!isFileUrl()) {
      const storageValue = window.localStorage.getItem("quarto-color-scheme");
      return storageValue != null ? storageValue : localAlternateSentinel;
    }
    return localAlternateSentinel;
  };

  const hasAlternateSentinel = () => getColorSchemeSentinel() === "alternate";

  // The effective initial darkness: the author's choice, overridden by
  // the OS preference when respect-user-color-scheme is on.
  let darkModeDefault = authorPrefersDark;
  let queryPrefersDark = null;
  if (respectUserColorScheme && window.matchMedia) {
    queryPrefersDark = window.matchMedia("(prefers-color-scheme: dark)");
    darkModeDefault = queryPrefersDark.matches;
  }

  // Author default light → the trailing default copies exist purely
  // for pre-JS paint; hand control to the swapper.
  if (!authorPrefersDark) {
    disableStylesheet(
      document.querySelectorAll("link.quarto-color-scheme-extra")
    );
  }

  // "alternate" means the dark (.quarto-color-alternate) sheets are
  // active. No stored preference → start from the effective default.
  localAlternateSentinel = darkModeDefault ? "alternate" : "default";

  window.quartoToggleColorScheme = () => {
    const toAlternate = !hasAlternateSentinel();
    toggleColorMode(toAlternate);
    setStyleSentinel(toAlternate);
    // Nudge anything that re-layouts on theme change (plots, OJS).
    window.dispatchEvent(new Event("resize"));
  };

  if (queryPrefersDark) {
    queryPrefersDark.addEventListener("change", (e) => {
      // An explicit user choice (persisted sentinel) always wins over
      // the OS preference.
      if (
        !isFileUrl() &&
        window.localStorage.getItem("quarto-color-scheme") !== null
      ) {
        return;
      }
      toggleColorMode(e.matches);
      localAlternateSentinel = e.matches ? "alternate" : "default";
    });
  }

  // Apply the initial state synchronously, before first paint.
  toggleColorMode(hasAlternateSentinel());

  // Documents without a navbar/sidebar toggle get a floating one
  // (Q1's after-body fallback).
  window.document.addEventListener("DOMContentLoaded", () => {
    let toggle = window.document.querySelector(".quarto-color-scheme-toggle");
    if (!toggle) {
      toggle = window.document.createElement("a");
      toggle.href = "";
      toggle.setAttribute(
        "onclick",
        "window.quartoToggleColorScheme(); return false;"
      );
      toggle.className = "top-right quarto-color-scheme-toggle";
      toggle.title = "Toggle dark mode";
      const icon = window.document.createElement("i");
      icon.className = "bi";
      toggle.appendChild(icon);
      window.document.body.appendChild(toggle);
    }
    setColorSchemeToggle(hasAlternateSentinel());
  });
})();
