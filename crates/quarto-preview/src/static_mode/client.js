// q2 preview --static: the browser side of the reload channel.
// Injected inline into every served HTML page (see server.rs). Plain
// script, no framework: it opens an EventSource on the server's SSE
// endpoint, shows a small status badge while a render runs, shows the
// diagnostics when one fails, and reloads or navigates when outputs
// change. Must never contain the closing script tag literally, since
// it is embedded inside one.
(function () {
  "use strict";
  if (window.__q2PreviewStatic) {
    return;
  }
  window.__q2PreviewStatic = true;

  var BADGE_ID = "q2-preview-static-badge";
  var PANEL_ID = "q2-preview-static-panel";

  function byId(id) {
    return document.getElementById(id);
  }

  function ensureBadge() {
    var el = byId(BADGE_ID);
    if (el) {
      return el;
    }
    el = document.createElement("div");
    el.id = BADGE_ID;
    el.setAttribute(
      "style",
      "position:fixed;right:12px;bottom:12px;z-index:2147483646;" +
        "padding:6px 10px;border-radius:6px;font:13px/1.4 system-ui,sans-serif;" +
        "background:#1f2937;color:#f9fafb;box-shadow:0 2px 8px #00000055;display:none;"
    );
    document.body.appendChild(el);
    return el;
  }

  function showBadge(text) {
    var el = ensureBadge();
    el.textContent = text;
    el.style.display = "block";
  }

  function hideBadge() {
    var el = byId(BADGE_ID);
    if (el) {
      el.style.display = "none";
    }
  }

  function hidePanel() {
    var el = byId(PANEL_ID);
    if (el) {
      el.parentNode.removeChild(el);
    }
  }

  function showPanel(title, text) {
    hidePanel();
    var panel = document.createElement("div");
    panel.id = PANEL_ID;
    panel.setAttribute(
      "style",
      "position:fixed;left:12px;right:12px;bottom:12px;max-height:60vh;z-index:2147483647;" +
        "overflow:auto;border-radius:8px;background:#111827;color:#f9fafb;" +
        "font:13px/1.45 ui-monospace,SFMono-Regular,Menlo,monospace;" +
        "box-shadow:0 4px 16px #00000088;"
    );
    var head = document.createElement("div");
    head.setAttribute(
      "style",
      "display:flex;align-items:center;justify-content:space-between;" +
        "padding:8px 12px;background:#7f1d1d;font-family:system-ui,sans-serif;font-weight:600;"
    );
    var label = document.createElement("span");
    label.textContent = title;
    var close = document.createElement("button");
    close.textContent = "Dismiss";
    close.setAttribute(
      "style",
      "border:0;border-radius:4px;padding:4px 8px;background:#f9fafb;color:#111827;cursor:pointer;font:inherit;"
    );
    close.addEventListener("click", hidePanel);
    head.appendChild(label);
    head.appendChild(close);
    var pre = document.createElement("pre");
    pre.setAttribute("style", "margin:0;padding:12px;white-space:pre-wrap;word-break:break-word;");
    pre.textContent = text;
    panel.appendChild(head);
    panel.appendChild(pre);
    document.body.appendChild(panel);
  }

  function normalizePath(p) {
    return p.replace(/index\.html$/, "");
  }

  function parse(e) {
    try {
      return JSON.parse(e.data);
    } catch (err) {
      return {};
    }
  }

  // Kept as one literal so the served HTML names its endpoint verbatim
  // (server.rs pins it against EVENTS_PATH).
  var source = new EventSource("/__q2-preview/events");
  var wasOpen = false;

  source.addEventListener("open", function () {
    // A second "open" means the server went away and came back
    // (EventSource reconnects by itself); whatever it rendered in the
    // meantime is newer than this page.
    if (wasOpen) {
      window.location.reload();
    }
    wasOpen = true;
  });

  source.addEventListener("render-start", function () {
    showBadge("Rendering…");
  });

  source.addEventListener("render-stop", function (e) {
    var d = parse(e);
    hideBadge();
    if (d.ok) {
      hidePanel();
    } else {
      var n = d.errors || 0;
      var title = "Render failed (" + n + (n === 1 ? " error)" : " errors)");
      showPanel(title, d.text || "No diagnostics were reported.");
    }
  });

  source.addEventListener("reload", function (e) {
    var d = parse(e);
    var target = d.target;
    if (target && normalizePath(target) !== normalizePath(window.location.pathname)) {
      window.location.replace(target);
    } else {
      window.location.reload();
    }
  });
})();
