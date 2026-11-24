/**
 * Iframe-safe code-annotation handler for q2-preview / hub-client.
 *
 * In `q2 render` the page loads `resources/js/code-annotations/
 * code-annotations.js`, which binds a click listener to each annotation
 * number. That script is not shipped to the preview: the preview re-renders
 * the AST on every edit, which replaces the numbers and would orphan a
 * listener bound to a node. Like `installCodeCopy`, this installs ONE
 * delegated listener on a stable root and matches by selector.
 *
 * Clicking a number in the annotation list (`dt[data-target-cell]`) or a
 * marker in the code (`.code-annotation-anchor`) draws the highlight band
 * over that annotation's lines; clicking it again clears the band. The band
 * elements and their ids match the native script, so the same CSS
 * (`#code-annotation-line-highlight`, `-gutter`) styles them.
 *
 * The preview renders `hover` and `select` as `below` (see
 * `crates/quarto-core/src/transforms/code_annotations.rs`), so there are no
 * popovers to manage here.
 */

const TRIGGER_SELECTOR = 'dt[data-target-cell], .code-annotation-anchor';
const ACTIVE_CLASS = 'code-annotation-active';
const BAND_ID = 'code-annotation-line-highlight';
const GUTTER_BAND_ID = 'code-annotation-line-highlight-gutter';

/** Position `band` over the lines from `first` to `last`, against the band's
 *  own containing block. Port of `placeBand` in the native script. */
function placeBand(band: HTMLElement, first: Element, last: Element): void {
    const container = band.offsetParent as HTMLElement | null;
    if (container === null) return;
    const containerRect = container.getBoundingClientRect();
    // reveal.js scales slides with a transform: convert client pixels back
    // to the CSS pixels of the container.
    const scale = containerRect.height / container.offsetHeight || 1;
    const firstRect = first.getBoundingClientRect();
    const lastRect = last.getBoundingClientRect();
    const top = (firstRect.top - containerRect.top) / scale - container.clientTop + container.scrollTop;
    const height = (lastRect.bottom - firstRect.top) / scale;
    band.style.top = `${top - 2}px`;
    band.style.height = `${height + 4}px`;
}

function clearBands(doc: Document): void {
    doc.getElementById(BAND_ID)?.remove();
    doc.getElementById(GUTTER_BAND_ID)?.remove();
}

/** Draw the band for `trigger`'s annotation. Returns false when its lines
 *  cannot be found (the block was edited away, say). */
function selectLines(root: HTMLElement, trigger: Element): boolean {
    const doc = root.ownerDocument;
    const cell = trigger.getAttribute('data-target-cell');
    const annotation = trigger.getAttribute('data-target-annotation');
    if (!cell || !annotation) return false;
    // Compare attribute values rather than building a selector from them:
    // a cell id is user-controlled text.
    const span = Array.from(root.querySelectorAll('span[data-code-cell]')).find(
        (el) =>
            el.getAttribute('data-code-cell') === cell &&
            el.getAttribute('data-code-annotation') === annotation,
    );
    const lines = span?.getAttribute('data-code-lines')?.split(',') ?? [];
    if (lines.length === 0) return false;
    const first = doc.getElementById(`${cell}-${lines[0]}`);
    const last = doc.getElementById(`${cell}-${lines[lines.length - 1]}`);
    const pre = first?.parentElement?.parentElement;
    if (!first || !last || !pre) return false;

    const band = doc.createElement('div');
    band.id = BAND_ID;
    band.style.position = 'absolute';
    band.style.left = '0';
    pre.appendChild(band);
    placeBand(band, first, last);

    const gutter = doc.getElementById(cell)?.querySelector('.code-annotation-gutter');
    if (gutter) {
        const gutterBand = doc.createElement('div');
        gutterBand.id = GUTTER_BAND_ID;
        gutterBand.style.position = 'absolute';
        gutter.appendChild(gutterBand);
        placeBand(gutterBand, first, last);
    }
    return true;
}

/**
 * Install the delegated annotation handler on `root`. Returns a cleanup
 * function that removes every listener and any band still showing.
 */
export function installCodeAnnotations(root: HTMLElement): () => void {
    // Keep a click on a number from also opening the block editor, which
    // PreviewRoot activates from pointer events (same reasoning as
    // `installCodeCopy`).
    const onPointer = (ev: Event) => {
        const target = ev.target as Element | null;
        if (target?.closest?.(TRIGGER_SELECTOR)) ev.stopPropagation();
    };

    const onClick = (ev: Event) => {
        const trigger = (ev.target as Element | null)?.closest?.(TRIGGER_SELECTOR);
        if (!trigger || !root.contains(trigger)) return;
        ev.preventDefault();
        ev.stopPropagation();
        const wasActive = trigger.classList.contains(ACTIVE_CLASS);
        root.querySelectorAll(`.${ACTIVE_CLASS}`).forEach((el) => el.classList.remove(ACTIVE_CLASS));
        clearBands(root.ownerDocument);
        if (!wasActive && selectLines(root, trigger)) {
            trigger.classList.add(ACTIVE_CLASS);
        }
    };

    root.addEventListener('pointerdown', onPointer, true);
    root.addEventListener('pointerup', onPointer, true);
    root.addEventListener('click', onClick, true);
    return () => {
        root.removeEventListener('pointerdown', onPointer, true);
        root.removeEventListener('pointerup', onPointer, true);
        root.removeEventListener('click', onClick, true);
        clearBands(root.ownerDocument);
    };
}
