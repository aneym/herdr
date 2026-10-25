import { Fragment } from "react";
import type { ReactNode } from "react";
import type { SidebarRow } from "./model";

const sections = [
  ["NEEDS YOU", "NEEDS YOU"],
  ["WORKING", "WORKING"],
  ["FINISHED", "FINISHED"],
  ["UTILITIES", "UTILITIES"],
] as const;

function Chevron({ open }: { open: boolean }) {
  return <svg className={`disclosure-chevron${open ? " is-open" : ""}`} viewBox="0 0 8 8" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M2.5 1.5 5 4 2.5 6.5" /></svg>;
}

export default function HomeSections({ rows, selected, renderRow, folds, fold }: { rows: SidebarRow[]; selected: string | null; renderRow: (row: SidebarRow) => ReactNode; folds: Record<string, boolean>; fold: (key: string, open: boolean) => void }) {
  const block = (items: SidebarRow[]) => items.map((row, index) => {
    const previous = items[index - 1];
    const divider = previous?.homeTag?.class === "owner" && row.homeTag?.class === "lane";
    return <Fragment key={row.id}>{divider && <div className="home-class-divider" role="separator" />}{renderRow(row)}</Fragment>;
  });
  return <>{sections.map(([key, title]) => {
    const members = rows.filter(row => row.kind === "home" && row.section === key);
    const current = members.filter(row => !row.homeTag?.folded);
    const earlier = key === "FINISHED" ? members.filter(row => row.homeTag?.folded) : [];
    const open = folds["home:earlier"] ?? earlier.some(row => row.id === selected);
    return <section key={key} className="home-section"><h2 className="sidebar-section-title"><span>{title}</span><span className="space-section-count">{current.length}</span></h2>{block(current)}{earlier.length > 0 && <>
      <button className="sidebar-row muted home-earlier" aria-expanded={open} onClick={() => fold("home:earlier", !open)}><span className="chevron"><Chevron open={open} /></span><span>Earlier {earlier.length}</span></button>
      {open && block(earlier)}
    </>}</section>;
  })}</>;
}
