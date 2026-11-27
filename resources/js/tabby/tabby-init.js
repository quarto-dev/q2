// Tabby init for reveal.js decks (bd-y5j0m776).
//
// Ported from two pieces of Quarto 1: the `if (tabby) { … }` block of
// `quarto-html-after-body.ejs`, which starts one Tabby instance per tabset,
// and `handleTabbyClicks` in the reveal support plugin
// (`formats/revealjs/plugins/support/support.js`), which keeps a click on a
// tab from reaching reveal.js. A tab is an `<a href="#tabset-1-2">`, so a
// click that got through would be read as slide navigation.
//
// Shipped as the `js:revealjs:tabby-init` artifact by
// `PanelTabsetResolveTransform`, only for a deck that has a tabset. It loads
// after `tabby.js` (the keys sort that way), which defines `window.Tabby`.
//
// Not ported: Quarto 1 also re-fires its `slideenter` event when a tab
// changes, so htmlwidgets and Shiny outputs in the new pane resize. Quarto 2
// has no such event yet. Tabby still dispatches its own `tabby` event on
// every change, for anything that wants to listen.
(function () {
  function init() {
    if (typeof window.Tabby !== "function") {
      return;
    }
    var tabsets = window.document.querySelectorAll(".panel-tabset-tabby");
    for (var i = 0; i < tabsets.length; i++) {
      if (tabsets[i].id) {
        new window.Tabby("#" + tabsets[i].id);
      }
    }
    // Tabby toggles the tab from a capturing listener on the document, which
    // has run by the time the click reaches the link itself.
    var tabs = window.document.querySelectorAll(".panel-tabset-tabby > li > a");
    for (var j = 0; j < tabs.length; j++) {
      tabs[j].onclick = function (ev) {
        ev.preventDefault();
        ev.stopPropagation();
        return false;
      };
    }
  }

  if (window.document.readyState === "loading") {
    window.document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
