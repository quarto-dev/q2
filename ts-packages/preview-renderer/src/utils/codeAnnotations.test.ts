// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import { installCodeAnnotations } from './codeAnnotations';

function fixture(): HTMLElement {
    document.body.innerHTML = `
      <div id="root">
        <div class="sourceCode" id="cell-1"><pre><code>
          <a class="code-annotation-anchor" data-target-cell="cell-1" data-target-annotation="1" href="">1</a><span id="cell-1-1">a</span>
          <span id="cell-1-2">b</span>
          <div class="code-annotation-gutter"></div>
        </code></pre></div>
        <dl>
          <dt data-target-cell="cell-1" data-target-annotation="1">1</dt>
          <dd><span data-code-cell="cell-1" data-code-lines="1,2" data-code-annotation="1">First.</span></dd>
          <dt data-target-cell="cell-1" data-target-annotation="9">9</dt>
        </dl>
      </div>`;
    return document.getElementById('root')!;
}

// jsdom reports no layout, so `offsetParent` is null and the band keeps no
// geometry; these tests cover which elements exist and which is active.
describe('installCodeAnnotations', () => {
    let root: HTMLElement;
    beforeEach(() => {
        root = fixture();
    });

    it('draws one band on click and clears it on a second click', () => {
        const cleanup = installCodeAnnotations(root);
        const dt = root.querySelector('dt')!;
        dt.click();
        expect(document.querySelectorAll('#code-annotation-line-highlight')).toHaveLength(1);
        expect(root.querySelector('pre > #code-annotation-line-highlight')).not.toBeNull();
        expect(root.querySelector('.code-annotation-gutter > #code-annotation-line-highlight-gutter')).not.toBeNull();
        expect(dt.classList.contains('code-annotation-active')).toBe(true);

        dt.click();
        expect(document.getElementById('code-annotation-line-highlight')).toBeNull();
        expect(dt.classList.contains('code-annotation-active')).toBe(false);
        cleanup();
    });

    it('moves the band when another trigger is clicked', () => {
        const cleanup = installCodeAnnotations(root);
        root.querySelector('dt')!.click();
        (root.querySelector('a.code-annotation-anchor') as HTMLElement).click();
        expect(document.querySelectorAll('#code-annotation-line-highlight')).toHaveLength(1);
        expect(root.querySelectorAll('.code-annotation-active')).toHaveLength(1);
        expect(root.querySelector('a.code-annotation-anchor')!.classList.contains('code-annotation-active')).toBe(true);
        cleanup();
    });

    it('does nothing for a number with no matching annotation', () => {
        const cleanup = installCodeAnnotations(root);
        const orphan = root.querySelectorAll('dt')[1] as HTMLElement;
        orphan.click();
        expect(document.getElementById('code-annotation-line-highlight')).toBeNull();
        expect(orphan.classList.contains('code-annotation-active')).toBe(false);
        cleanup();
    });

    it('removes the band and stops listening on cleanup', () => {
        const cleanup = installCodeAnnotations(root);
        const dt = root.querySelector('dt')!;
        dt.click();
        cleanup();
        expect(document.getElementById('code-annotation-line-highlight')).toBeNull();
        dt.click();
        expect(document.getElementById('code-annotation-line-highlight')).toBeNull();
    });
});
