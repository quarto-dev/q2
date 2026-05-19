// Auto-injected by AttributionViewerTransform when --attribution=git
// (or YAML attribution: git) is active.
//
// Colour paint is render-time CSS: `viewer.css` paints
// `[data-attr-actor]` via `var(--attr-color)`, and one per-actor rule
// per render publishes that variable plus `--attr-name`. This script
// only handles the interactive part — the floating badge that
// appears on hover. Identity comes from the wrapper's computed style
// (`--attr-color` / `--attr-name`); the timestamp stays per-node as
// `data-attr-time`.

(function () {
    function formatRelativeTime(timestamp) {
        var now = Date.now();
        // git blame emits seconds, Automerge emits milliseconds;
        // the 1e12 threshold distinguishes them.
        var tsMs = timestamp < 1e12 ? timestamp * 1000 : timestamp;
        var diffSec = Math.floor((now - tsMs) / 1000);
        if (diffSec < 60) return 'just now';
        var diffMin = Math.floor(diffSec / 60);
        if (diffMin < 60) return diffMin + 'm ago';
        var diffHr = Math.floor(diffMin / 60);
        if (diffHr < 24) return diffHr + 'h ago';
        var diffDay = Math.floor(diffHr / 24);
        return diffDay + 'd ago';
    }

    // CSS string custom properties round-trip with their wrapping
    // quotes (e.g. `--attr-name: "Charlie"` returns `"Charlie"`).
    // Strip them and undo the two escapes the CSS emitter applies
    // (`\\` and `\"`). Any other content survives unchanged.
    function readCssString(cs, name) {
        var raw = cs.getPropertyValue(name).trim();
        if (raw.length >= 2 && raw.charAt(0) === '"' && raw.charAt(raw.length - 1) === '"') {
            raw = raw.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, '\\');
        }
        return raw;
    }

    function buildBadge(leaf) {
        var cs = window.getComputedStyle(leaf);
        var color = cs.getPropertyValue('--attr-color').trim();
        var name = readCssString(cs, '--attr-name');
        var time = Number(leaf.getAttribute('data-attr-time'));
        if (!name || !color || !Number.isFinite(time)) return null;

        var badge = document.createElement('span');
        badge.className = 'q2-attr-badge';
        badge.style.setProperty('--attr-color', color);

        var dot = document.createElement('span');
        dot.className = 'q2-attr-badge-dot';
        dot.style.backgroundColor = color;
        badge.appendChild(dot);

        badge.appendChild(document.createTextNode(name + ' '));

        var timeEl = document.createElement('span');
        timeEl.className = 'q2-attr-badge-time';
        timeEl.textContent = formatRelativeTime(time);
        badge.appendChild(timeEl);

        return badge;
    }

    var currentBadge = null;

    document.addEventListener('mouseover', function (e) {
        var leaf = e.target.closest('[data-attr-actor]');
        if (!leaf) return;
        if (currentBadge) currentBadge.remove();

        var badge = buildBadge(leaf);
        if (!badge) return;

        var rect = leaf.getBoundingClientRect();
        badge.style.position = 'fixed';
        badge.style.top = (rect.bottom + 2) + 'px';
        badge.style.left = rect.left + 'px';

        document.body.appendChild(badge);
        currentBadge = badge;
    });

    document.addEventListener('mouseout', function (e) {
        var related = e.relatedTarget;
        if (related && related.closest && related.closest('[data-attr-actor]')) {
            return;
        }
        if (currentBadge) {
            currentBadge.remove();
            currentBadge = null;
        }
    });
})();
