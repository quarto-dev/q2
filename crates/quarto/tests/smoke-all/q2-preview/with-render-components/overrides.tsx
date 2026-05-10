// User render-components TSX exports for the q2-preview override
// smoke fixture (Plan 2C Phase 5.2). Two components — one Pandoc-tag
// override (`Para`), one CustomNode override (`Callout`). Both layer
// onto `previewRegistry` via `mergedPreviewRegistry` in entry.tsx.
//
// The fixture's ensureHtmlElements asserts both fire simultaneously,
// locking the unified-merge mechanism (single spread, single context)
// at the integration layer.

const React = (window as any).React;

export function Para(args: any) {
    return React.createElement(
        'p',
        { className: 'my-para' },
        args.node.c.map((inl: any, i: number) =>
            React.createElement('span', { key: i }, inl?.c ?? ''),
        ),
    );
}

export function Callout() {
    return React.createElement(
        'div',
        { className: 'my-callout' },
        'overridden',
    );
}
