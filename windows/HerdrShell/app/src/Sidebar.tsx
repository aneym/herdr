import { useState } from "react";
import type { MachineStatus } from "./bridge";
import type { SidebarRow } from "./model";
export function Status({ status }: { status: string }) { return <span className={`status ${status}`} aria-label={status}>{status === "blocked" ? "■" : "●"}</span>; }
function RenameInput({ label, commit, cancel }: { label: string; commit: (label: string) => Promise<void>; cancel: () => void }) {
  const [value, setValue] = useState(label);
  const [saving, setSaving] = useState(false);
  return <input className="rename-input" autoFocus aria-label="Rename tab" value={value} readOnly={saving} onFocus={event => event.target.select()} onChange={event => setValue(event.target.value)} onKeyDown={event => {
    event.stopPropagation();
    if (event.key === "Escape") { event.preventDefault(); cancel(); }
    if (event.key === "Enter" && !saving) { event.preventDefault(); setSaving(true); void commit(value).finally(() => setSaving(false)); }
  }} />;
}
export default function Sidebar({ rows, selected, machine, select, pin, renaming, startRename, cancelRename, commitRename }: { rows: SidebarRow[]; selected: string | null; machine: MachineStatus; select: (id: string) => void; pin: (id: string, pinned: boolean) => void; renaming: string | null; startRename: (id: string) => void; cancelRename: () => void; commitRename: (id: string, label: string) => Promise<void> }) {
  const [expanded, setExpanded] = useState<Record<string, boolean>>(() => { try { return JSON.parse(localStorage.getItem("herdr-space-expanded") || "{}"); } catch { return {}; } });
  const [hidden, setHidden] = useState(false);
  const toggle = (id: string, value: boolean) => setExpanded(previous => { const next = { ...previous, [id]: value }; try { localStorage.setItem("herdr-space-expanded", JSON.stringify(next)); } catch { /* Storage can be disabled by WebView policy. */ } return next; });
  const tabRow = (row: SidebarRow) => <div key={`${row.kind}:${row.id}`} className={`sidebar-row tab-row ${row.kind === "tab" ? "indented" : ""} ${selected === row.id ? "selected" : ""}`}>
    {renaming === row.id && rows.find(r => r.kind !== "space" && r.id === row.id) === row ? <RenameInput key={row.id} label={row.label} commit={label => commitRename(row.id, label)} cancel={cancelRename} /> : <button className="select-tab" onClick={() => select(row.id)} onDoubleClick={() => startRename(row.id)}><Status status={row.status} /><span className="label">{row.label}</span>{row.kind !== "tab" && <span className="muted space-label">{row.spaceLabel}</span>}{row.hotkey && <span className="muted hotkey">Ctrl+{row.hotkey}</span>}</button>}
    <button className={`pin ${row.pinned ? "is-pinned" : ""}`} aria-label={row.pinned ? "Unpin tab" : "Pin tab"} onClick={() => pin(row.id, !row.pinned)}>⌖</button>
  </div>;
  const spaceRow = (row: SidebarRow) => {
    const children = rows.filter(r => r.kind === "tab" && r.section === row.id);
    const open = children.some(r => r.id === renaming) || (expanded[row.id] ?? (rows.some(r => r.id === selected && r.spaceId === row.id) || row.status === "working" || row.status === "blocked" || row.status === "done"));
    return <div key={row.id}><button className="sidebar-row space-row" aria-expanded={open} onClick={() => toggle(row.id, !open)}><span className="chevron">{open ? "⌄" : "›"}</span><span className="label">{row.label}</span><Status status={row.status} /></button>{open && children.map(tabRow)}</div>;
  };
  return <aside className="sidebar"><nav>
    {["AGENTS", "PINNED"].map(section => { const items = rows.filter(r => r.section === section); return items.length ? <section key={section}><h2>{section}</h2>{items.map(tabRow)}</section> : null; })}
    <section className="spaces">{rows.filter(r => r.kind === "space" && !r.hidden).map(spaceRow)}
    {rows.some(r => r.kind === "space" && r.hidden) && <><button className="sidebar-row muted" aria-expanded={hidden} onClick={() => setHidden(!hidden)}><span className="chevron">{hidden ? "⌄" : "›"}</span>Hidden</button>{(hidden || rows.some(r => r.hidden && r.id === renaming)) && rows.filter(r => r.kind === "space" && r.hidden).map(spaceRow)}</>}
    </section>
  </nav><footer>{machine.state === "up" ? `${machine.name} · connected` : machine.state === "connecting" ? "connecting…" : `offline: ${machine.error || "disconnected"}`}</footer></aside>;
}
