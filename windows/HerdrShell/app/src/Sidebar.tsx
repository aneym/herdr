import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, PointerEvent as ReactPointerEvent } from "react";
import { buildAreas, focusTabs, stageImplied, stageWord } from "./areas";
import { bridge } from "./bridge";
import type { AreaLine } from "./areas";
import type { AreaChip } from "./areas";
import { LaneSnapshot } from "./laneFiles";
import type { Snapshot } from "./model";
import { setAgentHidden } from "./control";
import UpdatePill from "./UpdatePill";
import { space } from "./tokens";
import AgentFace from "./AgentFace";
import { DRAG_THRESHOLD, slotAt } from "./pinDrag";
import type { PinSection, RowBox } from "./pinDrag";
import type { MachineStatus } from "./bridge";
import { foldAllSpaces, foldKey, noteSelection, revealOnSelect, spaceOpen } from "./model";
import type { RevealMemo, SidebarRow } from "./model";
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
/** The sidebar's reveal memo, held by its parent so it outlives hiding the sidebar. Noted in
 *  render, ahead of the sidebar's effects, which run before the parent's own; idempotent. */
export function useSelectionReveal(selected: string | null): RevealMemo {
  const memo = useRef<RevealMemo>({ last: undefined, pending: null }).current;
  noteSelection(memo, selected);
  return memo;
}
const homeGlyphs = { cloud: "☁︎", local: "⌂︎", unsynced: "⇡︎" };
const homeTitles = { cloud: "Memory in Rails cloud", local: "Memory on this machine only", unsynced: "Memory not synced to Rails cloud" };
interface Press { id: string; section: PinSection; x: number; y: number; ids: string[]; block: RowBox[]; active: boolean; cancelled: boolean; done: () => void }
function scopeSlug(url?: string): string | null {
  try { const route = new URL(url!).searchParams.get("route"); const slug = route?.startsWith("scoping/") ? route.slice(8) : ""; return /^[a-z0-9][a-z0-9-]{0,80}$/.test(slug) ? slug : null; } catch { return null; }
}
/** Shared presentation controls; also usable when the sidebar is hidden. */
export function useSidebarNavigation(snapshot: Snapshot, catalog: LaneSnapshot, selected: string | null) {
  const read = <T,>(key: string, fallback: T): T => { try { return JSON.parse(localStorage.getItem(`herdr-shell.areas.${key}`) ?? "null") ?? fallback; } catch { return fallback; } };
  const save = (key: string, value: unknown) => { try { localStorage.setItem(`herdr-shell.areas.${key}`, JSON.stringify(value)); } catch { /* Storage can be disabled. */ } };
  const [mode, setMode] = useState<"areas" | "spaces">(() => read("mode", "spaces"));
  const [chip, setChip] = useState<AreaChip>(() => read("chip", "all"));
  const [focusCursor, setFocusCursor] = useState<number | null>(null);
  const changeMode = (value: "areas" | "spaces") => { setMode(value); setFocusCursor(null); save("mode", value); };
  const changeChip = (value: AreaChip) => { setChip(value); setFocusCursor(null); save("chip", value); };
  return { mode, chip, focusCursor, changeMode, changeChip, noteSelection: (stepping: boolean) => { if (!stepping) setFocusCursor(null); }, stepFocus: (delta: number) => {
    const ids = focusTabs(snapshot, catalog);
    if (!ids.length) return undefined;
    const current = focusCursor !== null ? focusCursor - 1 : ids.indexOf(selected ?? "");
    const next = current < 0 ? (delta > 0 ? 0 : ids.length - 1) : (current + delta + ids.length) % ids.length;
    setFocusCursor(next + 1);
    return ids[next];
  } };
}
export type SidebarNavigation = ReturnType<typeof useSidebarNavigation>;
export default function Sidebar({ navigation, snapshot = {}, catalog = new LaneSnapshot(), machines, chooseMachine, rows, selected, revealed, machine, notice, select, pin, movePin, renaming, startRename, cancelRename, commitRename, paneDropRow, openDetail }: { openDetail?: (id: string) => void; navigation?: SidebarNavigation; paneDropRow?: string | null; snapshot?: Snapshot; catalog?: LaneSnapshot; machines: MachineStatus[]; chooseMachine: (name: string) => void; rows: SidebarRow[]; selected: string | null; revealed: RevealMemo; machine: MachineStatus; notice: string | null; select: (id: string) => void; pin: (id: string, pinned: boolean) => void; movePin: (ids: string[], from: number, to: number) => void; renaming: string | null; startRename: (id: string) => void; cancelRename: () => void; commitRename: (id: string, label: string) => Promise<void> }) {
  // Folds are per machine: workspace ids repeat across machines. Studio keeps the pre-switcher key.
  const foldStore = machine.name === "studio" ? "herdr-space-expanded" : `herdr-space-expanded:${machine.name}`;
  const [expanded, setExpanded] = useState<Record<string, boolean>>(() => { try { return JSON.parse(localStorage.getItem(foldStore) || "{}"); } catch { return {}; } });
  const [hidden, setHidden] = useState(false);
  const read = <T,>(key: string, fallback: T): T => { try { return JSON.parse(localStorage.getItem(`herdr-shell.areas.${key}`) ?? "null") ?? fallback; } catch { return fallback; } };
  const save = (key: string, value: unknown) => { try { localStorage.setItem(`herdr-shell.areas.${key}`, JSON.stringify(value)); } catch { /* Storage can be disabled by WebView policy. */ } };
  const [hiddenAgents, setHiddenAgents] = useState(() => read("hiddenAgents", false));
  const [menu, setMenu] = useState<{ row?: SidebarRow; line?: AreaLine; x: number; y: number } | null>(null);
  const [menuError, setMenuError] = useState<string | null>(null);
  const [prompt, setPrompt] = useState<{ line: AreaLine; verb: "park" | "approve" } | null>(null);
  const [words, setWords] = useState("");
  const [savingAction, setSavingAction] = useState(false);
  const runAction = async (verb: "park" | "unpark" | "approve", args: string[]) => {
    setSavingAction(true); setMenuError(null);
    try { await bridge.remoteAction(machine.name, verb, args); setPrompt(null); }
    catch (error) { setMenuError(String(error)); }
    finally { setSavingAction(false); }
  };
  const ask = (line: AreaLine, verb: "park" | "approve") => { setMenu(null); setWords(verb === "approve" ? "approved" : ""); setPrompt({ line, verb }); };
  const menuRef = useRef<HTMLDivElement>(null);
  const menuReturnFocus = useRef<HTMLElement | null>(null);
  useEffect(() => {
    if (!menu) return;
    const close = (event: MouseEvent) => { if (!menuRef.current?.contains(event.target as Node)) setMenu(null); };
    const escape = (event: KeyboardEvent) => { if (event.key === "Escape") { event.stopPropagation(); setMenu(null); } };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", escape);
    menuRef.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
    return () => { document.removeEventListener("mousedown", close); document.removeEventListener("keydown", escape); };
  }, [menu]);
  useEffect(() => { setMenu(null); setPrompt(null); setMenuError(null); }, [machine.name]);
  const localNavigation = useSidebarNavigation(snapshot, catalog, selected);
  const { mode, chip, focusCursor, changeMode, changeChip } = navigation ?? localNavigation;
  const [folded, setFolded] = useState<string[]>(() => read("folded", []));
  const [focusExpanded, setFocusExpanded] = useState(() => read("focusExpanded", false));
  const [areaOnly, setAreaOnly] = useState<string | null>(() => read("only", null));
  const [manualOpen, setManualOpen] = useState<Record<string, boolean>>({});
  const parkedCount = useMemo(() => mode === "areas" ? buildAreas(snapshot, catalog, { chip: "parked", areaOnly }).length : 0, [snapshot, catalog, areaOnly, mode]);
  const areaLines = useMemo(() => mode === "areas" ? buildAreas(snapshot, catalog, { chip, folded: new Set(folded), focusExpanded, focusCursor, selectedTab: selected, manualOpen, areaOnly }) : [], [snapshot, catalog, chip, folded, focusExpanded, focusCursor, selected, manualOpen, areaOnly, mode]);
  const toggleAreaLine = (id: string, open: boolean) => {
    if (id === "focus") { setFocusExpanded(open); save("focusExpanded", open); }
    else if (id.startsWith("area:")) { const area = id.slice(5); const next = open ? folded.filter(a => a !== area) : [...folded, area]; setFolded(next); save("folded", next); }
    else setManualOpen(previous => ({ ...previous, [id]: open }));
  };

  const anySpaceExpanded = rows.some(row => row.kind === "space" && spaceOpen(row, rows, selected, expanded));
  const toggleAllSpaces = () => setExpanded(previous => {
    const next = foldAllSpaces(rows, previous, !anySpaceExpanded);
    try { localStorage.setItem(foldStore, JSON.stringify(next)); } catch { /* Storage can be disabled by WebView policy. */ }
    return next;
  });
  const toggle = (id: string, value: boolean) => setExpanded(previous => { const next = { ...previous, [id]: value }; try { localStorage.setItem(foldStore, JSON.stringify(next)); } catch { /* Storage can be disabled by WebView policy. */ } return next; });
  // Drag an AGENTS or PINNED row within its section, as the Mac's PinDrag: the row follows the
  // pointer, a line marks the slot, a release on the section moves the pin, Esc or a release
  // off the section moves nothing, and a press that travels less than the threshold is a click.
  const reveal = useRef({ rows, expanded });
  reveal.current = { rows, expanded };
  // Only a change of selection reveals; folding the selected tab's space afterwards sticks, also
  // across hiding and showing the sidebar, since App notes the changes (RevealMemo).
  useEffect(() => {
    const hiddenAgent = mode === "spaces" && revealed.pending !== null && revealed.pending === selected && reveal.current.rows.some(row => row.id === selected && row.kind === "agent" && row.hidden);
    const key = revealOnSelect(revealed, reveal.current.rows, selected, reveal.current.expanded);
    if (hiddenAgent) { setHiddenAgents(true); save("hiddenAgents", true); }
    else if (key) toggle(key, true);
  }, [selected, revealed]);
  const nav = useRef<HTMLElement>(null);
  const press = useRef<Press | null>(null);
  const swallowClick = useRef(false);
  const [drag, setDrag] = useState<{ id: string; section: PinSection; travel: number; target: number | null } | null>(null);
  useEffect(() => () => press.current?.done(), []);
  const startPress = (event: ReactPointerEvent, row: SidebarRow) => {
    if (event.button !== 0 || (row.kind !== "agent" && row.kind !== "pinned") || row.hidden || renaming === row.id) return;
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
    const block = rows.filter(r => r.kind === drag.section && !r.hidden).map(r => r.id);
    const from = block.indexOf(drag.id), mine = block.indexOf(row.id);
    if (drag.target === null || mine !== drag.target || drag.target === from) return { className: "" };
    return { className: drag.target < from ? "drop-above" : "drop-below" };
  };
  const tabRow = (row: SidebarRow) => { const dragged = dragStyle(row); const pinRow = !row.hidden && (row.kind === "agent" || row.kind === "pinned"); return <div key={`${row.kind}:${row.id}`} data-row={`${row.kind}:${row.id}`} data-pin-section={pinRow ? row.kind : undefined} data-tab={row.id} style={dragged.style} onPointerDown={pinRow ? event => startPress(event, row) : undefined} onClickCapture={event => { if (swallowClick.current) { event.stopPropagation(); event.preventDefault(); } }} className={`sidebar-row tab-row ${row.kind === "tab" ? "indented" : ""} ${selected === row.id ? "selected" : ""} ${dragged.className} ${paneDropRow === `tab:${row.id}` ? "pane-drop-fill" : ""}`}>
    {renaming === row.id && rows.find(r => r.kind !== "space" && r.id === row.id) === row ? <RenameInput key={row.id} label={row.label} commit={label => commitRename(row.id, label)} cancel={cancelRename} /> : <button className="select-tab" onContextMenu={event => { menuReturnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null; event.preventDefault(); setMenuError(null); setMenu({ row, x: event.clientX, y: event.clientY }); }} onClick={() => select(row.id)} onDoubleClick={() => startRename(row.id)}>{row.face ? <AgentFace face={row.face} status={row.status} request={row.request} /> : <Status status={row.status} />}<span className="label">{row.label}</span>{row.kind === "pinned" && <span className="muted space-label">{row.spaceLabel}</span>}{row.kind === "agent" && row.home && ["cloud", "local", "unsynced"].includes(row.home) && <span className={`home-glyph ${row.home === "unsynced" ? "warn" : "muted"}`} title={homeTitles[row.home]} aria-label={homeTitles[row.home]}>{homeGlyphs[row.home]}</span>}</button>}
    <button className={`pin ${row.pinned ? "is-pinned" : ""}`} aria-label={row.pinned ? "Unpin tab" : "Pin tab"} onClick={() => pin(row.id, !row.pinned)}>⌖</button>
  </div>; };
  const spaceRow = (row: SidebarRow) => {
    const children = rows.filter(r => r.kind === "tab" && r.section === row.id);
    const open = children.some(r => r.id === renaming) || spaceOpen(row, rows, selected, expanded);
    return <div key={row.id}><button data-space={row.id} className={`sidebar-row space-row ${paneDropRow === `space:${row.id}` ? "drop-above" : ""}`} aria-expanded={open} onClick={() => toggle(foldKey(row), !open)}><span className="chevron">{open ? "⌄" : "›"}</span><span className="label">{row.label}</span><Status status={row.status} /></button>{open && children.map(tabRow)}</div>;
  };
  const collapseSpacesButton = <button className="spaces-fold-all" aria-label={anySpaceExpanded ? "Collapse all spaces" : "Expand all spaces"} title={anySpaceExpanded ? "Collapse all spaces" : "Expand all spaces"} onClick={toggleAllSpaces}><svg width="12" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={anySpaceExpanded ? "m8 4 4 4 4-4M12 2v6m-4 12 4-4 4 4M12 16v6M4 12h16" : "m8 6 4-4 4 4M12 2v6m-4 10 4 4 4-4M12 16v6M4 12h16"} /></svg></button>;
  return <aside className={`sidebar ${drag ? "pin-dragging" : ""}`}><div className="machine-row" aria-label="Machines">{machines.map(item => <button key={item.name} aria-pressed={item.name === machine.name} onClick={() => chooseMachine(item.name)}><Status status={item.state} /><span>{item.name}</span></button>)}</div><div className="areas-mode" aria-label="Sidebar mode">{(["areas", "spaces"] as const).map(value => <button key={value} aria-pressed={mode === value} onClick={() => changeMode(value)}>{value === "areas" ? "Areas" : "Spaces"}</button>)}{mode === "spaces" && rows.some(row => row.kind === "space") && collapseSpacesButton}</div><nav ref={nav}>
    {mode === "areas" ? <>
      <div className="areas-chips" aria-label="Area filters">{([["all", "All"], ["scoping", "Scope"], ["building", "Build"], ["review", "Review"], ["use", "Use"], ["parked", "Parked"]] as const).map(([value, title]) => <button key={value} aria-pressed={chip === value} onClick={() => changeChip(value)}>{value === "parked" && parkedCount ? `Parked ${parkedCount}` : title}</button>)}</div>
      {areaOnly && <button className="sidebar-row muted" onClick={() => { setAreaOnly(null); save("only", null); }}>Only {catalog.areaName(areaOnly)} ×</button>}
      {areaLines.map(line => line.kind === "header" ? <h2 key={line.id}>{line.title}</h2> : <div key={line.id} data-row={line.id} className={`sidebar-row areas-line ${line.parked ? "areas-parked-row" : ""} ${(line.selected || (line.kind === "focus" && chip === "needs")) ? "selected" : ""} ${line.dim ? "muted" : ""}`} style={{ paddingLeft: space.sidebarAreaInset + line.depth * space.sidebarIndent }} onContextMenu={event => {
        if (!line.tab) return;
        event.preventDefault(); setMenuError(null); setMenu({ line, x: event.clientX, y: event.clientY });
      }}>
        {line.toggle && <button className="chevron" aria-label={`Fold ${line.title}`} aria-expanded={line.chevron} onClick={() => toggleAreaLine(line.toggle!, !line.chevron)}>{line.chevron ? "⌄" : "›"}</button>}
        <button className="select-tab" onClick={event => {
          if (renaming === line.tab) return;
          if (line.kind === "focus") { changeChip("needs"); toggleAreaLine("focus", !focusExpanded); }
          else if (line.kind === "area" && event.altKey) { setAreaOnly(line.area!); save("only", line.area!); }
          else if (line.tab) select(line.tab);
          else if (line.toggle) toggleAreaLine(line.toggle, !line.chevron);
        }}>
          {line.kind === "area" && <span className="areas-dot" style={{ backgroundColor: line.color }} />}
          {line.glyph && <span className={`areas-glyph ${line.glyphTone}`} aria-label={line.status}>{line.glyph}</span>}
          <span className="label">{line.tab && renaming === line.tab ? <RenameInput label={line.title} commit={label => commitRename(line.tab!, label)} cancel={cancelRename} /> : line.title}{line.parkNote && <small className="areas-park-note">{line.parkNote}</small>}</span>{line.badge && (chip === "all" || !stageImplied(chip, line)) && <span className={`areas-badge ${line.badge === "Ready for review" || line.stage === "reviewing" ? "is-review" : ""}`}>{stageWord(line.badge)}</span>}{line.trailing && <span className="areas-trailing">{line.trailing}</span>}
        </button>
      </div>)}
    </> : <>
    {["AGENTS", "PINNED"].map(section => { const items = rows.filter(r => r.section === section); return items.length ? <section key={section}><h2>{section}</h2>{items.filter(r => !r.hidden).map(tabRow)}{section === "AGENTS" && items.some(r => r.hidden) && <>
      <button data-row="hiddenagents" className="sidebar-row muted hidden-agents" aria-expanded={hiddenAgents} onClick={() => { setHiddenAgents(!hiddenAgents); save("hiddenAgents", !hiddenAgents); }}><span>Hidden</span><span>{items.filter(r => r.hidden).length}</span>{!hiddenAgents && items.some(r => r.hidden && (r.status === "blocked" || r.request != null)) && <span className="hidden-agents-dot" data-dot="accent" />}<span className="chevron">{hiddenAgents ? "▾" : "▸"}</span></button>
      {hiddenAgents && items.filter(r => r.hidden).map(tabRow)}
    </>}</section> : null; })}
    <section className="spaces">{rows.filter(r => r.kind === "space" && !r.hidden).map(spaceRow)}
    {rows.some(r => r.kind === "space" && r.hidden) && <><button className="sidebar-row muted" aria-expanded={hidden} onClick={() => setHidden(!hidden)}><span className="chevron">{hidden ? "⌄" : "›"}</span>Hidden</button>{(hidden || rows.some(r => r.hidden && r.id === renaming)) && rows.filter(r => r.kind === "space" && r.hidden).map(spaceRow)}</>}
    </section>
    </>}
  </nav>{menu && <div ref={menuRef} className="pane-menu agent-menu" role="menu" style={{ left: Math.min(menu.x, Math.max(0, window.innerWidth - 180)), top: Math.min(menu.y, Math.max(0, window.innerHeight - 40)) }}>
    {menu.row && openDetail && <button role="menuitem" onClick={() => { const id = menu.row!.id; setMenu(null); openDetail(id); menuReturnFocus.current?.focus(); }}>Show info</button>}
    {menu.row?.kind === "agent" && <button role="menuitem" onClick={() => { const row = menu.row!; setMenu(null); void setAgentHidden(machine.name, row.id, !row.hidden).catch(error => setMenuError(String(error))); }}>{menu.row.hidden ? "Show in Agents" : "Hide"}</button>}
    {menu.line && <>
      <button role="menuitem" onClick={() => { startRename(menu.line!.tab!); setMenu(null); }}>Rename…</button>
      {menu.line.parked ? <button role="menuitem" disabled={savingAction} onClick={() => { const tab = menu.line!.tab!; setMenu(null); void runAction("unpark", [tab]); }}>Resume</button> : (menu.line.kind === "lane" || menu.line.kind === "orchestrator") && <button role="menuitem" onClick={() => ask(menu.line!, "park")}>Park…</button>}
      {scopeSlug(catalog.lanes[menu.line.tab!]?.scopeURL) && <button role="menuitem" onClick={() => ask(menu.line!, "approve")}>Approve scope…</button>}
    </>}
  </div>}
  {prompt && <div className="pane-menu restart-confirm" role="dialog" aria-label={`${prompt.verb === "park" ? "Park" : "Approve"} ${prompt.line.title}?`} onKeyDown={event => { if (event.key === "Escape") { event.stopPropagation(); setPrompt(null); } }}>
    <p>{prompt.verb === "park" ? "Park" : "Approve"} {prompt.line.title}?</p>
    {prompt.verb === "park" && <p>It leaves every filter but Parked and stops counting in Needs you. Resume puts it back.</p>}
    <input className="rename-input" autoFocus value={words} disabled={savingAction} aria-label={prompt.verb === "park" ? "Park note" : "Approval quote"} placeholder={prompt.verb === "park" ? "Note (optional): why, and when to come back" : "Your words, saved as the approval quote"} onChange={event => setWords(event.target.value)} />
    <div className="restart-actions"><button onClick={() => setPrompt(null)} disabled={savingAction}>Cancel</button><button className="restart-primary" disabled={savingAction || (prompt.verb === "approve" && !words.trim())} onClick={() => {
      const args = prompt.verb === "park" ? [prompt.line.tab!, ...(words.trim() ? [`--note=${words.trim()}`] : [])] : [scopeSlug(catalog.lanes[prompt.line.tab!]?.scopeURL)!, `--quote=${words}`, "--by=alex"];
      void runAction(prompt.verb, args);
    }}>{prompt.verb === "park" ? "Park" : "Approve"}</button></div>
  </div>}
  <footer role="status">{notice ?? menuError ?? (machine.state === "up" ? `${machine.name} · connected` : machine.state === "connecting" ? "connecting…" : `offline: ${machine.error || "disconnected"}`)}<UpdatePill /></footer></aside>;
}
