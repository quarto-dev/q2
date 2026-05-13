import type {
    CustomBlockNode,
    NodeArgs,
    Slot,
} from '@quarto/preview-renderer/framework';
import {
    CALLOUT,
    CALLOUT_APPEARANCE_PREFIX,
    CALLOUT_BODY,
    CALLOUT_BODY_CONTAINER,
    CALLOUT_COLLAPSE,
    CALLOUT_FLEX_FILL,
    CALLOUT_HEADER,
    CALLOUT_ICON,
    CALLOUT_ICON_CONTAINER,
    CALLOUT_TITLE_CONTAINER,
    CALLOUT_TYPE_PREFIX,
} from '../quartoClasses';
import { makeSlotSetter, renderSlot } from '../utils';

/**
 * Callout — q2-preview port of `CalloutResolveTransform`'s HTML
 * structure (`crates/quarto-core/src/transforms/callout_resolve.rs`).
 *
 * `callout-resolve` is excluded from q2-preview's pipeline (see
 * `pipeline.rs:1050`'s `Q2_PREVIEW_TRANSFORM_EXCLUDED`), so the Rust
 * side hands us the `Callout` CustomNode wrapper unchanged. This
 * component must emit the same three-deep DOM that `callout_resolve.rs`
 * would have produced — Bootstrap's callout selectors target
 * `.callout > .callout-header > .callout-title-container` etc., so
 * flattening any level breaks theme CSS.
 *
 * `plain_data` (writer: `transforms/callout.rs:210`):
 *   - `type` (string): `note | warning | tip | important | caution`.
 *   - `appearance` (string): `default | simple | minimal`.
 *   - `collapse` (bool).
 *   - `icon` (bool): controls whether the `.callout-icon-container`
 *     subtree is emitted.
 */

interface CalloutPlainData {
    type?: string;
    appearance?: string;
    collapse?: boolean;
    icon?: boolean;
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
    // byte (matches Rust capitalize at callout_resolve.rs:304).
    if (!calloutType) return '';
    return calloutType[0].toUpperCase() + calloutType.slice(1);
}

/**
 * Mirror Rust's `inlines.is_empty()` check: only the literally-empty
 * Inlines slot is treated as "no title". A whitespace-only or
 * single-space title still wins over the default. `slots.title`
 * absent (no key in the map) also falls back to the default.
 */
function shouldUseDefaultTitle(titleSlot: Slot | undefined): boolean {
    if (!titleSlot) return true;
    if (titleSlot.kind === 'inlines' && titleSlot.value.length === 0) return true;
    return false;
}

export const Callout = ({ node, onNavigateToDocument, setLocalAst }: NodeArgs<CustomBlockNode>) => {
    const plain = (node.plain_data ?? {}) as CalloutPlainData;
    const calloutType = plain.type ?? 'note';
    const appearance = plain.appearance ?? 'default';
    const collapse = plain.collapse === true;
    const showIcon = plain.icon !== false; // undefined defaults to icon-on

    const classList = [CALLOUT, `${CALLOUT_TYPE_PREFIX}${calloutType}`];
    if (appearance !== 'default') {
        classList.push(`${CALLOUT_APPEARANCE_PREFIX}${appearance}`);
    }
    if (collapse) classList.push(CALLOUT_COLLAPSE);

    const id = node.attr[0];
    const setSlot = makeSlotSetter(node, setLocalAst);
    const ctx = { onNavigateToDocument };

    const titleSlot = node.slots.title;
    const titleNode = shouldUseDefaultTitle(titleSlot)
        ? defaultTitle(calloutType)
        : renderSlot(titleSlot, setSlot('title'), ctx);

    return (
        <div className={classList.join(' ')} id={id || undefined}>
            <div className={CALLOUT_HEADER}>
                {showIcon && (
                    <div className={CALLOUT_ICON_CONTAINER}>
                        <i className={CALLOUT_ICON}></i>
                    </div>
                )}
                <div className={`${CALLOUT_TITLE_CONTAINER} ${CALLOUT_FLEX_FILL}`}>
                    {titleNode}
                </div>
            </div>
            <div className={`${CALLOUT_BODY_CONTAINER} ${CALLOUT_BODY}`}>
                {renderSlot(node.slots.content, setSlot('content'), ctx)}
            </div>
        </div>
    );
};

