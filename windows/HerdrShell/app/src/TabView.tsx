import { useEffect, useRef, useState } from "react";
import type { Snapshot } from "./model";
import { scaleRect } from "./model";
import PaneTerm from "./PaneTerm";
import type { PaneController } from "./PaneTerm";
export default function TabView({ snapshot, selected, machine, focused, onFocus, shortcut, register }: { snapshot: Snapshot; selected: string | null; machine: string; focused: string | null; onFocus: (id: string) => void; shortcut: (event: KeyboardEvent) => boolean; register: (id: string, value: PaneController | null) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useEffect(() => { if (!host.current) return; const observer = new ResizeObserver(([entry]) => setSize({ width: entry.contentRect.width, height: entry.contentRect.height })); observer.observe(host.current); return () => observer.disconnect(); }, []);
  const layout = snapshot.layouts?.find(l => l.tab_id === selected);
  const panes = (snapshot.panes ?? []).filter(p => p.tab_id === selected && (!layout?.zoomed || p.pane_id === layout.focused_pane_id));
  return <main ref={host} className="tab-view">{panes.map((pane, index) => {
    const rect = layout?.panes.find(p => p.pane_id === pane.pane_id)?.rect;
    const box = layout?.zoomed ? { x: 0, y: 0, ...size } : rect && layout ? scaleRect(rect, layout.area, size.width, size.height) : { x: 0, y: size.height * index / panes.length, width: size.width, height: size.height / panes.length };
    return <div key={`${selected}:${pane.terminal_id}`} className="pane-box" style={{ left: box.x, top: box.y, width: Math.max(0, box.width - (box.x + box.width < size.width - .5 ? 1 : 0)), height: Math.max(0, box.height - (box.y + box.height < size.height - .5 ? 1 : 0)) }}><PaneTerm pane={pane} machine={machine} focused={focused === pane.pane_id} onFocus={onFocus} shortcut={shortcut} register={register} /></div>;
  })}</main>;
}
