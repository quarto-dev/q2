// Simple ${key} template replacement for interstitial testing
// This is NOT JavaScript template literals - it's a simple regex replacement
// that validates the Rust <-> JS data flow without requiring EJS.

/**
 * Render a simple template with ${key} placeholders.
 * @param {string} template - Template string with ${key} placeholders
 * @param {Object} data - Object with key-value pairs for substitution
 * @returns {string} Rendered string
 */
function renderSimpleTemplate(template, data) {
    return template.replace(/\$\{(\w+)\}/g, (match, key) => {
        return key in data ? String(data[key]) : '';
    });
}

// Export for use as global
globalThis.renderSimpleTemplate = renderSimpleTemplate;
