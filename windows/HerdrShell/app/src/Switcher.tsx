import { useMemo, useState, useEffect, useRef } from "react";
import type { SidebarRow } from "./model";
import { Status } from "./Sidebar";
export default function Switcher({ presentation = 0, query: savedQuery, changeQuery, selectQuery = false, rows, selected, machine, open, close }: { presentation?: number; query?: string; changeQuery?: (query: string) => void; selectQuery?: boolean; rows: SidebarRow[]; selected: string | null; machine: string; open: (id: string) => void; close: () => void }) {
  // As the Mac quick switcher: each row names its space and machine.
  const where = (row: SidebarRow) => [row.spaceLabel, machine.charAt(0).toUpperCase() + machine.slice(1)].filter(Boolean).join(" · ");
  const [localQuery, setLocalQuery] = useState("");
  const query = savedQuery ?? localQuery;
  const setQuery = changeQuery ?? setLocalQuery;
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { if (selectQuery) input.current?.select(); else input.current?.setSelectionRange(query.length, query.length); }, [selectQuery, presentation]);
  const [index, setIndex] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const results = useMemo(() => {
    const seen = new Set<string>();
    return rows.filter(r => {
      if (r.kind === "space" || seen.has(r.id)) return false;
      seen.add(r.id);
      return `${r.label} ${r.spaceLabel ?? ""}`.toLowerCase().includes(query.toLowerCase());
    });
  }, [rows, query]);
  const active = Math.min(index, Math.max(0, results.length - 1));
  useEffect(() => { list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" }); }, [active, results]);
  const pick = (id: string) => { open(id); close(); };
  return <div className="switcher-backdrop" onMouseDown={event => { if (event.target === event.currentTarget) close(); }}>
    <section className="switcher" role="dialog" aria-modal="true" aria-label="Switch tab" onKeyDown={event => {
      if (event.key === "Escape") { event.preventDefault(); close(); }
      else if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); setIndex(results.length ? (active + (event.key === "ArrowDown" ? 1 : -1) + results.length) % results.length : 0); }
      else if (event.key === "Enter") { event.preventDefault(); if (results[active]) pick(results[active].id); }
      else if (event.key === "Tab") { event.preventDefault(); }
      event.stopPropagation();
    }}>
      <label className="switcher-query"><svg className="switcher-glyph" viewBox="0 0 16 16" aria-hidden="true"><circle cx="6.5" cy="6.5" r="4.75" /><path d="M10 10l4 4" /></svg><input ref={input} autoFocus aria-label="Filter tabs" placeholder="Find a tab…" value={query} onChange={event => { setQuery(event.target.value); setIndex(0); }} aria-controls="switcher-results" aria-activedescendant={results[active] ? `switcher-${active}` : undefined} /></label>
      <div id="switcher-results" role="listbox" ref={list}>{results.map((row, i) => <button id={`switcher-${i}`} role="option" aria-selected={active === i} key={row.id} className={`sidebar-row ${active === i ? "selected" : ""}`} onMouseEnter={() => setIndex(i)} onClick={() => pick(row.id)}>
        <Status status={row.status} /><span className="label">{row.label}</span><span className="muted">{where(row)}</span>{selected === row.id && <span className="muted" aria-label="Current tab">✓</span>}
      </button>)}{!results.length && <div className="muted empty-results">No matching tabs</div>}</div>
    </section>
  </div>;
}
