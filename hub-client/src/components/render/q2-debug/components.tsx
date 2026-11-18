/**
 * q2-debug: a DOM-inspector style view of the Pandoc AST.
 *
 * One generic `DebugNode` renders every tag: a row with a disclosure
 * triangle, the tag, every field the node carries (attributes, targets,
 * levels, list numbering, source refs, Quarto extras, anything else on
 * the object) and, below it, the children. Block children are rows of
 * their own, indented; inline children flow and wrap inside the row as
 * chips, where a `Str` is just its text and whitespace a single glyph.
 * Click a triangle or tag to collapse a node; Alt+click collapses or
 * expands its whole subtree. Collapsed content is shown as `…`.
 *
 * The per-tag exports (`Para`, `Str`, ...) all render `DebugNode`; they
 * exist so the registry keeps one entry per tag for user TSX overrides.
 * Children are walked through the framework's `Node` so the atomic gate
 * and `setLocalAst` scoping keep working.
 */

import { createContext, useCallback, useContext, useMemo, useState, type MouseEvent, type ReactNode } from 'react';
import {
    Node,
    RegistryContext,
    blockTypes,
    useAttributionHover,
} from '@quarto/preview-renderer/framework';
import type {
    Attr,
    BlockNode,
    CustomBlockNode,
    CustomInlineNode,
    InlineNode,
    NodeArgs,
    PandocAST,
    Slot,
} from '@quarto/preview-renderer/framework';
import { DEBUG_STYLESHEET } from './styles';

type AnyNode = BlockNode | InlineNode;
type Args = NodeArgs<AnyNode>;
type Json = Record<string, unknown>;

// ---------------------------------------------------------------------------
// Collapsing: each node keeps its own state; an Alt+click broadcasts a
// state to the subtree through context, and the most recent broadcast
// (highest generation) wins over anything older.
// ---------------------------------------------------------------------------

interface Broadcast { gen: number; open: boolean }
const CollapseContext = createContext<Broadcast>({ gen: 0, open: true });
let nextGen = 1;

function useCollapsible(defaultOpen: boolean) {
    const parent = useContext(CollapseContext);
    const [own, setOwn] = useState<Broadcast | null>(null);
    const [broadcast, setBroadcast] = useState<Broadcast | null>(null);
    const open = own !== null && own.gen >= parent.gen
        ? own.open
        : parent.gen === 0 ? defaultOpen : parent.open;
    const toggle = useCallback((e: MouseEvent) => {
        e.stopPropagation();
        const next = !open;
        if (e.altKey) {
            const b = { gen: nextGen++, open: next };
            setOwn(b);
            setBroadcast(b);
        } else {
            setOwn({ gen: parent.gen, open: next });
        }
    }, [open, parent.gen]);
    const childContext = broadcast !== null && broadcast.gen > parent.gen ? broadcast : parent;
    return { open, toggle, childContext };
}

// ---------------------------------------------------------------------------
// Small value renderers
// ---------------------------------------------------------------------------

function Scalar({ value }: { value: unknown }) {
    if (value === null || value === undefined) return <span className="q2d-null">null</span>;
    switch (typeof value) {
        case 'string': return <span className="q2d-string">"{value}"</span>;
        case 'number': return <span className="q2d-number">{value}</span>;
        case 'boolean': return <span className="q2d-bool">{String(value)}</span>;
        default: return <span>{String(value)}</span>;
    }
}

function isScalar(value: unknown): boolean {
    return value === null || typeof value !== 'object';
}

/** A JSON value: scalars inline, containers as a collapsible subtree. */
function JsonValue({ value, defaultOpen = false }: { value: unknown; defaultOpen?: boolean }) {
    const { open, toggle, childContext } = useCollapsible(defaultOpen);
    if (isScalar(value)) return <Scalar value={value} />;
    const isArray = Array.isArray(value);
    const entries = isArray
        ? (value as unknown[]).map((v, i) => [String(i), v] as const)
        : Object.entries(value as Json);
    const brackets = isArray ? ['[', ']'] : ['{', '}'];
    if (entries.length === 0) return <span>{brackets[0]}{brackets[1]}</span>;
    return (
        <span className="q2d-json">
            <span className="q2d-twisty" onClick={toggle}>{open ? '▾' : '▸'}</span>
            {!open && <span className="q2d-summary" onClick={toggle}>{brackets[0]}…{brackets[1]}</span>}
            {open && (
                <CollapseContext.Provider value={childContext}>
                    <span>{brackets[0]}</span>
                    <div className="q2d-json-block">
                        {entries.map(([k, v]) => (
                            <div className="q2d-json-row" key={k}>
                                <span className="q2d-key">{k}</span>:
                                <JsonValue value={v} />
                            </div>
                        ))}
                    </div>
                    <span>{brackets[1]}</span>
                </CollapseContext.Provider>
            )}
        </span>
    );
}

function Prop({ name, children }: { name: string; children: ReactNode }) {
    return <span className="q2d-prop"><span className="q2d-key">{name}</span>=<span className="q2d-val">{children}</span></span>;
}

/** A Pandoc Attr triple as `#id .class key="value"`; nothing when empty. */
function AttrView({ attr, name = 'attr' }: { attr: Attr | null | undefined; name?: string }) {
    if (!attr) return null;
    const [id, classes, kvs] = attr;
    if (!id && classes.length === 0 && kvs.length === 0) return null;
    return (
        <span className="q2d-prop" title={name}>
            {id && <span className="q2d-attr-id">#{id}</span>}
            {classes.map((c, i) => <span className="q2d-attr-class" key={i}> .{c}</span>)}
            {kvs.map(([k, v], i) => <span key={i}> <span className="q2d-key">{k}</span>=<span className="q2d-string">"{v}"</span></span>)}
        </span>
    );
}

interface Loc { f: number; b: { o: number; l: number; c: number }; e: { o: number; l: number; c: number } }

/** View options, set from the corner control. */
const OptionsContext = createContext<{ refs: boolean }>({ refs: true });

/** Source refs: the pool index `s` (with the pool entry on hover) and the resolved location `l`. */
function Refs({ node }: { node: Json }) {
    const { refs } = useContext(OptionsContext);
    const { sourceInfoPool } = useContext(RegistryContext);
    if (!refs) return null;
    const s = node.s as number | undefined;
    const l = node.l as Loc | undefined;
    const attrS = node.attrS as number | undefined;
    if (s === undefined && l === undefined && attrS === undefined) return null;
    const entry = s !== undefined ? (sourceInfoPool as unknown as unknown[] | undefined)?.[s] : undefined;
    return (
        <span className="q2d-ref" title={[
            s !== undefined ? `s=${s}${entry !== undefined ? ' ' + JSON.stringify(entry) : ''}` : null,
            attrS !== undefined ? `attrS=${attrS}` : null,
            l !== undefined ? `l: file ${l.f}, ${l.b.l}:${l.b.c} (offset ${l.b.o}) – ${l.e.l}:${l.e.c} (offset ${l.e.o})` : null,
        ].filter((x) => x !== null).join('\n')}>
            {s !== undefined && `s${s}`}
            {attrS !== undefined && ` a${attrS}`}
            {l !== undefined && ` ${l.b.l}:${l.b.c}–${l.e.l}:${l.e.c}`}
        </span>
    );
}

/** Any key on the node this renderer does not already show. */
function Extras({ node, shown }: { node: Json; shown: readonly string[] }) {
    const skip = new Set(['t', 'c', 's', 'l', 'attrS', ...shown]);
    const rest = Object.entries(node).filter(([k]) => !skip.has(k));
    return <>{rest.map(([k, v]) => <Prop key={k} name={k}><JsonValue value={v} /></Prop>)}</>;
}

// ---------------------------------------------------------------------------
// Children: always through the framework's `Node`, with a scoped setter.
// ---------------------------------------------------------------------------

function Children({ nodes, args, update }: {
    nodes: AnyNode[];
    args: Args;
    update: (next: AnyNode[]) => AnyNode;
}) {
    return (
        <>
            {nodes.map((child, i) => (
                <Node
                    key={i}
                    node={child}
                    onNavigateToDocument={args.onNavigateToDocument}
                    setLocalAst={(n) => {
                        const next = nodes.slice();
                        next[i] = n;
                        args.setLocalAst(update(next));
                    }}
                />
            ))}
        </>
    );
}

/** A sequence of inline nodes, flowing. */
function Inlines(props: { nodes: InlineNode[]; args: Args; update: (next: InlineNode[]) => AnyNode }) {
    return <span className="q2d-inlines"><Children nodes={props.nodes} args={props.args} update={(n) => props.update(n as InlineNode[])} /></span>;
}

/** A sequence of block nodes, one row each. */
function Blocks(props: { nodes: BlockNode[]; args: Args; update: (next: BlockNode[]) => AnyNode }) {
    return <Children nodes={props.nodes} args={props.args} update={(n) => props.update(n as BlockNode[])} />;
}

function SlotView({ name, slot, args, update }: { name: string; slot: Slot; args: Args; update: (next: Slot) => AnyNode }) {
    const label = <span className="q2d-slot">{name}:</span>;
    switch (slot.kind) {
        case 'block':
            return <div>{label}<div className="q2d-children"><Node node={slot.value} onNavigateToDocument={args.onNavigateToDocument} setLocalAst={(n) => args.setLocalAst(update({ kind: 'block', value: n as BlockNode }))} /></div></div>;
        case 'inline':
            return <div>{label} <Node node={slot.value} onNavigateToDocument={args.onNavigateToDocument} setLocalAst={(n) => args.setLocalAst(update({ kind: 'inline', value: n as InlineNode }))} /></div>;
        case 'blocks':
            return <div>{label}<div className="q2d-children"><Blocks nodes={slot.value} args={args} update={(n) => update({ kind: 'blocks', value: n })} /></div></div>;
        case 'inlines':
            return <div>{label} <Inlines nodes={slot.value} args={args} update={(n) => update({ kind: 'inlines', value: n })} /></div>;
    }
}

// ---------------------------------------------------------------------------
// What each tag has: its props, how its children render, and a summary.
// ---------------------------------------------------------------------------

interface Shape {
    /** Fields rendered in the row. */
    props: ReactNode;
    /** Top-level node keys the props already cover (kept out of Extras). */
    shown: readonly string[];
    /** Children, rendered below the row (blocks) or inside it (inlines). */
    body: ReactNode;
    /** Whether `body` is inline content that belongs on the row. */
    inlineBody: boolean;
}

function describe(args: Args): Shape {
    const node = args.node as Json & { t: string };
    const t = node.t;
    const c = node.c;
    const set = (next: AnyNode) => next;
    const inlineChildren = (nodes: InlineNode[], update: (n: InlineNode[]) => AnyNode): Shape => ({
        props: null, shown: [], inlineBody: true,
        body: <Inlines nodes={nodes} args={args} update={update} />,
    });
    const blockChildren = (nodes: BlockNode[], update: (n: BlockNode[]) => AnyNode): Shape => ({
        props: null, shown: [], inlineBody: false,
        body: <Blocks nodes={nodes} args={args} update={update} />,
    });

    switch (t) {
        case 'Para':
            return { ...inlineChildren(c as InlineNode[], (n) => set({ ...node, c: n } as AnyNode)),
                props: <AttrView attr={node.attr as Attr | undefined} />, shown: ['attr'] };
        case 'Plain':
        case 'Emph': case 'Strong': case 'Underline': case 'Strikeout':
        case 'Superscript': case 'Subscript': case 'SmallCaps':
            return inlineChildren(c as InlineNode[], (n) => set({ ...node, c: n } as AnyNode));
        case 'Header': {
            const [level, attr, inlines] = c as [number, Attr, InlineNode[]];
            return { ...inlineChildren(inlines, (n) => set({ ...node, c: [level, attr, n] } as AnyNode)),
                props: <><Prop name="level">{level}</Prop><AttrView attr={attr} /></> };
        }
        case 'CodeBlock': {
            const [attr, text] = c as [Attr, string];
            return { props: <><AttrView attr={attr} /><Prop name="chars">{text.length}</Prop></>, shown: [], inlineBody: false,
                body: <pre className="q2d-pre">{text}</pre> };
        }
        case 'RawBlock': case 'RawInline': {
            const [format, text] = c as [string, string];
            return { props: <Prop name="format"><Scalar value={format} /></Prop>, shown: [], inlineBody: false,
                body: <pre className="q2d-pre">{text}</pre> };
        }
        case 'BlockQuote':
            return blockChildren(c as BlockNode[], (n) => set({ ...node, c: n } as AnyNode));
        case 'Div': {
            const [attr, blocks] = c as [Attr, BlockNode[]];
            return { ...blockChildren(blocks, (n) => set({ ...node, c: [attr, n] } as AnyNode)), props: <AttrView attr={attr} /> };
        }
        case 'BulletList': case 'OrderedList': {
            const isOrdered = t === 'OrderedList';
            const items = (isOrdered ? (c as [unknown, BlockNode[][]])[1] : c) as BlockNode[][];
            const itemAttr = node.itemAttr as (Attr | null)[] | undefined;
            const rebuild = (next: BlockNode[][]): AnyNode =>
                (isOrdered ? { ...node, c: [(c as [unknown, unknown])[0], next] } : { ...node, c: next }) as AnyNode;
            let props: ReactNode = <Prop name="items">{items.length}</Prop>;
            if (isOrdered) {
                const [start, style, delim] = (c as [[number, { t: string }, { t: string }], unknown])[0];
                props = <><Prop name="start">{start}</Prop><Prop name="style">{style.t}</Prop><Prop name="delim">{delim.t}</Prop>{props}</>;
            }
            return {
                props, shown: ['itemAttr'], inlineBody: false,
                body: items.map((item, i) => (
                    <div key={i}>
                        <span className="q2d-item">{isOrdered ? `${i + 1}.` : '•'}</span>
                        <AttrView attr={itemAttr?.[i]} name="itemAttr" />
                        <div className="q2d-children">
                            <Blocks nodes={item} args={args} update={(n) => { const next = items.slice(); next[i] = n; return rebuild(next); }} />
                        </div>
                    </div>
                )),
            };
        }
        case 'HorizontalRule':
            return { props: null, shown: [], inlineBody: false, body: null };
        case 'Figure': {
            const [attr, [caption, captionBlocks], body] = c as [Attr, [InlineNode[] | null, BlockNode[]], BlockNode[]];
            return {
                props: <AttrView attr={attr} />, shown: [], inlineBody: false,
                body: (
                    <>
                        {caption !== null && (
                            <div><span className="q2d-slot">caption:</span> <Inlines nodes={caption} args={args}
                                update={(n) => ({ ...node, c: [attr, [n, captionBlocks], body] } as AnyNode)} /></div>
                        )}
                        {captionBlocks.length > 0 && (
                            <div><span className="q2d-slot">caption blocks:</span><div className="q2d-children"><Blocks nodes={captionBlocks} args={args}
                                update={(n) => ({ ...node, c: [attr, [caption, n], body] } as AnyNode)} /></div></div>
                        )}
                        <Blocks nodes={body} args={args} update={(n) => ({ ...node, c: [attr, [caption, captionBlocks], n] } as AnyNode)} />
                    </>
                ),
            };
        }
        case 'LineBlock': {
            const lines = c as InlineNode[][];
            return {
                props: <Prop name="lines">{lines.length}</Prop>, shown: [], inlineBody: false,
                body: lines.map((line, i) => (
                    <div key={i}><span className="q2d-item">{i + 1}</span> <Inlines nodes={line} args={args}
                        update={(n) => { const next = lines.slice(); next[i] = n; return { ...node, c: next } as AnyNode; }} /></div>
                )),
            };
        }
        case 'DefinitionList': {
            const entries = c as [InlineNode[], BlockNode[][]][];
            return {
                props: <Prop name="terms">{entries.length}</Prop>, shown: [], inlineBody: false,
                body: entries.map(([term, defs], i) => (
                    <div key={i}>
                        <span className="q2d-slot">term:</span> <Inlines nodes={term} args={args}
                            update={(n) => { const next = entries.slice(); next[i] = [n, defs]; return { ...node, c: next } as AnyNode; }} />
                        {defs.map((def, j) => (
                            <div key={j}><span className="q2d-slot">definition {j + 1}:</span><div className="q2d-children"><Blocks nodes={def} args={args}
                                update={(n) => { const nd = defs.slice(); nd[j] = n; const next = entries.slice(); next[i] = [term, nd]; return { ...node, c: next } as AnyNode; }} /></div></div>
                        ))}
                    </div>
                )),
            };
        }
        case 'Table': {
            const [attr, [caption, captionBlocks], colspecs, head, bodies, foot] = c as [Attr, [InlineNode[] | null, BlockNode[]], unknown, unknown, unknown[], unknown];
            return {
                props: <><AttrView attr={attr} /><Prop name="bodies">{bodies.length}</Prop></>, shown: [], inlineBody: false,
                body: (
                    <>
                        {caption !== null && <div><span className="q2d-slot">caption:</span> <Inlines nodes={caption} args={args}
                            update={(n) => ({ ...node, c: [attr, [n, captionBlocks], colspecs, head, bodies, foot] } as AnyNode)} /></div>}
                        <div><span className="q2d-slot">colspecs:</span> <JsonValue value={colspecs} /></div>
                        <div><span className="q2d-slot">head:</span> <JsonValue value={head} /></div>
                        <div><span className="q2d-slot">bodies:</span> <JsonValue value={bodies} /></div>
                        <div><span className="q2d-slot">foot:</span> <JsonValue value={foot} /></div>
                    </>
                ),
            };
        }
        case 'Link': case 'Image': {
            const [attr, inlines, [url, title]] = c as [Attr, InlineNode[], [string, string]];
            return { ...inlineChildren(inlines, (n) => set({ ...node, c: [attr, n, [url, title]] } as AnyNode)),
                props: <><Prop name={t === 'Link' ? 'href' : 'src'}><Scalar value={url} /></Prop>{title && <Prop name="title"><Scalar value={title} /></Prop>}<AttrView attr={attr} /></> };
        }
        case 'Span': {
            const [attr, inlines] = c as [Attr, InlineNode[]];
            return { ...inlineChildren(inlines, (n) => set({ ...node, c: [attr, n] } as AnyNode)), props: <AttrView attr={attr} /> };
        }
        case 'Quoted': {
            const [quote, inlines] = c as [{ t: string }, InlineNode[]];
            return { ...inlineChildren(inlines, (n) => set({ ...node, c: [quote, n] } as AnyNode)), props: <Prop name="quote">{quote.t}</Prop> };
        }
        case 'Code': {
            const [attr, text] = c as [Attr, string];
            return { props: <><AttrView attr={attr} /></>, shown: [], inlineBody: true,
                body: <span className="q2d-chip q2d-str" title="Code">{text}</span> };
        }
        case 'Math': {
            const [kind, tex] = c as [{ t: string }, string];
            return { props: <Prop name="type">{kind.t}</Prop>, shown: [], inlineBody: true,
                body: <span className="q2d-chip q2d-str" title="TeX">{tex}</span> };
        }
        case 'Cite': {
            const [citations, inlines] = c as [unknown[], InlineNode[]];
            return { ...inlineChildren(inlines, (n) => set({ ...node, c: [citations, n] } as AnyNode)),
                props: <Prop name="citations"><JsonValue value={citations} /></Prop> };
        }
        case 'Note':
            return blockChildren(c as BlockNode[], (n) => set({ ...node, c: n } as AnyNode));
        case 'CustomBlock': case 'CustomInline': {
            const custom = args.node as CustomBlockNode | CustomInlineNode;
            const slots = Object.entries(custom.slots);
            return {
                props: <><Prop name="type"><Scalar value={custom.type_name} /></Prop><AttrView attr={custom.attr} /><Prop name="data"><JsonValue value={custom.plain_data} /></Prop></>,
                shown: ['type_name', 'attr', 'plain_data', 'slots'], inlineBody: false,
                body: slots.map(([name, slot]) => (
                    <SlotView key={name} name={name} slot={slot} args={args}
                        update={(next) => ({ ...custom, slots: { ...custom.slots, [name]: next } } as AnyNode)} />
                )),
            };
        }
        default:
            return { props: null, shown: [], inlineBody: false,
                body: c === undefined ? null : <div><span className="q2d-slot">c:</span> <JsonValue value={c} defaultOpen /></div> };
    }
}

// ---------------------------------------------------------------------------
// The node
// ---------------------------------------------------------------------------

const WHITESPACE: Record<string, string> = { Space: '·', SoftBreak: '↵', LineBreak: '⏎' };

const KNOWN = new Set([
    'Para', 'Plain', 'Header', 'CodeBlock', 'BulletList', 'OrderedList', 'BlockQuote', 'Div', 'HorizontalRule', 'RawBlock',
    'Figure', 'LineBlock', 'DefinitionList', 'Table', 'Str', 'Space', 'SoftBreak', 'LineBreak', 'Emph', 'Strong', 'Code',
    'Link', 'Image', 'Span', 'Math', 'Quoted', 'Underline', 'Strikeout', 'Superscript', 'Subscript', 'SmallCaps',
    'RawInline', 'Cite', 'Note', 'CustomBlock', 'CustomInline',
]);

function tagClass(t: string, isBlock: boolean): string {
    if (t === 'CustomBlock' || t === 'CustomInline') return 'q2d-tag q2d-tag-custom';
    if (!KNOWN.has(t)) return 'q2d-tag q2d-tag-unknown';
    return isBlock ? 'q2d-tag' : 'q2d-tag q2d-tag-inline';
}

export function DebugNode(args: Args) {
    const node = args.node as Json & { t: string };
    const t = node.t;
    const isBlock = blockTypes.includes(t);
    const { open, toggle, childContext } = useCollapsible(true);
    const shape = describe(args);
    const refs = <Refs node={node} />;
    const extras = <Extras node={node} shown={shape.shown} />;

    // Leaves that are only their text.
    if (t === 'Str') {
        return <span className="q2d-inline"><span className="q2d-chip q2d-str" title="Str">{String(node.c)}</span>{refs}{extras}</span>;
    }
    if (t in WHITESPACE) {
        return <span className="q2d-inline"><span className="q2d-chip q2d-ws" title={t}>{WHITESPACE[t]}</span>{refs}{extras}</span>;
    }

    const hasBody = shape.body !== null;
    const tag = <span className={tagClass(t, isBlock)} onClick={toggle} title={`${t}${hasBody ? ' — click to collapse, Alt+click for the subtree' : ''}`}>{t}</span>;

    if (!isBlock) {
        // An inline node: a chip on its parent's row, its children inside it.
        return (
            <span className={`q2d-inline q2d-container${open ? '' : ' q2d-closed'}`}>
                {tag}{shape.props}{refs}{extras}
                {hasBody && (open
                    ? <CollapseContext.Provider value={childContext}><span className="q2d-inline-body"> {shape.body}</span></CollapseContext.Provider>
                    : <span className="q2d-summary" onClick={toggle}> …</span>)}
            </span>
        );
    }

    // A block node: a row, then its children (indented, or on the row for inlines).
    return (
        <div className="q2d-block">
            <div className="q2d-row">
                <span className={`q2d-twisty${hasBody ? '' : ' q2d-leaf'}`} onClick={toggle}>{open ? '▾' : '▸'}</span>
                {tag}{shape.props}{refs}{extras}
                {hasBody && !open && <span className="q2d-summary" onClick={toggle}>…</span>}
                {hasBody && open && shape.inlineBody && (
                    <CollapseContext.Provider value={childContext}>{shape.body}</CollapseContext.Provider>
                )}
            </div>
            {hasBody && open && !shape.inlineBody && (
                <CollapseContext.Provider value={childContext}>
                    <div className="q2d-children">{shape.body}</div>
                </CollapseContext.Provider>
            )}
        </div>
    );
}

// Per-tag registry entries (collected in registry.ts): one name per tag so
// user TSX can override any of them; each is the generic node.
const perTag = (): ((props: Args) => ReactNode) => (args) => <DebugNode {...args} />;

export const Para = perTag();
export const Plain = perTag();
export const Header = perTag();
export const CodeBlock = perTag();
export const BulletList = perTag();
export const OrderedList = perTag();
export const BlockQuote = perTag();
export const Div = perTag();
export const HorizontalRule = perTag();
export const RawBlock = perTag();
export const Figure = perTag();
export const LineBlock = perTag();
export const DefinitionList = perTag();
export const Table = perTag();

export const Str = perTag();
export const Space = perTag();
export const SoftBreak = perTag();
export const LineBreak = perTag();
export const Emph = perTag();
export const Strong = perTag();
export const Code = perTag();
export const Link = perTag();
export const Image = perTag();
export const Span = perTag();
export const Quoted = perTag();
export const Math = perTag();
export const Underline = perTag();
export const Strikeout = perTag();
export const Superscript = perTag();
export const Subscript = perTag();
export const SmallCaps = perTag();
export const RawInline = perTag();
export const Cite = perTag();
export const Note = perTag();

// ---------------------------------------------------------------------------
// The document root: toolbar, version, meta, blocks.
// ---------------------------------------------------------------------------

/**
 * q2-debug document root. Delegates the badge stylesheet / hover
 * handler / overlay wiring to `useAttributionHover`. Off-path the
 * hook returns inert `hostProps` / `null` overlay+stylesheet.
 */
export const AstRenderer = ({ ast, onNavigateToDocument, setAst }: {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}) => {
    const attr = useAttributionHover();
    const [refs, setRefs] = useState(true);
    const options = useMemo(() => ({ refs }), [refs]);
    const root = ast as unknown as Json;
    const version = root['pandoc-api-version'] as number[] | undefined;
    const rootExtras = Object.entries(root).filter(([k]) => !['pandoc-api-version', 'meta', 'blocks'].includes(k));
    const blocksArgs: Args = {
        node: ast as unknown as AnyNode,
        setLocalAst: setAst as unknown as (n: AnyNode) => void,
        onNavigateToDocument,
    };
    return (
        <>
            <style>{DEBUG_STYLESHEET}</style>
            {attr.stylesheet}
            <div className="q2d pandoc-content-debug" {...attr.hostProps}>
              <OptionsContext.Provider value={options}>
                <div className="q2d-row">
                    <span className="q2d-twisty q2d-leaf" />
                    <span className="q2d-tag">Pandoc</span>
                    {version && <Prop name="api">{version.join('.')}</Prop>}
                    <Prop name="blocks">{ast.blocks.length}</Prop>
                    {rootExtras.map(([k, v]) => <Prop key={k} name={k}><JsonValue value={v} /></Prop>)}
                </div>
                <div className="q2d-children">
                    <div className="q2d-row">
                        <span className="q2d-twisty q2d-leaf" />
                        <span className="q2d-tag">meta</span>
                        <JsonValue value={ast.meta} />
                    </div>
                    <Blocks nodes={ast.blocks} args={blocksArgs} update={(n) => ({ ...ast, blocks: n }) as unknown as AnyNode} />
                </div>
              </OptionsContext.Provider>
                <label className="q2d-corner" title="show source refs (pool index and line:col) on every node">
                    <input type="checkbox" checked={refs} onChange={(e) => setRefs(e.target.checked)} /> refs
                </label>
                {attr.overlay}
            </div>
        </>
    );
};
