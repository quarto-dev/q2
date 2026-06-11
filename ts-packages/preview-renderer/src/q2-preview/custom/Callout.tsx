import { useContext, useId } from 'react';
import type {
    CustomBlockNode,
    NodeArgs,
    Slot,
} from '../../framework';
import { PreviewContext } from '../PreviewContext';
import {
    BS_ALIGN_CONTENT_CENTER,
    BS_COLLAPSE,
    BS_D_FLEX,
    BS_SHOW,
    CALLOUT,
    CALLOUT_BODY,
    CALLOUT_BODY_CONTAINER,
    CALLOUT_COLLAPSE,
    CALLOUT_EMPTY_CONTENT,
    CALLOUT_FLEX_FILL,
    CALLOUT_HEADER,
    CALLOUT_ICON,
    CALLOUT_ICON_CONTAINER,
    CALLOUT_STYLE_PREFIX,
    CALLOUT_TITLE_CONTAINER,
    CALLOUT_TITLED,
    CALLOUT_TYPE_PREFIX,
    NO_ICON,
    SCREEN_READER_ONLY,
} from '../quartoClasses';
import { makeSlotSetter, renderSlot } from '../utils';

/**
 * Callout — q2-preview port of `CalloutResolveTransform`'s HTML
 * structure (`crates/quarto-core/src/transforms/callout_resolve.rs`).
 *
 * `callout-resolve` is excluded from q2-preview's pipeline (see
 * `pipeline.rs::Q2_PREVIEW_TRANSFORM_EXCLUDED`), so the Rust side hands
 * us the `Callout` CustomNode wrapper unchanged. This component must
 * emit the same DOM that `callout_resolve.rs` would have produced —
 * Bootstrap's callout selectors target `.callout.callout-style-default
 * > .callout-header > .callout-title-container` etc.
 *
 * Two output shapes, mirroring `callout_resolve::build_titled_content`
 * and `build_untitled_content`:
 *
 *  Titled (user title OR appearance="default" + injected default title):
 *    .callout.callout-style-{appearance}.callout-{type}.callout-titled
 *      .callout-header.d-flex.align-content-center
 *        .callout-icon-container?    (when icon=true)
 *        .callout-title-container.flex-fill
 *      .callout-body-container.callout-body
 *
 *  Untitled (appearance="simple"/"minimal" + empty title):
 *    .callout.callout-style-{appearance}.callout-{type}
 *      .callout-body.d-flex
 *        .callout-icon-container?    (when icon=true)
 *        .callout-body-container
 *
 * `plain_data` (writer: `transforms/callout.rs`):
 *   - `type` (string): `note | warning | tip | important | caution`.
 *   - `appearance` (string): `default | simple | minimal` — minimal
 *     is normalized to `simple` + `icon=false` here, mirroring the
 *     Rust resolver's defense-in-depth (`callout_resolve.rs`).
 *   - `collapse` (bool): true → collapsible callout. Renders the
 *     `.callout-collapse.collapse[.show]` wrapper. NO interactive
 *     toggle in preview — see bd-???? follow-up.
 *   - `collapse_starts_collapsed` (bool): when `collapse=true`, true
 *     means start collapsed. Cosmetic in preview (no toggle).
 *   - `icon` (bool): controls `.callout-icon-container` subtree.
 */

interface CalloutPlainData {
    type?: string;
    appearance?: string;
    collapse?: boolean;
    collapse_starts_collapsed?: boolean;
    icon?: boolean;
    // Set by CalloutTransform when the user's id (e.g. `tip-foo`)
    // classifies as a crossref-eligible callout (`callout.rs:236-241`).
    // Presence suppresses the screen-reader callout-type span — the
    // crossref-rendered prefix announces the type on its own.
    ref_type?: string;
}

const DEFAULT_TITLES: Record<string, string> = {
    note: 'Note',
    warning: 'Warning',
    tip: 'Tip',
    important: 'Important',
    caution: 'Caution',
};

function defaultTitle(calloutType: string): string {
    if (DEFAULT_TITLES[calloutType]) return DEFAULT_TITLES[calloutType];
    // Fallback for forward-compat callout types: ASCII-uppercase first
    // byte (mirrors Rust capitalize).
    if (!calloutType) return '';
    return calloutType[0].toUpperCase() + calloutType.slice(1);
}

/**
 * Mirror Rust's `extract_user_title` + emptiness check: only the
 * literally-empty Inlines slot is treated as "no user title".
 * A whitespace-only title still counts as titled.
 */
function hasUserTitle(titleSlot: Slot | undefined): boolean {
    if (!titleSlot) return false;
    if (titleSlot.kind === 'inlines' && titleSlot.value.length === 0) return false;
    return true;
}

export const Callout = ({ node, onNavigateToDocument, setLocalAst }: NodeArgs<CustomBlockNode>) => {
    const previewCtx = useContext(PreviewContext);
    const poolId = (node as any).s as string | number | undefined;
    const resolved = previewCtx?.resolveSource ? previewCtx.resolveSource(node) : null;
    const isEditable = resolved != null && resolved.reachabilityClass !== 'Opaque' && poolId !== undefined;
    const affordanceAttr = isEditable ? { 'data-block-pool-id': poolId, tabIndex: -1 } : {};

    // Q1-parity collapse-wrapper id naming (callouts.lua:281, 307,
    // 310, 316-317): the wrapper's `id` attribute is `callout-N`
    // (the bare counter); its `callout-N-contents` class is what
    // Bootstrap's `bs-target` references via the class selector.
    // `useId()` gives us a stable unique id per component instance.
    // We strip the React-internal punctuation so the resulting id
    // looks like `callout-r0-contents` rather than `callout-:r0:-contents`.
    const reactId = useId().replace(/[^a-z0-9]/gi, '');
    const calloutId = `callout-${reactId}`;
    const contentsClass = `${calloutId}-contents`;

    const plain = (node.plain_data ?? {}) as CalloutPlainData;
    const calloutType = plain.type ?? 'note';
    const rawAppearance = plain.appearance ?? 'default';
    const rawIcon = plain.icon !== false; // undefined defaults to icon-on
    // Defense-in-depth normalization: minimal → simple + icon=false.
    const appearance = rawAppearance === 'minimal' ? 'simple' : rawAppearance;
    const icon = rawAppearance === 'minimal' ? false : rawIcon;
    const collapse = plain.collapse === true;
    const startsCollapsed = collapse && plain.collapse_starts_collapsed === true;

    const titleSlot = node.slots.title;
    const userTitled = hasUserTitle(titleSlot);
    // Default-title injection: `appearance="default"` + no user title →
    // inject display name (then the titled path is taken). For non-default
    // appearances, an empty title means untitled.
    const isTitled = userTitled || appearance === 'default';

    const contentSlot = node.slots.content;
    const isEmptyContent = !contentSlot
        || (contentSlot.kind === 'blocks' && contentSlot.value.length === 0);

    const classList = [
        CALLOUT,
        `${CALLOUT_STYLE_PREFIX}${appearance}`,
        `${CALLOUT_TYPE_PREFIX}${calloutType}`,
    ];
    if (!icon) classList.push(NO_ICON);
    if (isTitled) classList.push(CALLOUT_TITLED);
    if (isEmptyContent) classList.push(CALLOUT_EMPTY_CONTENT);

    const id = node.attr[0];

    const setSlot = makeSlotSetter(node, setLocalAst);
    const ctx = { onNavigateToDocument };

    // Q1-parity (callouts.lua:254-262): icon container is ALWAYS
    // emitted. When `icon=false`, the inner <i> carries an
    // additional `no-icon` co-class which CSS uses to hide it
    // visually via `.callout.no-icon`. Keeps DOM counts of
    // `.callout-icon-container` stable across icon/no-icon callouts.
    const iconContainer = (
        <div className={CALLOUT_ICON_CONTAINER}>
            <i className={icon ? CALLOUT_ICON : `${CALLOUT_ICON} ${NO_ICON}`}></i>
        </div>
    );

    const bodyContents = renderSlot(contentSlot, setSlot('content'), ctx);

    if (isTitled) {
        // Accessibility (callouts.lua:271-275): when the title is
        // user-supplied AND the callout isn't crossref-eligible,
        // prepend a screen-reader-only Span carrying the type's
        // display name so screen readers announce e.g. "Warning:
        // Watch Out" instead of just "Watch Out". Default-injected
        // titles already ARE the display name; crossref-rendered
        // titles already announce the type via the rendered prefix.
        const isCrossref = !!plain.ref_type;
        const userTitleNode = userTitled
            ? renderSlot(titleSlot, setSlot('title'), ctx)
            : defaultTitle(calloutType);
        const titleNode = (userTitled && !isCrossref) ? (
            <>
                <span className={SCREEN_READER_ONLY}>{defaultTitle(calloutType)}</span>
                {userTitleNode}
            </>
        ) : userTitleNode;

        const headerClass = [CALLOUT_HEADER, BS_D_FLEX, BS_ALIGN_CONTENT_CENTER].join(' ');
        const bodyContainerClass = [CALLOUT_BODY_CONTAINER, CALLOUT_BODY].join(' ');

        // Collapse wrapper: visual only in preview (no interactive toggle).
        // The `show` class controls whether the body is visible — we
        // honor `startsCollapsed` cosmetically.
        const bodyDiv = (
            <div className={bodyContainerClass}>
                {bodyContents}
            </div>
        );
        const wrappedBody = collapse ? (
            // id="callout-N" + class="callout-N-contents callout-collapse
            // collapse [show]" — mirrors Rust resolver and TS Quarto.
            <div
                id={calloutId}
                className={[contentsClass, CALLOUT_COLLAPSE, BS_COLLAPSE, !startsCollapsed ? BS_SHOW : null]
                    .filter(Boolean)
                    .join(' ')}
            >
                {bodyDiv}
            </div>
        ) : bodyDiv;

        return (
            <div className={classList.join(' ')} id={id || undefined} {...affordanceAttr}>
                <div className={headerClass}>
                    {iconContainer}
                    <div className={`${CALLOUT_TITLE_CONTAINER} ${CALLOUT_FLEX_FILL}`}>
                        {titleNode}
                    </div>
                </div>
                {wrappedBody}
            </div>
        );
    }

    // Untitled path: single `.callout-body.d-flex` wrapping icon + body-container.
    return (
        <div className={classList.join(' ')} id={id || undefined} {...affordanceAttr}>
            <div className={`${CALLOUT_BODY} ${BS_D_FLEX}`}>
                {iconContainer}
                <div className={CALLOUT_BODY_CONTAINER}>
                    {bodyContents}
                </div>
            </div>
        </div>
    );
};
