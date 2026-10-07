import { useEffect, useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent } from "react";
import UpdatePill from "./UpdatePill";
import AgentFace from "./AgentFace";
import { DRAG_THRESHOLD, slotAt } from "./pinDrag";
import type { PinSection, RowBox } from "./pinDrag";
import type { MachineStatus } from "./bridge";
import { foldKey, revealOnSelect, spaceOpen } from "./model";
import type { SidebarRow } from "./model";
export function Status({ status }: { status: string }) { return <span className={`status ${status}`} aria-label={status}>{status === "blocked" ? "■" : "●"}</span>; }
function RenameInput({ label, commit, cancel }: { label: string; commit: (label: string) => Promise<void>; cancel: () => void }) {
  const [value, setValue] = useState(label);
  const [saving, setSaving] = useState(false);
  return <input className="rename-input" autoFocus aria-label="Rename tab" value={value} readOnly={saving} onFocus={event => event.target.select()} onChange={event => setValue(event.target.value)} onBlur={() => { if (!saving) cancel(); }} onKeyDown={event => {
    event.stopPropagation();
    if (event.key === "Escape") { event.preventDefault(); cancel(); }
    if (event.key === "Enter" && !saving) { event.preventDefault(); setSaving(true); void commit(value).finally(() => setSaving(false)); }
  }} />;
}
interface Press { id: string; section: PinSection; x: number; y: number; ids: string[]; block: RowBox[]; active: boolean; cancelled: boolean; done: () => void }
export default function Sidebar({ rows, selected, revealed, machine, notice, select, pin, movePin, renaming, startRename, cancelRename, commitRename }: { rows: SidebarRow[]; selected: string | null; revealed: { current: string | null | undefined }; machine: MachineStatus; notice: string | null; select: (id: string) => void; pin: (id: string, pinned: boolean) => void; movePin: (ids: string[], from: number, to: number) => void; renaming: string | null; startRename: (id: string) => void; cancelRename: () => void; commitRename: (id: string, label: string) => Promise<void> }) {
  const [expanded, setExpanded] = useState<Record<string, boolean>>(() => { try { return JSON.parse(localStorage.getItem("herdr-space-expanded") || "{}"); } catch { return {}; } });
  const [hidden, setHidden] = useState(false);
  const toggle = (id: string, value: boolean) => setExpanded(previous => { const next = { ...previous, [id]: value }; try { localStorage.setItem("herdr-space-expanded", JSON.stringify(next)); } catch { /* Storage can be disabled by WebView policy. */ } return next; });
  // Drag an AGENTS or PINNED row within its section, as the Mac's PinDrag: the row follows the
  // pointer, a line marks the slot, a release on the section moves the pin, Esc or a release
  // off the section moves nothing, and a press that travels less than the threshold is a click.
  const reveal = useRef({ rows, expanded });
  reveal.current = { rows, expanded };
  // Only a change of selection reveals; folding the selected tab's space afterwards sticks, also
  // across hiding and showing the sidebar, since App keeps the last revealed selection.
  useEffect(() => { const key = revealOnSelect(revealed, reveal.current.rows, selected, reveal.current.expanded); if (key) toggle(key, true); }, [selected, revealed]);
  const nav = useRef<HTMLElement>(null);
  const press = useRef<Press | null>(null);
  const swallowClick = useRef(false);
  const [drag, setDrag] = useState<{ id: string; section: PinSection; travel: number; target: number | null } | null>(null);
  useEffect(() => () => press.current?.done(), []);
  const startPress = (event: ReactPointerEvent, row: SidebarRow) => {
    if (event.button !== 0 || (row.kind !== "agent" && row.kind !== "pinned") || renaming === row.id) return;
    press.current?.done();
    const section = row.kind;
    const sectionRows = () => [...nav.current?.querySelectorAll<HTMLElement>(`[data-pin-section="${section}"]`) ?? []];
    const elements = sectionRows();
    const block = elements.map(el => { const r = el.getBoundingClientRect(); return { top: r.top, bottom: r.bottom, left: r.left, right: r.right }; });
    const ids = elements.map(el => el.dataset.tab ?? "");
    const move = (e: PointerEvent) => {
      const p = press.current;
      if (!p || p.cancelled) return;
      if (!p.active) {
        if (Math.hypot(e.clientX - p.x, e.clientY - p.y) < DRAG_THRESHOLD) return;
        if (p.block.length < 2 || !p.ids.includes(p.id)) { p.done(); return; }
        p.active = true;
      }
      setDrag({ id: p.id, section: p.section, travel: e.clientY - p.y, target: slotAt(p.block, e.clientX, e.clientY) });
    };
    const up = (e: PointerEvent) => {
      const p = press.current;
      if (!p) return;
      p.done();
      if (!p.active) return;
      // The release still clicks the row under it; a drag is never a click.
      swallowClick.current = true;
      setTimeout(() => { swallowClick.current = false; }, 0);
      // A snapshot may have reordered the section mid-drag: the drop names slots in the order shown now.
      const ids = sectionRows().map(el => el.dataset.tab ?? "");
      const to = p.cancelled ? null : slotAt(p.block, e.clientX, e.clientY);
      const from = ids.indexOf(p.id);
      if (to !== null && from >= 0 && to !== from) movePin(ids, from, to);
    };
    const key = (e: KeyboardEvent) => {
      const p = press.current;
      if (e.key !== "Escape" || !p?.active || p.cancelled) return;
      e.preventDefault(); e.stopPropagation();
      p.cancelled = true;
      setDrag(null);
    };
    const done = () => {
      window.removeEventListener("pointermove", move, true);
      window.removeEventListener("pointerup", up, true);
      window.removeEventListener("pointercancel", done, true);
      window.removeEventListener("keydown", key, true);
      if (press.current?.done === done) press.current = null;
      setDrag(null);
    };
    press.current = { id: row.id, section, x: event.clientX, y: event.clientY, ids, block, active: false, cancelled: false, done };
    window.addEventListener("pointermove", move, true);
    window.addEventListener("pointerup", up, true);
    window.addEventListener("pointercancel", done, true);
    window.addEventListener("keydown", key, true);
  };
  const dragStyle = (row: SidebarRow): { className: string; style?: CSSProperties } => {
    if (!drag || drag.section !== row.kind) return { className: "" };
    if (drag.id === row.id) return { className: "pin-dragged", style: { transform: `translateY(${drag.travel}px)` } };
    const block = rows.filter(r => r.kind === drag.section).map(r => r.id);
    const from = block.indexOf(drag.id), mine = block.indexOf(row.id);
    if (drag.target === null || mine !== drag.target || drag.target === from) return { className: "" };
    return { className: drag.target < from ? "drop-above" : "drop-below" };
  };
  const tabRow = (row: SidebarRow) => { const dragged = dragStyle(row); const pinRow = row.kind === "agent" || row.kind === "pinned"; return <div key={`${row.kind}:${row.id}`} data-row={`${row.kind}:${row.id}`} data-pin-section={pinRow ? row.kind : undefined} data-tab={row.id} style={dragged.style} onPointerDown={pinRow ? event => startPress(event, row) : undefined} onClickCapture={event => { if (swallowClick.current) { event.stopPropagation(); event.preventDefault(); } }} className={`sidebar-row tab-row ${row.kind === "tab" ? "indented" : ""} ${selected === row.id ? "selected" : ""} ${dragged.className}`}>
    {renaming === row.id && rows.find(r => r.kind !== "space" && r.id === row.id) === row ? <RenameInput key={row.id} label={row.label} commit={label => commitRename(row.id, label)} cancel={cancelRename} /> : <button className="select-tab" onClick={() => select(row.id)} onDoubleClick={() => startRename(row.id)}>{row.face ? <AgentFace face={row.face} status={row.status} request={row.request} /> : <Status status={row.status} />}<span className="label">{row.label}</span>{row.kind !== "tab" && <span className="muted space-label">{row.spaceLabel}</span>}</button>}
    <button className={`pin ${row.pinned ? "is-pinned" : ""}`} aria-label={row.pinned ? "Unpin tab" : "Pin tab"} onClick={() => pin(row.id, !row.pinned)}>⌖</button>
  </div>; };
  const spaceRow = (row: SidebarRow) => {
    const children = rows.filter(r => r.kind === "tab" && r.section === row.id);
    const open = children.some(r => r.id === renaming) || spaceOpen(row, rows, selected, expanded);
    return <div key={row.id}><button className="sidebar-row space-row" aria-expanded={open} onClick={() => toggle(foldKey(row), !open)}><span className="chevron">{open ? "⌄" : "›"}</span><span className="label">{row.label}</span><Status status={row.status} /></button>{open && children.map(tabRow)}</div>;
  };
  return <aside className={`sidebar ${drag ? "pin-dragging" : ""}`}><nav ref={nav}>
    {["AGENTS", "PINNED"].map(section => { const items = rows.filter(r => r.section === section); return items.length ? <section key={section}><h2>{section}</h2>{items.map(tabRow)}</section> : null; })}
    <section className="spaces">{rows.filter(r => r.kind === "space" && !r.hidden).map(spaceRow)}
    {rows.some(r => r.kind === "space" && r.hidden) && <><button className="sidebar-row muted" aria-expanded={hidden} onClick={() => setHidden(!hidden)}><span className="chevron">{hidden ? "⌄" : "›"}</span>Hidden</button>{(hidden || rows.some(r => r.hidden && r.id === renaming)) && rows.filter(r => r.kind === "space" && r.hidden).map(spaceRow)}</>}
    </section>
  </nav><footer role="status">{notice ?? (machine.state === "up" ? `${machine.name} · connected` : machine.state === "connecting" ? "connecting…" : `offline: ${machine.error || "disconnected"}`)}<UpdatePill /></footer></aside>;
}
