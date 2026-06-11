/**
 * WASM end-to-end tests for the apply_node_edit write-back path
 * (target-incremental-writes Phase 6).
 *
 * Three integration gaps that unit tests cannot catch — each has a
 * dedicated regression test:
 *
 *  1. Pool lives at `astContext.p` (not at the document root).
 *  2. Multi-file project path (`render_page_in_project_with_attribution`)
 *     must return `untransformed_ast_json` — previously hardcoded to None.
 *  3. `apply_node_edit` must accept the compact pool-entry format
 *     {"t":0,"r":[s,e],"d":file_id} that the frontend actually sends.
 *
 * All tests use `render_page_in_project_with_attribution` because that
 * is the function the hub-client calls for q2-preview documents.
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { initWasm, vfsAddFile, vfsClear } from '@quarto/preview-runtime';
import { sliceBytes } from '@quarto/preview-renderer/utils/sliceSource';

const __dirname = dirname(fileURLToPath(import.meta.url));

interface RenderResponse {
    success: boolean;
    error?: string;
    ast_json?: string;
    untransformed_ast_json?: string;
}
interface AstResponse {
    success: boolean;
    error?: string;
    ast?: string;
    qmd?: string;
}

// Minimal q2-preview document for testing.
const CONTENT = '---\nformat: q2-preview\n---\n\nHello world.\n';
const PATH = '/project/doc.qmd';
const PROJECT_YML = 'project:\n  type: default\n';

let wasm: any;

beforeAll(async () => {
    const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
    const wasmBytes = await readFile(join(wasmDir, 'wasm_quarto_hub_client_bg.wasm'));
    wasm = await import('wasm-quarto-hub-client');
    await wasm.default(wasmBytes);
    await initWasm();
});

beforeEach(() => {
    vfsClear();
    vfsAddFile('/project/_quarto.yml', PROJECT_YML);
    vfsAddFile(PATH, CONTENT);
});

// ---------------------------------------------------------------------------
// Regression 1: pool must be at astContext.p, not at the document root.
// ---------------------------------------------------------------------------

describe('pool location', () => {
    it('pool is at astContext.p, not at raw.p', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        // Root-level `p` must NOT exist — pool lives inside `astContext`.
        expect(a_u.p, 'pool must not be at document root').toBeUndefined();
        // `astContext.p` must be a non-empty array.
        expect(Array.isArray(a_u.astContext?.p), 'astContext.p must be an array').toBe(true);
        expect((a_u.astContext.p as unknown[]).length).toBeGreaterThan(0);
    });

    it('pool entries use compact format {"t","r","d"}', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const entry = pool[0];
        expect(typeof entry.t).toBe('number');
        expect(Array.isArray(entry.r)).toBe(true);
        expect(entry.r).toHaveLength(2);
        expect(typeof entry.d).not.toBe('undefined');
    });
});

// ---------------------------------------------------------------------------
// Regression 2: multi-file project path must return untransformed_ast_json.
// ---------------------------------------------------------------------------

describe('project path returns untransformed_ast_json', () => {
    it('render_page_in_project_with_attribution populates untransformed_ast_json', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);
        expect(result.ast_json, 'ast_json must be present').toBeTruthy();
        expect(
            result.untransformed_ast_json,
            'untransformed_ast_json must be present for project path',
        ).toBeTruthy();
    });

    it('untransformed_ast_json is a valid Pandoc document with blocks', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        expect(Array.isArray(a_u.blocks)).toBe(true);
        expect(a_u.blocks.length).toBeGreaterThan(0);
        // The paragraph block must have a pool reference.
        const para = a_u.blocks.find((b: any) => b.t === 'Para');
        expect(para, 'untransformed AST must contain a Para block').toBeTruthy();
        expect(typeof para.s).toBe('number');
    });
});

// ---------------------------------------------------------------------------
// Regression 3: apply_node_edit must accept the compact pool-entry format.
// ---------------------------------------------------------------------------

describe('apply_node_edit with compact source_info format', () => {
    it('accepts the compact {"t","r","d"} pool-entry format the frontend sends', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const para = a_u.blocks.find((b: any) => b.t === 'Para');
        // The compact pool entry — exactly what the iframe sends.
        const compactEntry = pool[para.s];
        const destSiJson = JSON.stringify(compactEntry);

        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('Replaced text.\n'));
        expect(subtree.success).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(CONTENT, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);
        expect(editResult.qmd).toContain('Replaced text.');
    });
});

// ---------------------------------------------------------------------------
// Full round-trip: render → edit block 0 → verify QMD precision.
// ---------------------------------------------------------------------------

describe('full edit round-trip', () => {
    it('replaces only the target paragraph; rest of QMD is verbatim', async () => {
        const content = '---\nformat: q2-preview\n---\n\nFirst para.\n\nSecond para.\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        // Find first Para block (block 0 in the untransformed AST).
        const para0 = a_u.blocks.find((b: any) => b.t === 'Para');
        const destSiJson = JSON.stringify(pool[para0.s]);

        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('Edited.\n'));
        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success).toBe(true);

        // Exact assertions — not just presence.
        expect(editResult.qmd).toContain('Edited.');
        expect(editResult.qmd).not.toContain('First para.');
        expect(editResult.qmd).toContain('Second para.');
    });

    it('edit preserves inline markdown (typed emphasis round-trips correctly)', async () => {
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const para = a_u.blocks.find((b: any) => b.t === 'Para');
        const destSiJson = JSON.stringify(pool[para.s]);

        // The replacement text uses inline markdown.
        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('Hello *world*.\n'));
        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(CONTENT, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success).toBe(true);
        // The asterisks must survive the round-trip.
        expect(editResult.qmd).toContain('*world*');
        expect(editResult.qmd).not.toContain('Hello world.');
    });
});

// ---------------------------------------------------------------------------
// parseQmdContentSync accepts text without trailing newline.
// ---------------------------------------------------------------------------

describe('parseQmdContentSync no-trailing-newline', () => {
    it('parseQmdContentSync handles text without trailing newline', () => {
        // parse_qmd_content is the underlying WASM function; the TS wrapper
        // (parseQmdContentSync in wasmRenderer.ts) is a thin JSON.parse layer.
        const result: AstResponse = JSON.parse(wasm.parse_qmd_content('a *emph* word'));
        expect(result.success).toBe(true);
        expect(result.ast).toBeTruthy();
    });
});

// ---------------------------------------------------------------------------
// Slice fidelity — byte offsets from pool entries must round-trip through
// sliceBytes to recover the verbatim source text without trailing newlines.
// ---------------------------------------------------------------------------

describe('slice fidelity', () => {
    // Uses PATH ('/project/doc.qmd') — overridden inside each test, consistent
    // with the 'full edit round-trip' pattern above.

    it('sliceBytes recovers paragraph text without trailing newline', async () => {
        const content = '---\nformat: q2-preview\n---\n\na *emph* word\n\n## My Heading\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const para = a_u.blocks.find((b: any) => b.t === 'Para');
        expect(para, 'untransformed AST must contain a Para block').toBeTruthy();

        const entry = pool[para.s];
        // The Rust parser spans may include a trailing '\n'; trimEnd() reflects what
        // the textarea would receive.  The important invariant is that the inline
        // markdown content is fully present.
        const sliced = sliceBytes(content, entry.r[0], entry.r[1]).trimEnd();
        expect(sliced).toBe('a *emph* word');
    });

    it('sliceBytes recovers heading text without trailing newline', async () => {
        const content = '---\nformat: q2-preview\n---\n\na *emph* word\n\n## My Heading\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        // Header block has type 'Header'
        const heading = a_u.blocks.find((b: any) => b.t === 'Header');
        expect(heading, 'untransformed AST must contain a Header block').toBeTruthy();

        const entry = pool[heading.s];
        // The Rust parser spans may include a trailing '\n'; trimEnd() reflects what
        // the textarea would receive.  The important invariant is that the '#' prefix
        // and heading text are fully present.
        const sliced = sliceBytes(content, entry.r[0], entry.r[1]).trimEnd();
        expect(sliced).toBe('## My Heading');
    });
});

// ---------------------------------------------------------------------------
// Surrounding-verbatim round-trips — blocks that are not edited must appear
// byte-verbatim in the output QMD.
// ---------------------------------------------------------------------------

describe('surrounding-verbatim round-trips', () => {
    it('editing the middle block preserves surrounding blocks verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\nSecond para.\n\nThird para.\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const paras: any[] = a_u.blocks.filter((b: any) => b.t === 'Para');
        // There should be exactly 3 Para blocks.
        expect(paras.length).toBe(3);

        // Edit the middle paragraph (index 1).
        const middlePara = paras[1];
        const destSiJson = JSON.stringify(pool[middlePara.s]);
        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('Edited para.\n'));
        expect(subtree.success).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);

        // Surrounding blocks must appear byte-verbatim.
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('\n\nThird para.\n');
        // The edited block is present; the original middle text is gone.
        expect(editResult.qmd).toContain('Edited para.');
        expect(editResult.qmd).not.toContain('Second para.');
    });

    it('editing the last block preserves preceding blocks verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\nSecond para.\n\nThird para.\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const paras: any[] = a_u.blocks.filter((b: any) => b.t === 'Para');
        expect(paras.length).toBe(3);

        // Edit the last paragraph (index 2).
        const lastPara = paras[2];
        const destSiJson = JSON.stringify(pool[lastPara.s]);
        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('EOF edit.\n'));
        expect(subtree.success).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);

        // First two paragraphs must appear verbatim.
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('Second para.');
        expect(editResult.qmd).toContain('EOF edit.');
        expect(editResult.qmd).not.toContain('Third para.');
    });
});

// ---------------------------------------------------------------------------
// Inline markdown round-trip — emphasis syntax survives apply_node_edit.
// ---------------------------------------------------------------------------

describe('inline markdown round-trip via apply_node_edit', () => {
    it('inline emphasis survives the round-trip unchanged', async () => {
        const content = '---\nformat: q2-preview\n---\n\na *emph* word\n\n## My Heading\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const para = a_u.blocks.find((b: any) => b.t === 'Para');
        const destSiJson = JSON.stringify(pool[para.s]);

        // Replace with the same inline markdown.
        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('a *emph* word\n'));
        expect(subtree.success).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);
        // Inline markdown must be preserved.
        expect(editResult.qmd).toContain('a *emph* word');
    });
});

// ---------------------------------------------------------------------------
// Heading round-trip — the # prefix is written by the serializer, not the
// caller.  Passing '## My Heading' as newText must produce '## My Heading'
// in the output without any manual prepending.
// ---------------------------------------------------------------------------

describe('heading round-trip', () => {
    it('heading level and # prefix are preserved through the writer', async () => {
        const content = '---\nformat: q2-preview\n---\n\na *emph* word\n\n## My Heading\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const heading = a_u.blocks.find((b: any) => b.t === 'Header');
        expect(heading, 'untransformed AST must contain a Header block').toBeTruthy();
        const destSiJson = JSON.stringify(pool[heading.s]);

        // Parse the heading text as new QMD.
        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('## My Heading\n'));
        expect(subtree.success).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);
        // The ## prefix must appear in the output — no manual prepending needed.
        expect(editResult.qmd).toContain('## My Heading');
    });
});

// ---------------------------------------------------------------------------
// Tier-1 round-trips — CodeBlock and RawBlock (Plan 2b checklist item 12).
// Surrounding blocks must appear byte-verbatim after editing.
// ---------------------------------------------------------------------------

describe('CodeBlock round-trip', () => {
    it('editing a fenced code block preserves surrounding paragraphs byte-verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\n```python\nprint("hi")\n```\n\nSecond para.\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const codeBlock = a_u.blocks.find((b: any) => b.t === 'CodeBlock');
        expect(codeBlock, 'untransformed AST must contain a CodeBlock').toBeTruthy();
        const destSiJson = JSON.stringify(pool[codeBlock.s]);

        const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content('```python\nprint("hello world")\n```\n'));
        expect(subtree.success, `parse failed: ${subtree.error}`).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);

        // Edited content present; original code absent.
        expect(editResult.qmd).toContain('print("hello world")');
        expect(editResult.qmd).not.toContain('print("hi")');
        // Surrounding paragraphs must be byte-verbatim.
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('\n\nSecond para.\n');
    });
});

describe('RawBlock round-trip', () => {
    it('editing a raw HTML block preserves surrounding paragraphs byte-verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nBefore raw.\n\n```{=html}\n<div>original html</div>\n```\n\nAfter raw.\n';
        vfsAddFile(PATH, content);

        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);

        const a_u = JSON.parse(result.untransformed_ast_json!);
        const pool: any[] = a_u.astContext.p;
        const rawBlock = a_u.blocks.find((b: any) => b.t === 'RawBlock');
        expect(rawBlock, 'untransformed AST must contain a RawBlock').toBeTruthy();
        const destSiJson = JSON.stringify(pool[rawBlock.s]);

        const subtree: AstResponse = JSON.parse(
            wasm.parse_qmd_content('```{=html}\n<div>updated html</div>\n```\n'),
        );
        expect(subtree.success, `parse failed: ${subtree.error}`).toBe(true);

        const editResult: AstResponse = JSON.parse(
            wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
        );
        expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);

        // Edited raw block present; original absent.
        expect(editResult.qmd).toContain('<div>updated html</div>');
        expect(editResult.qmd).not.toContain('<div>original html</div>');
        // Surrounding paragraphs must be byte-verbatim.
        expect(editResult.qmd).toContain('Before raw.\n\n');
        expect(editResult.qmd).toContain('\n\nAfter raw.\n');
    });
});

// ---------------------------------------------------------------------------
// Tier-2 round-trips (Plan 2c §3) — container blocks edited as whole-block
// text-channel replacements. Surrounding paragraphs must be byte-verbatim;
// the edited block itself may be reformatted by the writer (no byte-identity
// assertion on the edited block — only content presence).
//
// Plan 3's Rust node_edit_tests.rs covers nested-descent correctness; these
// exercise the complete WASM pipeline (render → pool → edit → apply) for the
// top-level container types not previously covered.
// ---------------------------------------------------------------------------

/** Render a content fixture, locate the first block of `blockType`, replace it
 *  via apply_node_edit with `newQmd`, and return the resulting QMD. */
async function editFirstBlock(
    content: string,
    blockType: string,
    newQmd: string,
): Promise<AstResponse> {
    vfsAddFile(PATH, content);
    const result: RenderResponse = JSON.parse(
        await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
    );
    expect(result.success, result.error).toBe(true);

    const a_u = JSON.parse(result.untransformed_ast_json!);
    const pool: any[] = a_u.astContext.p;
    const block = a_u.blocks.find((b: any) => b.t === blockType);
    expect(block, `untransformed AST must contain a ${blockType} block`).toBeTruthy();
    const destSiJson = JSON.stringify(pool[block.s]);

    const subtree: AstResponse = JSON.parse(wasm.parse_qmd_content(newQmd));
    expect(subtree.success, `parse failed: ${subtree.error}`).toBe(true);

    const editResult: AstResponse = JSON.parse(
        wasm.apply_node_edit(content, result.untransformed_ast_json!, destSiJson, subtree.ast!),
    );
    expect(editResult.success, `apply_node_edit failed: ${editResult.error}`).toBe(true);
    return editResult;
}

describe('Div round-trip', () => {
    it('editing a fenced div preserves surrounding paragraphs byte-verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\n::: {.note}\nDiv body.\n:::\n\nSecond para.\n';
        const editResult = await editFirstBlock(content, 'Div', '::: {.note}\nEdited div body.\n:::\n');
        expect(editResult.qmd).toContain('Edited div body.');
        expect(editResult.qmd).not.toContain('Div body.');
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('\n\nSecond para.\n');
    });
});

describe('BlockQuote round-trip', () => {
    it('editing a block quote preserves surrounding paragraphs byte-verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\n> Quoted text.\n\nSecond para.\n';
        const editResult = await editFirstBlock(content, 'BlockQuote', '> Edited quote.\n');
        expect(editResult.qmd).toContain('Edited quote.');
        expect(editResult.qmd).not.toContain('Quoted text.');
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('\n\nSecond para.\n');
    });
});

describe('BulletList round-trip', () => {
    it('editing a bullet list preserves surrounding paragraphs byte-verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\n- item one\n- item two\n\nSecond para.\n';
        // Sibling items may be reformatted — assert only the edited content +
        // surrounding-paragraph verbatim, per the plan.
        const editResult = await editFirstBlock(
            content,
            'BulletList',
            '- edited one\n- edited two\n',
        );
        expect(editResult.qmd).toContain('edited one');
        expect(editResult.qmd).toContain('edited two');
        expect(editResult.qmd).not.toContain('item one');
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('\n\nSecond para.\n');
    });
});

describe('OrderedList round-trip', () => {
    it('editing an ordered list preserves surrounding paragraphs byte-verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\n1. item one\n2. item two\n\nSecond para.\n';
        const editResult = await editFirstBlock(
            content,
            'OrderedList',
            '1. edited one\n2. edited two\n',
        );
        expect(editResult.qmd).toContain('edited one');
        expect(editResult.qmd).toContain('edited two');
        expect(editResult.qmd).not.toContain('item one');
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('\n\nSecond para.\n');
    });
});

describe('DefinitionList round-trip', () => {
    // postprocess.rs always fully rewrites DefinitionList (a desugared form of
    // Div(.definition-list)), so no byte-identity assertion on the edited block.
    it('editing a definition list preserves surrounding paragraphs byte-verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\n::: {.definition-list}\n* term 1\n  - definition body\n:::\n\nSecond para.\n';
        const editResult = await editFirstBlock(
            content,
            'DefinitionList',
            '::: {.definition-list}\n* term 1\n  - edited definition\n:::\n',
        );
        expect(editResult.qmd).toContain('edited definition');
        expect(editResult.qmd).not.toContain('definition body');
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('\n\nSecond para.\n');
    });
});

describe('Table round-trip', () => {
    it('editing a GFM table preserves surrounding paragraphs byte-verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nSecond para.\n';
        const editResult = await editFirstBlock(
            content,
            'Table',
            '| A | B |\n|---|---|\n| 9 | 8 |\n',
        );
        expect(editResult.qmd).toContain('9');
        expect(editResult.qmd).toContain('8');
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('\n\nSecond para.\n');
    });
});

describe('Figure round-trip', () => {
    it('editing a crossref figure preserves surrounding paragraphs byte-verbatim', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\n![A caption.](img.png){#fig-foo}\n\nSecond para.\n';
        const editResult = await editFirstBlock(
            content,
            'Figure',
            '![An edited caption.](img.png){#fig-foo}\n',
        );
        expect(editResult.qmd).toContain('An edited caption.');
        expect(editResult.qmd).not.toContain('A caption.');
        expect(editResult.qmd).toContain('First para.\n\n');
        expect(editResult.qmd).toContain('\n\nSecond para.\n');
    });

    // Exploratory (per plan): record what happens to an id-less image — is the
    // AST node still a first-class `Figure`, and does it carry a pool ref
    // (data-block-pool-id, i.e. `.s`)? We don't require either; the test just
    // documents the observed behavior for future readers.
    it('records whether an id-less image renders as an editable Figure', async () => {
        const content =
            '---\nformat: q2-preview\n---\n\nFirst para.\n\n![A caption.](img.png)\n\nSecond para.\n';
        vfsAddFile(PATH, content);
        const result: RenderResponse = JSON.parse(
            await wasm.render_page_in_project_with_attribution(PATH, undefined, null),
        );
        expect(result.success, result.error).toBe(true);
        const a_u = JSON.parse(result.untransformed_ast_json!);
        const figure = a_u.blocks.find((b: any) => b.t === 'Figure');
        const para = a_u.blocks.find(
            (b: any) => b.t === 'Para' && JSON.stringify(b).includes('img.png'),
        );
        // Observed (2026-06-11): an id-less single-image paragraph IS promoted
        // to a first-class `Figure` AST node (implicit-figure behavior) — the
        // console line below logs `Figure node present: true`. So the `{#fig-foo}`
        // id is not required to get a Figure node here; it governs crossref
        // numbering, not figure-promotion. (If this ever flips, update the note.)
        // eslint-disable-next-line no-console
        console.log(
            `[Figure exploratory] id-less image → Figure node present: ${!!figure}, ` +
                `Para-with-image present: ${!!para}`,
        );
        expect(figure ?? para, 'id-less image must appear somewhere in the AST').toBeTruthy();
    });
});
