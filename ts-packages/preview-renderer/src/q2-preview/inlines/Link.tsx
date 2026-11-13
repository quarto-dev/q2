import type { MouseEvent } from 'react';
import { renderChildren } from '../../framework';
import type { LinkInline, NodeArgs } from '../../framework';

/**
 * A lightbox link (`a.lightbox`, added by the `lightbox` transform around
 * an image — bd-kcpkgftw) opens a GLightbox overlay in `q2 render`. The
 * preview does not load GLightbox, and the link's raw image href would
 * navigate the iframe away, so a click on it does nothing here.
 */
const LIGHTBOX_CLASS = 'lightbox';
const ignoreClick = (e: MouseEvent) => e.preventDefault();

export const Link = (args: NodeArgs<LinkInline>) => {
    const [[id, classes, kvs], , [url, title]] = args.node.c;
    const props: Record<string, string> = { href: url };
    if (id) props.id = id;
    if (classes.length) props.className = classes.join(' ');
    if (title) props.title = title;
    for (const [k, v] of kvs) {
        if (k.startsWith('data-') || k === 'rel' || k === 'target' || k === 'role') props[k] = v;
    }
    const onClick = classes.includes(LIGHTBOX_CLASS) ? ignoreClick : undefined;
    return (
        <a {...props} onClick={onClick}>
            {renderChildren(args)}
        </a>
    );
};
