/**
 * The dashboard view nodes (`crates/quarto-core/src/dashboard/view.rs`).
 * Each renders its element with the render's attributes and adds the
 * behavior bslib's / Bootstrap's JavaScript gives the render:
 *
 * - `DashboardPageView`: the page pane, active per the navbar tabs.
 * - `DashboardCardView`: a bslib card with full screen (`data-full-screen`,
 *   the overlay and its Close link, `body.bslib-has-full-screen`).
 * - `DashboardTabNav` / `DashboardTabPane`: a tabset card's tabs.
 * - `DashboardSidebarLayout`: a bslib sidebar layout with collapse
 *   (`sidebar-collapsed`, the aside's `hidden`, the toggle's
 *   `aria-expanded`).
 */
import { createElement, useContext, useEffect, useState } from 'react';
import type { CustomBlockNode, NodeArgs } from '../../../framework';
import { makeSlotSetter, renderSlot } from '../../utils';
import {
    attrProps,
    CardTabsContext,
    DashboardPagesContext,
    isActivePane,
    stateClassName,
    withClasses,
} from './shared';

/** Plain-data keys per node (writer: `dashboard/view.rs`). */
export const DASHBOARD_PAGE_VIEW_PLAIN_DATA_KEYS = ['id'] as const;
export const DASHBOARD_CARD_VIEW_PLAIN_DATA_KEYS = ['expandable'] as const;
export const DASHBOARD_TAB_NAV_PLAIN_DATA_KEYS = ['tabset_id', 'tabs'] as const;
export const DASHBOARD_TAB_PANE_PLAIN_DATA_KEYS = ['id'] as const;
export const DASHBOARD_SIDEBAR_LAYOUT_PLAIN_DATA_KEYS = ['sidebar_id'] as const;
export const DASHBOARD_PARAGRAPH_PLAIN_DATA_KEYS = [] as const;

type Plain = Record<string, unknown>;

function plainOf(node: CustomBlockNode): Plain {
    return (node.plain_data ?? {}) as Plain;
}

export const DashboardPageView = ({ node, setLocalAst, onNavigateToDocument }: NodeArgs<CustomBlockNode>) => {
    const pages = useContext(DashboardPagesContext);
    const props = attrProps(node.attr);
    const id = node.attr[0];
    // Without a navbar choice yet, the render's initial state (the first
    // page carries `active`) decides.
    const active = pages.active === null ? node.attr[1].includes('active') : pages.active === id;
    const setSlot = makeSlotSetter(node, setLocalAst);
    return (
        <div {...props} className={stateClassName(node.attr[1], active)}>
            {renderSlot(node.slots.content, setSlot('content'), { onNavigateToDocument })}
        </div>
    );
};

const EXPAND_ICON = (
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" style={{ height: '1em', width: '1em' }} aria-hidden="true" role="img">
        <path d="M20 5C20 4.4 19.6 4 19 4H13C12.4 4 12 3.6 12 3C12 2.4 12.4 2 13 2H21C21.6 2 22 2.4 22 3V11C22 11.6 21.6 12 21 12C20.4 12 20 11.6 20 11V5ZM4 19C4 19.6 4.4 20 5 20H11C11.6 20 12 20.4 12 21C12 21.6 11.6 22 11 22H3C2.4 22 2 21.6 2 21V13C2 12.4 2.4 12 3 12C3.6 12 4 12.4 4 13V19Z" />
    </svg>
);

const CLOSE_ICON = (
    <svg width="20" height="20" fill="currentColor" className="bi bi-x-lg" viewBox="0 0 16 16">
        <path d="M2.146 2.854a.5.5 0 1 1 .708-.708L8 7.293l5.146-5.147a.5.5 0 0 1 .708.708L8.707 8l5.147 5.146a.5.5 0 0 1-.708.708L8 8.707l-5.146 5.147a.5.5 0 0 1-.708-.708L7.293 8 2.146 2.854Z" />
    </svg>
);

export const DashboardCardView = ({ node, setLocalAst, onNavigateToDocument }: NodeArgs<CustomBlockNode>) => {
    const plain = plainOf(node);
    const expandable = plain.expandable === true;
    const [fullScreen, setFullScreen] = useState(false);
    const [activeTab, setActiveTab] = useState<string | null>(null);

    useEffect(() => {
        if (!fullScreen) return;
        document.body.classList.add('bslib-has-full-screen');
        const onKey = (e: KeyboardEvent) => {
            if (e.key === 'Escape') setFullScreen(false);
        };
        document.addEventListener('keydown', onKey);
        return () => {
            document.body.classList.remove('bslib-has-full-screen');
            document.removeEventListener('keydown', onKey);
        };
    }, [fullScreen]);

    const props = attrProps(node.attr);
    if (expandable) props['data-full-screen'] = fullScreen ? 'true' : 'false';
    const setSlot = makeSlotSetter(node, setLocalAst);
    return (
        <CardTabsContext.Provider value={{ active: activeTab, setActive: setActiveTab }}>
            {fullScreen ? (
                <div id="bslib-full-screen-overlay" onClick={() => setFullScreen(false)}>
                    <a className="bslib-full-screen-exit" tabIndex={0} onClick={() => setFullScreen(false)}>
                        Close {CLOSE_ICON}
                    </a>
                </div>
            ) : null}
            <div {...props}>
                {renderSlot(node.slots.content, setSlot('content'), { onNavigateToDocument })}
                {expandable
                    ? // bslib shows the control through `.card:hover > * >
                      // .bslib-full-screen-enter`: it needs a host element, and
                      // the render's is `<bslib-tooltip>` (inert here, as bslib's
                      // web components are not loaded in the preview).
                      createElement(
                          'bslib-tooltip',
                          null,
                          <span
                              className="bslib-full-screen-enter badge rounded-pill"
                              title="Expand"
                              role="button"
                              onClick={() => setFullScreen(true)}
                          >
                              {EXPAND_ICON}
                          </span>,
                      )
                    : null}
            </div>
        </CardTabsContext.Provider>
    );
};

type Tab = { id: string; title: string };

export const DashboardTabNav = ({ node }: NodeArgs<CustomBlockNode>) => {
    const plain = plainOf(node);
    const tabsetId = String(plain.tabset_id ?? '');
    const tabs = (Array.isArray(plain.tabs) ? plain.tabs : []) as Tab[];
    const card = useContext(CardTabsContext);
    const active = card?.active ?? null;
    return (
        <ul className="nav nav-tabs card-header-tabs" role="tablist" data-tabsetid={tabsetId}>
            {tabs.map((tab) => {
                const on = isActivePane(tab.id, active);
                return (
                    <li className="nav-item" role="presentation" key={tab.id}>
                        <a
                            href={`#${tab.id}`}
                            role="tab"
                            data-value={tab.title}
                            aria-selected={on ? 'true' : 'false'}
                            className={on ? 'nav-link active' : 'nav-link'}
                            onClick={(e) => {
                                e.preventDefault();
                                card?.setActive(tab.id);
                            }}
                        >
                            {tab.title}
                        </a>
                    </li>
                );
            })}
        </ul>
    );
};

export const DashboardTabPane = ({ node, setLocalAst, onNavigateToDocument }: NodeArgs<CustomBlockNode>) => {
    const card = useContext(CardTabsContext);
    const id = node.attr[0];
    const on = isActivePane(id, card?.active ?? null);
    const props = attrProps(node.attr);
    const setSlot = makeSlotSetter(node, setLocalAst);
    return (
        <div {...props} className={stateClassName(node.attr[1], on)}>
            {renderSlot(node.slots.content, setSlot('content'), { onNavigateToDocument })}
        </div>
    );
};

/** A value box\'s title or value: a classed `<p>` (`view.rs` `PARAGRAPH`). */
export const DashboardParagraph = ({ node, setLocalAst, onNavigateToDocument }: NodeArgs<CustomBlockNode>) => {
    const setSlot = makeSlotSetter(node, setLocalAst);
    return (
        <p {...attrProps(node.attr)}>
            {renderSlot(node.slots.content, setSlot('content'), { onNavigateToDocument })}
        </p>
    );
};

const CHEVRON = (
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" className="bi bi-chevron-left collapse-icon" style={{ fill: 'currentColor' }} aria-hidden="true" role="img">
        <path fillRule="evenodd" d="M11.354 1.646a.5.5 0 0 1 0 .708L5.707 8l5.647 5.646a.5.5 0 0 1-.708.708l-6-6a.5.5 0 0 1 0-.708l6-6a.5.5 0 0 1 .708 0z" />
    </svg>
);

export const DashboardSidebarLayout = ({ node, setLocalAst, onNavigateToDocument }: NodeArgs<CustomBlockNode>) => {
    const plain = plainOf(node);
    const sidebarId = String(plain.sidebar_id ?? '');
    const [open, setOpen] = useState(true);
    const props = attrProps(node.attr);
    const setSlot = makeSlotSetter(node, setLocalAst);
    const ctx = { onNavigateToDocument };
    return (
        <div {...props} className={withClasses(props, !open && 'sidebar-collapsed')}>
            {renderSlot(node.slots.main, setSlot('main'), ctx)}
            <aside id={sidebarId} className="sidebar html-fill-container html-fill-item" hidden={!open}>
                {renderSlot(node.slots.sidebar, setSlot('sidebar'), ctx)}
            </aside>
            <button
                className="collapse-toggle"
                type="button"
                title="Toggle sidebar"
                aria-expanded={open ? 'true' : 'false'}
                aria-controls={sidebarId}
                onClick={() => setOpen((o) => !o)}
            >
                {CHEVRON}
            </button>
        </div>
    );
};
