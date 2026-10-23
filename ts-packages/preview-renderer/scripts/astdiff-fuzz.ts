/**
 * Fuzzer for the AST differ: insert simple / common qmd snippets at many
 * positions of parseable seed documents, parse before and after with the
 * pampa CLI, diff, and report the insertions that produce a large edit
 * script relative to what was typed.
 *
 *   npx tsx scripts/astdiff-fuzz.ts [--pampa <path>] [--max <n>] [--top <n>]
 *                                   [--positions boundaries|all] [--seed <name>] [--dump <json>]
 *
 * "Churn" is the number of AST nodes (objects with a `t`) inside inserted
 * subtrees plus inside deleted subtrees, plus created and changed nodes.
 * A simple insertion should churn only what it typed; anything else is a
 * place where the differ fell back to delete + insert.
 */

import { execFile } from 'node:child_process';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';
import { reconcileAst, describeOp, type Op, type Touched } from '../src/astdiff/index';
import type { PandocAST } from '../src/framework/types';

const execFileP = promisify(execFile);

// --- CLI -------------------------------------------------------------------

const args = process.argv.slice(2);
const opt = (name: string, dflt: string): string => {
    const i = args.indexOf(`--${name}`);
    return i !== -1 && args[i + 1] !== undefined ? args[i + 1] : dflt;
};
const PAMPA = resolve(opt('pampa', '../../target/debug/pampa'));
const MAX_TRIALS = Number(opt('max', '20000'));
const TOP = Number(opt('top', '30'));
const POSITIONS = opt('positions', 'boundaries');
const ONLY_SEED = opt('seed', '');
const WORKERS = Number(opt('workers', '8'));
const DUMP = opt('dump', '');

// --- seeds -------------------------------------------------------------------

const SEEDS: Record<string, string> = {
    prose: `# Title

A first paragraph with *emphasis*, **strong** text and \`code\`.
It continues on a second line with a [link](https://example.com).

## Section

Another paragraph here. It has a few sentences. Nothing special.

### Subsection

Last paragraph of the prose seed.
`,
    lists: `Intro paragraph.

- first item
- second item with *emph*
  - nested one
  - nested two
- third item

1. one
2. two
3. three

Closing paragraph.
`,
    blocks: `::: {.callout-note}
Inside a div.

Second paragraph in the div.
:::

> A quote with **bold**.
> Second quoted line.

\`\`\`python
x = 1
print(x)
\`\`\`

---

After the rule.
`,
    nested: `::: {}

## Quarto stuff!!! this is all pretty cool

asdds a q

this is pretty nice now! a cool dog is my friend!

and what of it?
- This is all diff'd
- > we are quoting things and whatnot

what is up? ssss
`,
    extras: `Text with a footnote[^1] and math $x^2$ inline.

$$
E = mc^2
$$

| a | b |
|---|---|
| 1 | 2 |

[^1]: The note text.

Final line.
`,
};

const TOKENS: string[] = [
    // single characters
    ' ', 'a', 'Z', '1', '.', ',', '\n', '#', '*', '_', '-', '>', ':', '`', '[', ']', '(', ')', '!', '|', '$', '\\', '^', '~', '<', '{', '}', '@', '\t',
    // common sequences
    '\n\n', '# ', '## ', '- ', '1. ', '> ', '::: ', ':::\n', '::: {}\n', '::: {.x}\n', '```\n', '```', '**', '__', '](', '![', '$$', '<!-- ', '-->',
    ' {.class}', ' {#id}', '  ', '\\\n', '[^1]', 'word ', ' word', ' word.', 'Hello world', '{{< ', ' >}}', '\n- ', '\n> ', '\n# ', '\n\n# ', '\n:::\n',
];

// --- helpers -----------------------------------------------------------------

async function parse(file: string): Promise<PandocAST | null> {
    try {
        const { stdout } = await execFileP(PAMPA, ['-t', 'json', '--json-source-location', 'full', file], { maxBuffer: 64 * 1024 * 1024 });
        const start = stdout.indexOf('{');
        if (start === -1) return null;
        return JSON.parse(stdout.slice(start)) as PandocAST;
    } catch {
        return null;
    }
}

function countNodes(v: unknown): number {
    if (Array.isArray(v)) return v.reduce((n: number, x) => n + countNodes(x), 0);
    if (v !== null && typeof v === 'object') {
        const o = v as Record<string, unknown>;
        let n = typeof o.t === 'string' ? 1 : 0;
        for (const k of Object.keys(o)) if (k === 'c' || k === 'slots' || k === 'value') n += countNodes(o[k]);
        return n;
    }
    return 0;
}

function churnOf(t: Touched): { inserted: number; removed: number; created: number; changed: number; total: number } {
    const inserted = t.insertedSubtrees.reduce((n, x) => n + Math.max(1, countNodes(x)), 0);
    const removed = t.removedNodes.reduce((n, x) => n + Math.max(1, countNodes(x)), 0);
    const created = t.createdNodes.length;
    const changed = t.changedNodes.length;
    return { inserted, removed, created, changed, total: inserted + removed + created + changed };
}

function positionsFor(text: string): number[] {
    if (POSITIONS === 'all') return Array.from({ length: text.length + 1 }, (_, i) => i);
    const set = new Set<number>([0, text.length]);
    for (let i = 0; i < text.length; i++) {
        const ch = text[i];
        if (ch === '\n') { set.add(i); set.add(i + 1); }
        if (ch === ' ') { set.add(i); set.add(i + 1); }
    }
    return Array.from(set).sort((a, b) => a - b);
}

function show(s: string): string {
    return JSON.stringify(s);
}

function context(text: string, pos: number): string {
    const lineStart = text.lastIndexOf('\n', pos - 1) + 1;
    let lineEnd = text.indexOf('\n', pos);
    if (lineEnd === -1) lineEnd = text.length;
    const line = text.slice(lineStart, lineEnd);
    const col = pos - lineStart;
    const lineNo = text.slice(0, lineStart).split('\n').length;
    return `L${lineNo}:${col} ${show(line.slice(0, col))}|${show(line.slice(col))}`;
}

interface Trial {
    seed: string;
    token: string;
    pos: number;
}

interface Result extends Trial {
    ops: Op[];
    churn: ReturnType<typeof churnOf>;
    reused: boolean;
}

async function runTrial(trial: Trial, base: PandocAST, dir: string, k: number): Promise<Result | 'noparse'> {
    const text = SEEDS[trial.seed];
    const edited = text.slice(0, trial.pos) + trial.token + text.slice(trial.pos);
    const file = join(dir, `t${k}.qmd`);
    writeFileSync(file, edited);
    const after = await parse(file);
    if (after === null) return 'noparse';
    const current = structuredClone(base);
    const r = reconcileAst(current, after);
    return { ...trial, ops: r.ops, churn: churnOf(r.touched), reused: r.reused };
}

async function pool<T, R>(items: T[], workers: number, fn: (item: T, i: number) => Promise<R>): Promise<R[]> {
    const out: R[] = new Array(items.length);
    let next = 0;
    const run = async () => {
        for (;;) {
            const i = next++;
            if (i >= items.length) return;
            out[i] = await fn(items[i], i);
        }
    };
    await Promise.all(Array.from({ length: workers }, run));
    return out;
}

// --- main ----------------------------------------------------------------------

async function main() {
    const dir = mkdtempSync(join(tmpdir(), 'astdiff-fuzz-'));
    const seedNames = Object.keys(SEEDS).filter((s) => ONLY_SEED === '' || s === ONLY_SEED);
    const bases: Record<string, PandocAST> = {};
    for (const name of seedNames) {
        const file = join(dir, `${name}.qmd`);
        writeFileSync(file, SEEDS[name]);
        const ast = await parse(file);
        if (!ast) throw new Error(`seed ${name} does not parse`);
        bases[name] = ast;
    }

    let trials: Trial[] = [];
    for (const seed of seedNames) {
        for (const pos of positionsFor(SEEDS[seed])) {
            for (const token of TOKENS) trials.push({ seed, token, pos });
        }
    }
    if (trials.length > MAX_TRIALS) {
        // Deterministic thinning: keep every k-th trial.
        const step = trials.length / MAX_TRIALS;
        trials = trials.filter((_, i) => Math.floor(i % step) === 0 || i % Math.ceil(step) === 0).slice(0, MAX_TRIALS);
    }
    console.log(`seeds: ${seedNames.join(', ')} | tokens: ${TOKENS.length} | trials: ${trials.length} | workers: ${WORKERS}`);

    const t0 = Date.now();
    let done = 0;
    const results = await pool(trials, WORKERS, async (trial, i) => {
        const r = await runTrial(trial, bases[trial.seed], dir, i);
        done++;
        if (done % 1000 === 0) console.error(`  ${done}/${trials.length} (${((Date.now() - t0) / 1000).toFixed(0)}s)`);
        return r;
    });

    const ok = results.filter((r): r is Result => r !== 'noparse');
    const noparse = results.length - ok.length;
    const fallback = ok.filter((r) => !r.reused);
    console.log(`parsed: ${ok.length} | did not parse: ${noparse} | reconcile fallbacks: ${fallback.length} | ${((Date.now() - t0) / 1000).toFixed(0)}s\n`);

    // Large = churn beyond the token's own worth. Thresholds are generous on
    // purpose: we want structural fallbacks, not debates about one Space.
    const tokenWorth = (token: string) => Math.max(2, token.split(/\s+/).filter(Boolean).length + 2);
    const large = ok.filter((r) => r.churn.total > tokenWorth(r.token) + 4 || r.churn.removed >= 3);
    console.log(`large diffs: ${large.length} of ${ok.length} (${((100 * large.length) / ok.length).toFixed(1)}%)\n`);

    // Per-token summary.
    console.log('per token (large / trials, max churn):');
    const byToken = new Map<string, { n: number; large: number; max: number }>();
    for (const r of ok) {
        const e = byToken.get(r.token) ?? { n: 0, large: 0, max: 0 };
        e.n++;
        if (large.includes(r)) e.large++;
        e.max = Math.max(e.max, r.churn.total);
        byToken.set(r.token, e);
    }
    for (const [token, e] of [...byToken.entries()].sort((a, b) => b[1].large / b[1].n - a[1].large / a[1].n)) {
        if (e.large === 0) continue;
        console.log(`  ${show(token).padEnd(14)} ${String(e.large).padStart(4)} / ${String(e.n).padEnd(5)} max churn ${e.max}`);
    }

    // Worst cases.
    console.log(`\ntop ${TOP} by churn:`);
    const worst = [...large].sort((a, b) => b.churn.total - a.churn.total).slice(0, TOP);
    for (const r of worst) {
        const c = r.churn;
        console.log(`\n[${r.seed}] insert ${show(r.token)} at ${context(SEEDS[r.seed], r.pos)}`);
        console.log(`  churn ${c.total} (inserted ${c.inserted}, removed ${c.removed}, created ${c.created}, changed ${c.changed}) | ${r.ops.length} ops${r.reused ? '' : ' | FALLBACK'}`);
        const lines = r.ops.map(describeOp);
        for (const l of lines.slice(0, 8)) console.log(`    ${l.length > 110 ? l.slice(0, 109) + '…' : l}`);
        if (lines.length > 8) console.log(`    … ${lines.length - 8} more`);
    }

    if (DUMP) {
        const { writeFileSync: write } = await import('node:fs');
        write(DUMP, JSON.stringify(large.map((r) => ({
            seed: r.seed, token: r.token, pos: r.pos, context: context(SEEDS[r.seed], r.pos),
            churn: r.churn, reused: r.reused, ops: r.ops.map(describeOp),
        })), null, 1));
    }

    if (fallback.length > 0) {
        console.log(`\nreconcile fallbacks (edit script did not reproduce the fresh parse):`);
        for (const r of fallback.slice(0, 10)) console.log(`  [${r.seed}] ${show(r.token)} at ${context(SEEDS[r.seed], r.pos)}`);
    }
}

main().catch((err) => {
    console.error(err);
    process.exit(1);
});
