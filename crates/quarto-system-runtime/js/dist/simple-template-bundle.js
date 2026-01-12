// Simple template bundle for interstitial testing
// This is a self-contained bundle with no dependencies.
// DO NOT EDIT - generated from js/src/simple-template.js

(function() {
    "use strict";

    /**
     * Render a simple template with ${key} placeholders.
     * @param {string} template - Template string with ${key} placeholders
     * @param {Object} data - Object with key-value pairs for substitution
     * @returns {string} Rendered string
     */
    function renderSimpleTemplate(template, data) {
        return template.replace(/\$\{(\w+)\}/g, function(match, key) {
            return key in data ? String(data[key]) : '';
        });
    }

    // Export to globalThis
    globalThis.renderSimpleTemplate = renderSimpleTemplate;
})();
