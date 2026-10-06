import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { bridge } from "./bridge";
import type { MachineStatus } from "./bridge";
import { buildSidebar, tabOrder } from "./model";
import type { Snapshot } from "./model";
import Sidebar from "./Sidebar";
import TabView from "./TabView";
import type { PaneController } from "./PaneTerm";
import { installControl } from "./control";
import "@xterm/xterm/css/xterm.css";
import "./styles.css";
export default function App() {
  const [machine, setMachine] = useState<MachineStatus>({ name: "studio", state: "connecting" });
  const [snapshot, setSnapshot] = useState<Snapshot>({});
  const [selected, setSelected] = useState<string | null>(null);
  const [focused, setFocused] = useState<string | null>(null);
  const controllers = useRef(new Map<string, PaneController>());
  const rows = useMemo(() => buildSidebar(snapshot), [snapshot]);
  const state = useRef({ machine, snapshot, selected, focused, rows });
  state.current = { machine, snapshot, selected, focused, rows };
  const select = useCallback((id: string) => {
    if (!state.current.snapshot.tabs?.some(t => t.tab_id === id)) throw new Error(`Unknown tab: ${id}`);
    // Update the control target immediately, before React commits the new view.
    state.current.selected = id;
    state.current.focused = null;
    setSelected(id); setFocused(null);
  }, []);
  useEffect(() => {
    let disposed = false;
    let snapshotRevision = 0;
    let machineRevision = 0;
    let requestRevision = 0;
    const unlisteners: (() => void)[] = [];
    const track = async (promise: Promise<() => void>) => { const fn = await promise; if (disposed) fn(); else unlisteners.push(fn); };
    const refresh = async (name: string) => {
      const revision = snapshotRevision;
      const request = ++requestRevision;
      try { const next = await bridge.snapshot(name); if (!disposed && revision === snapshotRevision && request === requestRevision) setSnapshot(next); }
      catch (error) { if (!disposed && revision === snapshotRevision && request === requestRevision) setMachine({ name, state: "down", error: String(error) }); }
    };
    void (async () => {
      try {
        await track(bridge.machineEvents(status => { if (!disposed && status.name === "studio") { machineRevision++; requestRevision++; setMachine(status); if (status.state === "up") void refresh(status.name); } }));
        await track(bridge.snapshots(value => { if (!disposed && value.machine === "studio") { snapshotRevision++; setSnapshot(value.snapshot); } }));
        const revision = machineRevision;
        const machines = await bridge.machines();
        if (disposed || revision !== machineRevision) return;
        const current = machines.find(m => m.name === "studio") ?? { name: "studio", state: "down" as const, error: "Machine unavailable" };
        setMachine(current);
        if (current.state === "up") await refresh(current.name);
      } catch (error) { if (!disposed) setMachine({ name: "studio", state: "down", error: String(error) }); }
    })();
    return () => { disposed = true; unlisteners.forEach(fn => fn()); };
  }, []);
  const previousOrder = useRef<string[]>([]);
  useEffect(() => {
    const order = tabOrder(rows);
    if (!selected || !order.includes(selected)) {
      const index = selected ? previousOrder.current.indexOf(selected) : -1;
      const initial = snapshot.tabs?.find(t => t.focused)?.tab_id;
      setSelected(index >= 0 ? order[Math.min(index, order.length - 1)] ?? null : initial ?? order[0] ?? null);
    }
    previousOrder.current = order;
  }, [rows, selected, snapshot]);
  useEffect(() => {
    const layout = snapshot.layouts?.find(l => l.tab_id === selected);
    const panes = snapshot.panes?.filter(p => p.tab_id === selected && (!layout?.zoomed || p.pane_id === layout.focused_pane_id)) ?? [];
    if (!panes.some(p => p.pane_id === focused)) setFocused(panes.find(p => p.focused)?.pane_id ?? panes[0]?.pane_id ?? null);
  }, [snapshot, selected, focused]);
  const shortcut = useCallback((event: KeyboardEvent) => {
    if (event.type !== "keydown" || !event.ctrlKey || event.altKey || event.metaKey) return false;
    const current = state.current;
    let target: string | undefined;
    if (/^[1-9]$/.test(event.key)) target = current.rows.find(r => r.hotkey === Number(event.key))?.id;
    else if (event.key === "Tab") { const order = tabOrder(current.rows); const index = order.indexOf(current.selected ?? ""); target = order[(index + (event.shiftKey ? -1 : 1) + order.length) % order.length]; }
    else return false;
    if (target) select(target);
    return true;
  }, [select]);
  useEffect(() => { const handler = (event: KeyboardEvent) => { if (shortcut(event)) { event.preventDefault(); event.stopPropagation(); } }; window.addEventListener("keydown", handler, true); return () => window.removeEventListener("keydown", handler, true); }, [shortcut]);
  const register = useCallback((id: string, value: PaneController | null) => { if (value) controllers.current.set(id, value); else controllers.current.delete(id); }, []);
  useEffect(() => installControl(() => {
    const current = state.current;
    const panes = [...controllers.current.values()].filter(p => current.snapshot.panes?.some(info => info.pane_id === p.info().pane_id && info.tab_id === current.selected));
    return { machine: current.machine, selected: current.selected, rows: current.rows, panes, focused: panes.find(p => p.info().pane_id === current.focused), open: select };
  }), [select]);
  const pin = (id: string, pinned: boolean) => { void bridge.api(machine.name, "tab.set_pinned", { tab_id: id, pinned }).catch(error => setMachine({ ...machine, state: "down", error: String(error) })); };
  return <div className="layout"><Sidebar rows={rows} selected={selected} machine={machine} select={select} pin={pin} /><TabView snapshot={snapshot} selected={selected} machine={machine.name} focused={focused} onFocus={setFocused} shortcut={shortcut} register={register} /></div>;
}
