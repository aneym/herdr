import { useEffect, useMemo, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type { Layout, Snapshot } from "./model";
import { scaleRect } from "./model";
import PaneSurface from "./PaneSurface";
import type { PaneController } from "./PaneTerm";
import { bridge } from "./bridge";
import { dividers, extent, Resizer } from "./dividers";
import type { Divider, ResizeAnswer } from "./dividers";
export default function TabView({ snapshot, selected, machine, focused, onFocus, shortcut, register, pin }: { snapshot: Snapshot; selected: string | null; machine: string; focused: string | null; onFocus: (id: string) => void; shortcut: (event: KeyboardEvent) => boolean; register: (id: string, value: PaneController | null) => void; pin: (id: string, pinned: boolean) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useEffect(() => { if (!host.current) return; const observer = new ResizeObserver(([entry]) => setSize({ width: entry.contentRect.width, height: entry.contentRect.height })); observer.observe(host.current); return () => observer.disconnect(); }, []);
  // While a divider drag runs, panes follow the layouts pane.resize answers with, not snapshots:
  // a snapshot from before the last resize would snap the divider back.
  const [held, setHeld] = useState<Layout | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const snapshotLayout = snapshot.layouts?.find(l => l.tab_id === selected);
  const layout = held?.tab_id === selected ? held : snapshotLayout;
  const shown = useRef(layout); shown.current = layout;
  const release = useRef<ReturnType<typeof setTimeout>>();
  const resizer = useMemo(() => new Resizer(
    async (pane, direction, amount) => ((await bridge.api(machine, "pane.resize", { pane_id: pane, direction, amount })) as { resize?: ResizeAnswer } | null)?.resize ?? null,
    id => shown.current?.splits?.find(s => s.id === id)?.ratio,
    next => setHeld(next),
    // Snapshots were held back; catch up with the latest once the drag has settled.
    () => { clearTimeout(release.current); release.current = setTimeout(() => { if (!resizer.busy) setHeld(null); }, 200); },
  ), [machine]);
  const stop = useRef<(() => void) | null>(null);
  // A drag belongs to one tab on one machine: switching either, or unmounting, drops it.
  useEffect(() => () => { stop.current?.(); resizer.cancel(); clearTimeout(release.current); setHeld(null); setDragging(null); }, [selected, resizer]);
  const panes = (snapshot.panes ?? []).filter(p => p.tab_id === selected && (!layout?.zoomed || p.pane_id === layout.focused_pane_id));
  const lines = layout && !layout.zoomed ? dividers(layout) : [];
  const grab = (event: ReactPointerEvent<HTMLDivElement>, d: Divider) => {
    if (event.button !== 0 || !layout || resizer.busy) return;
    event.preventDefault();
    const el = event.currentTarget;
    try { el.setPointerCapture(event.pointerId); } catch { /* A synthetic pointer has nothing to capture; window listeners still follow it. */ }
    const start = d.vertical ? event.clientX : event.clientY;
    const extentPx = extent(d) * (d.vertical ? size.width / Math.max(1, layout.area.width) : size.height / Math.max(1, layout.area.height));
    const along = (e: PointerEvent) => (d.vertical ? e.clientX : e.clientY) - start;
    const move = (e: PointerEvent) => resizer.moved(d, along(e), extentPx);
    const up = (e: PointerEvent) => { done(); resizer.ended(d, along(e), extentPx); };
    const done = () => { window.removeEventListener("pointermove", move, true); window.removeEventListener("pointerup", up, true); window.removeEventListener("pointercancel", cancel, true); stop.current = null; setDragging(null); };
    const cancel = () => { done(); resizer.ended(d, 0, 0); };
    window.addEventListener("pointermove", move, true);
    window.addEventListener("pointerup", up, true);
    window.addEventListener("pointercancel", cancel, true);
    stop.current = done;
    clearTimeout(release.current);
    // Hold the layout from the press on, so no snapshot redraws the panes mid-drag.
    setHeld(layout);
    setDragging(d.splitId);
    resizer.began(d);
  };
  const boxes = panes.map((pane, index) => {
    const rect = layout?.panes.find(p => p.pane_id === pane.pane_id)?.rect;
    return layout?.zoomed ? { x: 0, y: 0, ...size } : rect && layout ? scaleRect(rect, layout.area, size.width, size.height) : { x: 0, y: size.height * index / panes.length, width: size.width, height: size.height / panes.length };
  });
  // As the Mac's PaneCap: the tab's pin sits on the top-right pane's cap only.
  const corner = boxes.reduce((best, b, i) => best < 0 || b.y < boxes[best].y - .5 || (Math.abs(b.y - boxes[best].y) <= .5 && b.x + b.width > boxes[best].x + boxes[best].width) ? i : best, -1);
  const tab = snapshot.tabs?.find(t => t.tab_id === selected);
  return <main ref={host} className="tab-view">{panes.map((pane, index) => {
    const box = boxes[index];
    return <div key={`${selected}:${pane.terminal_id}`} className="pane-box" style={{ left: box.x, top: box.y, width: Math.max(0, box.width - (box.x + box.width < size.width - .5 ? 1 : 0)), height: Math.max(0, box.height - (box.y + box.height < size.height - .5 ? 1 : 0)) }}><PaneSurface hasAgent={!!pane.agent || !!snapshot.agents?.some(a => a.pane_id === pane.pane_id && a.agent)} pane={pane} machine={machine} focused={focused === pane.pane_id} onFocus={onFocus} shortcut={shortcut} register={register} {...(index === corner && tab ? { pinned: tab.pin_index != null, onPin: () => pin(tab.tab_id, tab.pin_index == null) } : {})} /></div>;
  })}{layout && lines.map(d => {
    const r = scaleRect({ x: d.vertical ? d.pos : d.splitRect.x, y: d.vertical ? d.splitRect.y : d.pos, width: d.vertical ? 0 : d.splitRect.width, height: d.vertical ? d.splitRect.height : 0 }, layout.area, size.width, size.height);
    // The 1 px gap between panes sits just before the line; the grab strip centres on it.
    const style = d.vertical ? { left: r.x - 4.5, top: r.y, width: 7, height: r.height } : { left: r.x, top: r.y - 4.5, width: r.width, height: 7 };
    return <div key={d.splitId} data-split={d.splitId} className={`divider ${d.vertical ? "vertical" : "horizontal"} ${dragging === d.splitId ? "dragging" : ""}`} style={style} onPointerDown={event => grab(event, d)} />;
  })}</main>;
}
