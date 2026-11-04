/**
 * The dashboard page shell for the preview: React's counterpart of
 * `DASHBOARD_HTML_TEMPLATE` and its `title-block` / `_nav-container` /
 * `_nav-buttons` partials (`crates/quarto-core/src/template.rs`).
 *
 * `PreviewRoot` swaps it in for `PreviewDocument` (the registry's `Ast`
 * entry) when the format is `q2-dashboard`. It reads the navbar metadata the
 * `DashboardTransform` writes (`rendered.dashboard.{pages, logo,
 * nav-buttons, body-classes, content-classes}`) and owns the active page:
 * from the URL hash when it names a page (Q1 #14818 / #9411), else the
 * first, updated by the page tabs.
 *
 * In a website (dashboard plan Phase 9) the site's header wraps the
 * dashboard header, the content area is the site's `#quarto-content`
 * (the skip link's target), and the site footer follows, as in
 * `DASHBOARD_SITE_HEADER_PARTIAL` and the dashboard template.
 */
import { useEffect, useMemo, useState } from 'react';
import {
    extractMetaBool,
    extractMetaString,
    extractMetaStringList,
    getMetaPath,
    renderChildren,
} from '../../../framework';
import type { PandocAST } from '../../../framework';
import {
    AnnouncementSlot,
    FooterSlot,
    HeaderIncludesEffect,
    NavbarSlot,
    SkipLinkSlot,
} from '../../chromeSlots';
import { appendColorModeClass } from '../../PreviewDocument';
import { DashboardPagesContext } from './shared';

type MetaValue = { t?: string; c?: unknown } | undefined;

function metaList(v: unknown): unknown[] {
    const m = v as MetaValue;
    return m?.t === 'MetaList' && Array.isArray(m.c) ? m.c : [];
}

function field(map: unknown, key: string): unknown {
    const m = map as MetaValue;
    return m?.t === 'MetaMap' && m.c && typeof m.c === 'object'
        ? (m.c as Record<string, unknown>)[key]
        : undefined;
}

function str(map: unknown, key: string): string | undefined {
    return extractMetaString(field(map, key));
}

type Page = { id: string; text: string; active: boolean; scrolling: string };
type NavButton = {
    href: string;
    text?: string;
    icon?: string;
    rel?: string;
    target?: string;
    title?: string;
    ariaLabel?: string;
};

function readPages(meta: Record<string, unknown>): Page[] {
    return metaList(getMetaPath(meta, ['rendered', 'dashboard', 'pages'])).map((p) => ({
        id: str(p, 'id') ?? '',
        text: str(p, 'text') ?? '',
        active: extractMetaBool(field(p, 'active')) === true,
        scrolling: str(p, 'scrolling') ?? 'false',
    }));
}

function readNavButtons(meta: Record<string, unknown>): NavButton[] {
    return metaList(getMetaPath(meta, ['rendered', 'dashboard', 'nav-buttons'])).map((b) => ({
        href: str(b, 'href') ?? '#',
        text: str(b, 'text'),
        icon: str(b, 'icon'),
        rel: str(b, 'rel'),
        target: str(b, 'target'),
        title: str(b, 'title'),
        ariaLabel: str(b, 'aria-label'),
    }));
}

/** The author line: `by-author` names (the template's `it.name.literal`),
 *  else the `author` strings. */
function readAuthors(meta: Record<string, unknown>): string[] {
    const byAuthor = metaList(meta['by-author'])
        .map((a) => extractMetaString(field(field(a, 'name'), 'literal')) ?? str(a, 'name'))
        .filter((s): s is string => !!s);
    if (byAuthor.length) return byAuthor;
    const author = meta.author;
    const single = extractMetaString(author);
    return single ? [single] : extractMetaStringList(author);
}

/** The page whose id the URL hash names, if any. */
function pageFromHash(pages: Page[]): string | null {
    if (typeof window === 'undefined') return null;
    const hash = decodeURIComponent(window.location.hash.replace(/^#/, ''));
    return pages.some((p) => p.id === hash) ? hash : null;
}

function NavButtons({ buttons }: { buttons: NavButton[] }) {
    if (buttons.length === 0) return null;
    // quarto-dashboard.js fills the `|url|` share placeholder.
    const here = typeof window === 'undefined' ? '' : window.location.href;
    return (
        <div className="quarto-dashboard-links">
            {buttons.map((b, i) => (
                <a
                    key={i}
                    className="quarto-dashboard-link"
                    href={b.href.replace('|url|', here)}
                    rel={b.rel}
                    title={b.title}
                    target={b.target}
                    aria-label={b.ariaLabel}
                >
                    {b.icon ? <i className={`bi bi-${b.icon}`}></i> : null}
                    {b.text ? <span className="quarto-dashboard-link-text">{b.text}</span> : null}
                </a>
            ))}
        </div>
    );
}

export const DashboardDocument = ({
    ast,
    onNavigateToDocument,
    setAst,
}: {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}) => {
    const meta = (ast.meta ?? {}) as Record<string, unknown>;
    const pages = useMemo(() => readPages(meta), [meta]);
    const buttons = readNavButtons(meta);
    const title = extractMetaString(meta.title);
    const subtitle = extractMetaString(meta.subtitle);
    const authors = readAuthors(meta);
    const logo = getMetaPath(meta, ['rendered', 'dashboard', 'logo']);
    const scrolling = extractMetaBool(meta.scrolling) === true;
    const skipText =
        extractMetaString(getMetaPath(meta, ['quarto', 'language', 'skip-to-content'])) ??
        'Skip to main content';

    const [active, setActiveState] = useState<string | null>(() => pageFromHash(pages));
    const setActive = (id: string) => {
        setActiveState(id);
        try {
            window.history.replaceState(null, '', `#${encodeURIComponent(id)}`);
        } catch {
            // A sandboxed frame may refuse; the tab state still changes.
        }
    };
    // Follow back/forward and in-page `#page` links.
    useEffect(() => {
        const onHash = () => {
            const id = pageFromHash(pages);
            if (id) setActiveState(id);
        };
        window.addEventListener('hashchange', onHash);
        return () => window.removeEventListener('hashchange', onHash);
    }, [pages]);
    const activeId = active ?? pages.find((p) => p.active)?.id ?? pages[0]?.id ?? null;

    // Site chrome (`rendered.navigation.*`, as `PreviewDocument` reads it).
    const nav = (key: string) => extractMetaString(getMetaPath(meta, ['rendered', 'navigation', key]));
    const navbarHtml = nav('navbar');
    const announceAbove = nav('announcement-above');
    const announceBelow = nav('announcement-below');
    const footerHtml = nav('footer');
    const inSite = !!(navbarHtml || announceAbove || announceBelow);
    const pinned =
        extractMetaBool(getMetaPath(meta, ['navigation', 'navbar', 'pinned'])) === true ||
        extractMetaBool(getMetaPath(meta, ['navigation', 'sidebar', 'pinned'])) === true;

    // <body> classes, as the template composes them: the dashboard classes,
    // then `$body-classes$` (a user `body-classes` wins wholesale; else the
    // site's classes, `fullcontent`, and `nav-fixed` under a navbar, as in
    // `PreviewDocument`), then the lowering's.
    const userBodyClasses = extractMetaString(meta['body-classes']);
    const structural =
        userBodyClasses ??
        [nav('body-classes'), 'fullcontent', navbarHtml ? 'nav-fixed' : null].filter(Boolean).join(' ');
    const bodyClasses = appendColorModeClass(
        [
            'quarto-dashboard',
            scrolling ? null : 'dashboard-fill',
            structural,
            extractMetaString(getMetaPath(meta, ['rendered', 'dashboard', 'body-classes'])),
        ]
            .filter(Boolean)
            .join(' '),
    );
    useEffect(() => {
        const previous = document.body.className;
        document.body.className = bodyClasses;
        return () => {
            document.body.className = previous;
        };
    }, [bodyClasses]);
    useEffect(() => {
        const next = extractMetaString(meta.pagetitle) ?? title;
        if (!next) return;
        const previous = document.title;
        document.title = next;
        return () => {
            document.title = previous;
        };
    }, [meta, title]);

    const contentClasses = [
        navbarHtml ? 'quarto-container page-navbar' : null,
        'page-layout-custom quarto-dashboard-content bslib-gap-spacing html-fill-container',
        scrolling ? 'dashboard-scrolling' : 'bslib-page-fill',
        pages.length ? 'quarto-dashboard-pages' : null,
        extractMetaString(getMetaPath(meta, ['rendered', 'dashboard', 'content-classes'])),
    ]
        .filter(Boolean)
        .join(' ');

    const children = renderChildren({
        node: ast as any,
        setLocalAst: setAst as any,
        onNavigateToDocument,
    });

    const hasBrand = !!(title || logo || subtitle || authors.length);
    const logoImg = (mode: 'light' | 'dark') => {
        const v = field(logo, mode);
        const path = str(v, 'path');
        return path ? (
            <a href="#">
                <img src={path} alt={str(v, 'alt') ?? ''} className={`navbar-logo ${mode}-content d-inline-block`} />
            </a>
        ) : null;
    };

    const dashboardHeader = (
            <header id="quarto-dashboard-header">
                <nav className="navbar navbar-expand-md slim" data-bs-theme="dark">
                    <div className="navbar-container container-fluid">
                        <button
                            className={pages.length ? 'navbar-toggler' : 'navbar-toggler hidden'}
                            type="button"
                            data-bs-toggle="collapse"
                            data-bs-target="#dashboard-collapse"
                            aria-controls="dashboard-collapse"
                            aria-expanded="false"
                            aria-label="Toggle navigation"
                        >
                            <span className="navbar-toggler-icon"></span>
                        </button>
                        {hasBrand ? (
                            <div className="navbar-brand-container">
                                {logo ? logoImg('light') : null}
                                {logo ? logoImg('dark') : null}
                                <div className="navbar-title">
                                    {title ? (
                                        <div className="navbar-title-text">
                                            <a href="#">{title}</a>
                                        </div>
                                    ) : null}
                                    {subtitle ? <div className="navbar-subtitle">{subtitle}</div> : null}
                                    {authors.length ? (
                                        <div className="navbar-author">{authors.join(', ')}</div>
                                    ) : null}
                                </div>
                            </div>
                        ) : null}
                        {pages.length ? (
                            <div id="dashboard-collapse" className="navbar-collapse collapse">
                                <ul className="navbar-nav navbar-nav-scroll me-auto" role="tablist">
                                    {pages.map((p) => {
                                        const on = p.id === activeId;
                                        return (
                                            <li className="nav-item" role="presentation" key={p.id}>
                                                <a
                                                    id={`tab-${p.id}`}
                                                    className={on ? 'nav-link active' : 'nav-link'}
                                                    role="tab"
                                                    data-scrolling={p.scrolling}
                                                    href={`#${p.id}`}
                                                    aria-controls={p.id}
                                                    aria-selected={on ? 'true' : 'false'}
                                                    onClick={(e) => {
                                                        e.preventDefault();
                                                        setActive(p.id);
                                                    }}
                                                >
                                                    <span className="nav-link-text">{p.text}</span>
                                                </a>
                                            </li>
                                        );
                                    })}
                                </ul>
                                <NavButtons buttons={buttons} />
                            </div>
                        ) : (
                            <NavButtons buttons={buttons} />
                        )}
                    </div>
                </nav>
            </header>
    );

    return (
        <DashboardPagesContext.Provider value={{ active: activeId, setActive }}>
            {navbarHtml ? (
                <a id="quarto-skip-link" className="visually-hidden-focusable" href="#quarto-content">
                    {skipText}
                </a>
            ) : (
                <SkipLinkSlot text={skipText} />
            )}
            <HeaderIncludesEffect
                items={extractMetaStringList(getMetaPath(meta, ['rendered', 'includes', 'header']))}
            />
            {inSite ? (
                <header
                    id="quarto-header"
                    className="headroom fixed-top"
                    {...(pinned ? { 'data-headroom-pinned': 'true' } : {})}
                >
                    {announceAbove ? <AnnouncementSlot html={announceAbove} /> : null}
                    {navbarHtml ? <NavbarSlot html={navbarHtml} /> : null}
                    {dashboardHeader}
                    {announceBelow ? <AnnouncementSlot html={announceBelow} /> : null}
                </header>
            ) : (
                dashboardHeader
            )}
            <div
                className={contentClasses}
                id={navbarHtml ? 'quarto-content' : 'quarto-document-content'}
                tabIndex={-1}
                role="main"
            >
                {children}
            </div>
            {footerHtml ? <FooterSlot html={footerHtml} /> : null}
        </DashboardPagesContext.Provider>
    );
};
