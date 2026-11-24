/*
 * code-annotations.js
 * Copyright (c) 2026 Posit, PBC
 *
 * Runtime for code annotations. The markup comes from
 * `crates/quarto-core/src/transforms/code_annotations.rs` and the HTML
 * writer:
 *
 *   <a|button class="code-annotation-anchor" data-target-cell data-target-annotation>
 *   <span id="<cell>-<line>" class="code-annotation-target">        one per code line
 *   <dl class="code-annotation-container-grid" [data-code-annotations="hover|select"]>
 *     <dt data-target-cell data-target-annotation>
 *     <dd><span data-code-cell data-code-lines data-code-annotation>
 *
 * The line-highlight band is a port of the script in Quarto 1's
 * `quarto-html-after-body.ejs`. Quarto 1 shows hover/select annotations
 * with tippy.js; this uses Bootstrap's Popover, which the page already
 * loads. Where Bootstrap is missing (reveal decks) the hidden list is
 * shown under the code instead, and a click on a marker still highlights
 * its lines.
 */
(function () {
  "use strict";

  var selectedEl;

  function annotationSpan(cell, annotation) {
    return document.querySelector(
      'span[data-code-cell="' + cell + '"][data-code-annotation="' + annotation + '"]'
    );
  }

  // Each band is positioned against its own containing block, which is not
  // the offsetParent of the lines, so place it from client rects.
  function placeBand(band, firstEl, lastEl) {
    var container = band.offsetParent;
    if (container === null) return;
    var containerRect = container.getBoundingClientRect();
    // reveal.js scales slides with a transform: convert client pixels back
    // to the CSS pixels of the container.
    var scale = containerRect.height / container.offsetHeight || 1;
    var firstRect = firstEl.getBoundingClientRect();
    var lastRect = lastEl.getBoundingClientRect();
    var top = (firstRect.top - containerRect.top) / scale - container.clientTop + container.scrollTop;
    var height = (lastRect.bottom - firstRect.top) / scale;
    band.style.top = top - 2 + "px";
    band.style.height = height + 4 + "px";
  }

  function unselectCodeLines() {
    ["code-annotation-line-highlight", "code-annotation-line-highlight-gutter"].forEach(function (id) {
      var el = document.getElementById(id);
      if (el) el.remove();
    });
    selectedEl = undefined;
  }

  function selectCodeLines(el) {
    var cell = el.getAttribute("data-target-cell");
    var span = annotationSpan(cell, el.getAttribute("data-target-annotation"));
    if (!span) return;
    var lines = span.getAttribute("data-code-lines").split(",");
    var first = document.getElementById(cell + "-" + lines[0]);
    var last = document.getElementById(cell + "-" + lines[lines.length - 1]);
    if (!first || !last) return;
    var parent = first.parentElement.parentElement;

    var band = document.getElementById("code-annotation-line-highlight");
    if (band === null) {
      band = document.createElement("div");
      band.setAttribute("id", "code-annotation-line-highlight");
      band.style.position = "absolute";
      // Anchor to the block's left edge; the stylesheet's negative
      // margin then extends the band into the page margin.
      band.style.left = "0";
      parent.appendChild(band);
    }
    // Width and horizontal offset come from the stylesheet
    // (`#code-annotation-line-highlight`), as in Quarto 1.9.
    placeBand(band, first, last);

    var gutterBand = document.getElementById("code-annotation-line-highlight-gutter");
    if (gutterBand === null) {
      gutterBand = document.createElement("div");
      gutterBand.setAttribute("id", "code-annotation-line-highlight-gutter");
      gutterBand.style.position = "absolute";
      var gutter = document.getElementById(cell).querySelector(".code-annotation-gutter");
      if (gutter) gutter.appendChild(gutterBand);
    }
    if (gutterBand.offsetParent !== null) placeBand(gutterBand, first, last);

    selectedEl = el;
  }

  function clearActive(selector) {
    document.querySelectorAll(selector + ".code-annotation-active").forEach(function (el) {
      el.classList.remove("code-annotation-active");
    });
  }

  // Click a marker to highlight its lines; click it again to clear.
  function toggleOnClick(el, selector) {
    el.addEventListener("click", function (event) {
      event.preventDefault();
      var wasSelected = el === selectedEl;
      unselectCodeLines();
      clearActive(selector);
      if (!wasSelected) {
        selectCodeLines(el);
        el.classList.add("code-annotation-active");
      }
    });
  }

  function modeFor(anchor) {
    var span = annotationSpan(
      anchor.getAttribute("data-target-cell"),
      anchor.getAttribute("data-target-annotation")
    );
    var dl = span && span.closest("dl");
    return { span: span, dl: dl, mode: dl ? dl.getAttribute("data-code-annotations") : null };
  }

  function initPopover(anchor, info) {
    var click = info.mode === "select";
    var popover = new window.bootstrap.Popover(anchor, {
      html: true,
      content: function () {
        var tip = info.span.cloneNode(true);
        tip.classList.add("code-annotation-tip-content");
        return tip;
      },
      placement: "right",
      fallbackPlacements: ["right", "top", "bottom", "left"],
      trigger: click ? "click" : "hover focus",
      container: "body",
      customClass: "code-annotation-popover",
    });
    anchor.addEventListener("show.bs.popover", function () {
      // One annotation open at a time.
      document.querySelectorAll("button.code-annotation-anchor.code-annotation-active").forEach(function (other) {
        if (other !== anchor) {
          var p = window.bootstrap.Popover.getInstance(other);
          if (p) p.hide();
        }
      });
      selectCodeLines(anchor);
      anchor.classList.add("code-annotation-active");
    });
    anchor.addEventListener("hide.bs.popover", function () {
      if (selectedEl === anchor) unselectCodeLines();
      anchor.classList.remove("code-annotation-active");
    });
    document.addEventListener("keydown", function (event) {
      if (event.key === "Escape") popover.hide();
    });
    if (click) {
      document.addEventListener("click", function (event) {
        var tip = document.querySelector(".code-annotation-popover");
        if (event.target !== anchor && !(tip && tip.contains(event.target))) popover.hide();
      });
    }
  }

  function init() {
    // `below`: the list is visible; its numbers are the click targets.
    document.querySelectorAll("dl.code-annotation-container-grid:not([data-code-annotations]) dt[data-target-cell]").forEach(
      function (dt) {
        toggleOnClick(dt, "dt[data-target-cell]");
      }
    );

    // `hover` / `select`: the markers in the code are buttons.
    document.querySelectorAll("button.code-annotation-anchor").forEach(function (anchor) {
      var info = modeFor(anchor);
      if (!info.span) return;
      if (window.bootstrap && window.bootstrap.Popover) {
        initPopover(anchor, info);
      } else {
        // No popover library: show the list and highlight on click.
        if (info.dl) info.dl.classList.remove("code-annotation-container-hidden");
        toggleOnClick(anchor, "button.code-annotation-anchor");
      }
    });

    var pending;
    window.addEventListener("resize", function () {
      clearTimeout(pending);
      pending = setTimeout(function () {
        if (selectedEl) selectCodeLines(selectedEl);
      }, 10);
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
