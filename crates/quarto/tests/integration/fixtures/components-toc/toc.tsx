// A table of contents rendered by the author: `toc-component: toc.tsx`
// mounts this over `nav#TOC` with the structured TOC as `props` and the
// static markup as `children` (kept here, visually hidden, as the
// no-script fallback the runtime already captured).
import { useEffect, useState } from "react";
import type { QuartoComponentProps, QuartoTocEntry, QuartoTocProps } from "@quarto/components";

function flatten(entries: QuartoTocEntry[]): QuartoTocEntry[] {
  return entries.flatMap((e) => [e, ...flatten(e.children)]);
}

function currentId(entries: QuartoTocEntry[]): string | null {
  const margin = 200;
  let current: string | null = null;
  for (const e of flatten(entries)) {
    const el = document.getElementById(e.id);
    if (el && window.pageYOffset >= el.offsetTop - margin) current = e.id;
  }
  return current;
}

function List({ entries, active }: { entries: QuartoTocEntry[]; active: string | null }) {
  return (
    <ol>
      {entries.map((e) => (
        <li key={e.id} data-active={e.id === active ? "true" : undefined}>
          <a href={`#${e.id}`}>{e.title}</a>
          {e.children.length > 0 && <List entries={e.children} active={active} />}
        </li>
      ))}
    </ol>
  );
}

export default function Toc({ props, children }: QuartoComponentProps<QuartoTocProps>) {
  const [active, setActive] = useState<string | null>(null);
  useEffect(() => {
    const update = () => setActive(currentId(props.entries));
    update();
    window.addEventListener("scroll", update, { passive: true });
    return () => window.removeEventListener("scroll", update);
  }, [props.entries]);
  return (
    <div className="my-toc" data-entries={props.entries.length} data-location={props.location}>
      <h2>{props.title ?? "Contents"} (custom)</h2>
      <List entries={props.entries} active={active} />
      <div hidden>{children}</div>
    </div>
  );
}
