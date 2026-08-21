/**
 * Shared scroll-sync DOM helpers (bd-9kzfi). jsdom environment so a
 * real Document/querySelectorAll backs `findElementForLine`.
 */
import { describe, it, expect } from 'vitest';
import { parseDataLoc, findElementForLine, lineForClickTarget } from './scrollSyncDom';

describe('parseDataLoc', () => {
    it('parses a well-formed data-loc into 1-based fields', () => {
        expect(parseDataLoc('0:3:1-5:18')).toEqual({
            fileId: 0,
            startLine: 3,
            startCol: 1,
            endLine: 5,
            endCol: 18,
        });
    });

    it('returns null for malformed input', () => {
        expect(parseDataLoc('not-a-loc')).toBeNull();
        expect(parseDataLoc('0:3:1-5')).toBeNull();
    });
});

describe('findElementForLine', () => {
    function docWith(html: string): Document {
        const doc = document.implementation.createHTMLDocument('t');
        doc.body.innerHTML = html;
        return doc;
    }

    it('matches the element whose line range contains the line', () => {
        const doc = docWith(
            '<p data-loc="0:1:1-1:5" id="a">a</p>' +
                '<p data-loc="0:3:1-3:5" id="b">b</p>',
        );
        expect(findElementForLine(doc, 3)?.id).toBe('b');
        expect(findElementForLine(doc, 1)?.id).toBe('a');
    });

    it('prefers the most specific (smallest range) enclosing element', () => {
        const doc = docWith(
            '<div data-loc="0:1:1-10:1" id="outer">' +
                '<p data-loc="0:4:1-4:9" id="inner">x</p>' +
                '</div>',
        );
        // Line 4 is inside both; the tighter <p> wins.
        expect(findElementForLine(doc, 4)?.id).toBe('inner');
        // Line 2 is only inside the outer div.
        expect(findElementForLine(doc, 2)?.id).toBe('outer');
    });

    it('falls back to the nearest preceding block past the last range', () => {
        // Cursor past every range (e.g. a fresh blank line at the end of the
        // document). Snaps to the closest block that starts at or before the
        // line, so end-of-document edits still scroll the preview.
        const doc = docWith(
            '<p data-loc="0:1:1-1:5" id="a">a</p>' +
                '<p data-loc="0:3:1-3:5" id="b">b</p>',
        );
        expect(findElementForLine(doc, 99)?.id).toBe('b');
    });

    it('falls back to the nearest block in a gap between ranges', () => {
        // Line 5 is between the two paragraphs (covered by neither); the
        // preceding block wins over the following one.
        const doc = docWith(
            '<p data-loc="0:1:1-2:5" id="a">a</p>' +
                '<p data-loc="0:8:1-8:5" id="b">b</p>',
        );
        expect(findElementForLine(doc, 5)?.id).toBe('a');
    });

    it('falls back to the first block for a line before all ranges', () => {
        const doc = docWith(
            '<p data-loc="0:5:1-5:5" id="a">a</p>' +
                '<p data-loc="0:9:1-9:5" id="b">b</p>',
        );
        expect(findElementForLine(doc, 1)?.id).toBe('a');
    });

    it('returns null only when there are no located elements at all', () => {
        const doc = docWith('<p>no data-loc here</p>');
        expect(findElementForLine(doc, 99)).toBeNull();
    });

    it('ignores elements with an unparseable data-loc', () => {
        const doc = docWith('<p data-loc="garbage" id="g">g</p>');
        expect(findElementForLine(doc, 1)).toBeNull();
    });
});

describe('lineForClickTarget', () => {
    // bd-9kzfi follow-up (2026-08-21 plan): the click-to-editor-scroll fix.
    // U1a-U1e per the task-2 brief. Each row is bound to a specific wrong
    // implementation, not merely "not null" — see the brief for the exact
    // hazard each guards against.
    function docWith(html: string): Document {
        const doc = document.implementation.createHTMLDocument('t');
        doc.body.innerHTML = html;
        return doc;
    }

    it('U1a: resolves to the innermost [data-loc] ancestor, not an outer section', () => {
        // Reddens if closest('[data-loc]') is swapped for a document-wide
        // querySelector('[data-loc]') — that would return the section's 5
        // (first data-loc in document order) regardless of target.
        const doc = docWith(
            '<section data-loc="0:5:1-30:1">' +
                '<p data-loc="0:12:1-14:20"><em id="t">x</em></p>' +
                '</section>',
        );
        expect(lineForClickTarget(doc.getElementById('t'))).toBe(12);
    });

    it('U1b: returns null when there is no [data-loc] ancestor, even though an unrelated located element exists elsewhere in the document', () => {
        // The decoy <p> is NOT an ancestor of #s — it discriminates a
        // "nearest located block in the document" fallback (which would
        // wrongly latch onto it and return 2) from the correct
        // ancestor-only lookup (which must still return null). A fixture
        // with zero [data-loc] elements anywhere would pass under both
        // implementations and not actually test this hazard.
        const doc = docWith(
            '<p data-loc="0:2:1-2:5" id="decoy">decoy</p>' +
                '<div id="stray"><span id="s">no loc</span></div>',
        );
        expect(lineForClickTarget(doc.getElementById('s'))).toBeNull();
    });

    it('U1c: returns null inside the active-edit-region even when nested in a located section', () => {
        // Load-bearing hazard row: the edit region is nested INSIDE the
        // located <section> (not floating at the document root), so a
        // deleted active-region guard would fall through to the section's
        // closest('[data-loc]') and return 5 instead of null.
        const doc = docWith(
            '<section data-loc="0:5:1-30:1">' +
                '<div id="q2-active-edit-region"><span id="inEdit">y</span></div>' +
                '</section>',
        );
        expect(lineForClickTarget(doc.getElementById('inEdit'))).toBeNull();
    });

    it('U1d: returns null when the nearest [data-loc] ancestor is the <section> itself', () => {
        // Reddens if the <section> check is deleted — inter-block
        // whitespace directly inside the section would resolve to 5.
        const doc = docWith(
            '<section data-loc="0:5:1-30:1">' +
                '<p data-loc="0:12:1-14:20">x</p>' +
                '<span id="ws"></span>' +
                '</section>',
        );
        expect(lineForClickTarget(doc.getElementById('ws'))).toBeNull();
    });

    it('U1e: returns null and never throws for non-Element targets', () => {
        // Reddens if the narrowing guard is dropped (throws instead of
        // returning null) — see U1f below for why that guard is a
        // duck-typed `.closest` check rather than `instanceof Element`.
        expect(() => lineForClickTarget(document)).not.toThrow();
        expect(lineForClickTarget(document)).toBeNull();
        expect(() => lineForClickTarget(null)).not.toThrow();
        expect(lineForClickTarget(null)).toBeNull();
    });

    it('U1f: resolves correctly across a real cross-realm (iframe) Element boundary', () => {
        // Task 4 finding (2026-08-21 plan, ruling recorded on review): the
        // originally pinned `target instanceof Element` guard is
        // realm-specific. In production `lineForClickTarget` runs in the
        // PARENT frame's realm against a `pointerup` target that lives in
        // the sandboxed IFRAME's realm — each realm has its own `Element`
        // constructor, so `target instanceof Element` (checked against the
        // parent's `Element`) is always false for a real iframe-originated
        // node. That silently broke the whole feature end-to-end (no line
        // ever resolved, so no reveal ever fired) while U1a-U1e stayed green,
        // because those rows build same-realm fixtures via
        // `document.implementation.createHTMLDocument`. This row builds a
        // genuinely cross-realm element (a real iframe's contentDocument) so
        // a future "cleanup" that reverts the duck-typed `.closest` guard
        // back to `instanceof Element` reddens here, not only in the slower
        // Playwright suite (E2).
        //
        // Named revert: change the guard in `lineForClickTarget` back to
        // `target instanceof Element` → returns null instead of 12 → RED.
        const iframe = document.createElement('iframe');
        document.body.appendChild(iframe);
        try {
            const frameDoc = iframe.contentDocument!;
            frameDoc.open();
            frameDoc.write('<p data-loc="0:12:1-14:20"><em id="t">x</em></p>');
            frameDoc.close();

            const target = frameDoc.getElementById('t');
            expect(target).not.toBeNull();
            // Sanity check that the fixture is genuinely cross-realm: the
            // iframe's element is not an instance of the parent's Element.
            expect(target instanceof Element).toBe(false);

            expect(lineForClickTarget(target)).toBe(12);
        } finally {
            document.body.removeChild(iframe);
        }
    });
});
