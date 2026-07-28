// Code-copy init handler — ported from Q1's `quarto-html-after-body.ejs`
// (`if (copyCode) { … }` block). Initializes ClipboardJS on every
// `.code-copy-button` and wires the "Copied!" Bootstrap Tooltip + icon
// flash on success.
//
// Shipped as the `js:code-copy-init` artifact by the native pipeline's
// `ClipboardJsStage`. Depends on `window.ClipboardJS` (the vendored
// `clipboard.min.js`) and, for the success tooltip, on `window.bootstrap`
// (loaded by `bootstrap.bundle.min.js`). Both are emitted in sort-key
// order before this script, so both globals are present by the time it
// runs.
//
// Compared to Q1, two simplifications:
// - No `data-in-quarto-modal` codepath: Q2 doesn't ship the embedded
//   source-code modal feature yet.
// - English-only tooltip strings: Q2 doesn't have a language-table
//   system. When one lands, both strings should read from it.
(function () {
  function init() {
    if (typeof window.ClipboardJS !== "function") {
      return;
    }

    var isCodeAnnotation = function (el) {
      for (var i = 0; i < el.classList.length; i++) {
        if (el.classList[i].indexOf("code-annotation-") === 0) {
          return true;
        }
      }
      return false;
    };

    var getTextToCopy = function (trigger) {
      // Clone so removing annotation children doesn't mutate the
      // rendered DOM. The clone is discarded after we read innerText.
      var outerScaffold = trigger.parentElement.cloneNode(true);
      var codeEl = outerScaffold.querySelector("code");
      if (!codeEl) {
        return "";
      }
      var children = Array.prototype.slice.call(codeEl.children);
      for (var i = 0; i < children.length; i++) {
        if (isCodeAnnotation(children[i])) {
          children[i].remove();
        }
      }
      return codeEl.innerText;
    };

    var onCopySuccess = function (e) {
      var button = e.trigger;
      // Don't keep focus so the keyboard ring doesn't sit on a button
      // whose state is about to revert.
      button.blur();
      button.classList.add("code-copy-button-checked");
      var currentTitle = button.getAttribute("title");
      button.setAttribute("title", "Copied!");

      var tooltip = null;
      if (window.bootstrap && window.bootstrap.Tooltip) {
        button.setAttribute("data-bs-toggle", "tooltip");
        button.setAttribute("data-bs-placement", "left");
        button.setAttribute("data-bs-title", "Copied!");
        tooltip = new window.bootstrap.Tooltip(button, {
          trigger: "manual",
          customClass: "code-copy-button-tooltip",
          offset: [0, -8],
        });
        tooltip.show();
      }

      window.setTimeout(function () {
        if (tooltip) {
          tooltip.hide();
          button.removeAttribute("data-bs-title");
          button.removeAttribute("data-bs-toggle");
          button.removeAttribute("data-bs-placement");
        }
        button.setAttribute("title", currentTitle);
        button.classList.remove("code-copy-button-checked");
      }, 1000);

      e.clearSelection();
    };

    var clipboard = new window.ClipboardJS(".code-copy-button", {
      text: getTextToCopy,
    });
    clipboard.on("success", onCopySuccess);
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
