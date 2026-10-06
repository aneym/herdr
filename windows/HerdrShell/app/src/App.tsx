import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { cycleMachine } from "./machines";
import { bridge } from "./bridge";
import type { MachineStatus } from "./bridge";
import { buildSidebar, noteSelection, pinCount, tabOrder } from "./model";
import type { RevealMemo } from "./model";
import type { Snapshot } from "./model";
import Sidebar from "./Sidebar";
import { useAgentCards } from "./AgentFace";
import Switcher from "./Switcher";
import { actionFor } from "./keys";
import type { Action } from "./keys";
import { runAction } from "./actions";
import TabView from "./TabView";
import DocPanel, { useDocs } from "./DocPanel";
import type { DocsState } from "./docs";
import type { PaneController } from "./PaneTerm";
import { installControl } from "./control";
import { PENDING_LIFETIME_MS, pinMovePlan } from "./pinDrag";
import type { PendingOrders } from "./pinDrag";
import type { ControlState } from "./control";
import type { MutableRefObject } from "react";
import "@xterm/xterm/css/xterm.css";
import "./tokens.css";
import "./styles.css";
interface ViewSelection { selected: string | null; focused: string | null }
export default function App() {
  const [machines, setMachines] = useState<MachineStatus[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [active, setActive] = useState(() => { try { return localStorage.getItem("herdr-shell.machine") || "studio"; } catch { return "studio"; } });
  const [snapshots, setSnapshots] = useState<Record<string, Snapshot>>({});
  const selections = useRef(new Map<string, ViewSelection>());
  const control = useRef<(() => ControlState) | null>(null);
  useEffect(() => installControl(() => {
    if (!control.current) throw new Error("No active machine view");
    return control.current();
  }), []);
  const current = useRef({ machines, active });
  current.current = { machines, active };
  const chooseMachine = useCallback((name: string) => {
    const target = current.current.machines.find(m => m.name === name);
    if (!target) throw new Error(`Unknown machine: ${name}`);
    if (current.current.active === name) return target;
    try { localStorage.setItem("herdr-shell.machine", name); } catch { /* Storage can be disabled. */ }
    flushSync(() => setActive(name));
    return target;
  }, []);
  useEffect(() => {
    let disposed = false;
    const revisions = new Map<string, number>();
    const statusRevisions = new Map<string, number>();
    const requests = new Map<string, number>();
    const unlisteners: (() => void)[] = [];
    const track = async (promise: Promise<() => void>) => { const fn = await promise; if (disposed) fn(); else unlisteners.push(fn); };
    const refresh = async (name: string) => {
      const revision = revisions.get(name);
      const request = (requests.get(name) ?? 0) + 1;
      requests.set(name, request);
      try {
        const snapshot = await bridge.snapshot(name);
        if (!disposed && revision === revisions.get(name) && request === requests.get(name)) setSnapshots(value => ({ ...value, [name]: snapshot }));
      } catch { /* The supervisor publishes connection failures. */ }
    };
    void (async () => {
      await track(bridge.machineEvents(status => {
        if (disposed) return;
        statusRevisions.set(status.name, (statusRevisions.get(status.name) ?? 0) + 1);
        requests.set(status.name, (requests.get(status.name) ?? 0) + 1);
        setMachines(value => [...value.filter(m => m.name !== status.name), status].sort((a, b) => a.name.localeCompare(b.name)));
        if (status.state === "up") void refresh(status.name);
      }));
      await track(bridge.snapshots(value => {
        if (disposed) return;
        revisions.set(value.machine, (revisions.get(value.machine) ?? 0) + 1);
        setSnapshots(previous => ({ ...previous, [value.machine]: value.snapshot }));
      }));
      const before = new Map(statusRevisions);
      const list = await bridge.machines();
      if (disposed) return;
      setLoaded(true);
      setMachines(value => list.map(status => before.get(status.name) !== statusRevisions.get(status.name) ? value.find(m => m.name === status.name) ?? status : status));
      for (const status of list) if (status.state === "up" && before.get(status.name) === statusRevisions.get(status.name)) void refresh(status.name);
    })().catch(error => {
      if (!disposed) setMachines([{ name: current.current.active, state: "down", error: String(error) }]);
    });
    return () => { disposed = true; unlisteners.forEach(fn => fn()); };
  }, []);
  useEffect(() => {
    if (loaded && machines.length && !machines.some(m => m.name === active)) {
      const name = machines[0].name;
      try { localStorage.setItem("herdr-shell.machine", name); } catch { /* Storage can be disabled. */ }
      setActive(name);
    }
  }, [machines, active, loaded]);
  const machine: MachineStatus = machines.find(m => m.name === active) ?? { name: active, state: loaded ? "down" : "connecting", error: loaded ? "Machine unavailable" : undefined };
  return <MachineView key={active} machine={machine} machines={machines} snapshot={snapshots[active] ?? {}} chooseMachine={chooseMachine} selections={selections.current} control={control} />;
}
function MachineView({ machine, machines, snapshot, chooseMachine, selections, control }: { machine: MachineStatus; machines: MachineStatus[]; snapshot: Snapshot; chooseMachine: (name: string) => MachineStatus; selections: Map<string, ViewSelection>; control: MutableRefObject<(() => ControlState) | null> }) {
  const [selected, setSelected] = useState<string | null>(selections.get(machine.name)?.selected ?? null);
  const [focused, setFocused] = useState<string | null>(selections.get(machine.name)?.focused ?? null);
  const savedSelection = useRef({ selected, focused });
  savedSelection.current = { selected, focused };
  useEffect(() => () => { selections.set(machine.name, savedSelection.current); }, [selections, machine.name]);
  const [sidebarVisible, setSidebarVisible] = useState(true);
  const [switcherOpen, setSwitcherOpen] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ text: string } | null>(null);
  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), 5000);
    return () => clearTimeout(timer);
  }, [notice]);
  const showError = useCallback((error: unknown) => setNotice({ text: String(error) }), []);
  const [pending, setPending] = useState<{ tabId: string; paneId: string } | null>(null);
  const controllers = useRef(new Map<string, PaneController>());
  const revealed = useRef<RevealMemo>({ last: undefined, pending: null }).current;
  // Noted in render, ahead of the sidebar's effects, which run before App's own; idempotent.
  noteSelection(revealed, selected);
  // A dropped pin order shows until the snapshot agrees or PENDING_LIFETIME_MS passes.
  const [pendingPins, setPendingPins] = useState<PendingOrders>({});
  const cards = useAgentCards(machine.name, machine.state === "up");
  const rows = useMemo(() => buildSidebar(snapshot, pendingPins, Date.now(), cards), [snapshot, pendingPins, cards]);
  useEffect(() => {
    const entries = Object.values(pendingPins);
    if (!entries.length) return;
    const shown = buildSidebar(snapshot);
    const settled = (Object.keys(pendingPins) as (keyof PendingOrders)[]).filter(section => {
      const entry = pendingPins[section];
      return !entry || Date.now() - entry.at >= PENDING_LIFETIME_MS || shown.filter(r => r.kind === section).map(r => r.id).join() === entry.order.join();
    });
    if (settled.length) { setPendingPins(value => { const next = { ...value }; settled.forEach(section => delete next[section]); return next; }); return; }
    const timer = setTimeout(() => setPendingPins(value => ({ ...value })), PENDING_LIFETIME_MS - (Date.now() - Math.min(...entries.map(e => e!.at))));
    return () => clearTimeout(timer);
  }, [snapshot, pendingPins]);
  const docsKey = `herdr-shell.docs.${machine.name}.${selected}`;
  const [docsShown, setDocsShown] = useState<Record<string, boolean>>({});
  const [docsActive, setDocsActive] = useState<Record<string, string>>({});
  const storedDocs = () => { try { return localStorage.getItem(docsKey) === "true"; } catch { return false; } };
  const docsOpen = selected !== null && (docsShown[docsKey] ?? storedDocs());
  const { items: docsItems, error: docsError } = useDocs(machine.name, selected, snapshot, docsOpen);
  const activeDoc = docsItems.find(item => item.name === docsActive[docsKey])?.name ?? docsItems[0]?.name ?? null;
  const docs: DocsState = { open: docsOpen, items: docsItems.map(item => item.name), active: activeDoc };
  const state = useRef({ machine, machines, snapshot, selected, focused, rows, docs, docsKey, docsShown });
  state.current = { machine, machines, snapshot, selected, focused, rows, docs, docsKey, docsShown };
  const select = useCallback((id: string) => {
    if (!state.current.snapshot.tabs?.some(t => t.tab_id === id)) throw new Error(`Unknown tab: ${id}`);
    // Update the control target immediately, before React commits the new view.
    if (state.current.selected !== id) {
      const current = state.current;
      const key = `herdr-shell.docs.${current.machine.name}.${id}`;
      let open = current.docsShown[key];
      if (open === undefined) { try { open = localStorage.getItem(key) === "true"; } catch { open = false; } }
      current.docsKey = key;
      current.docs = { open, items: [], active: null };
    }
    state.current.selected = id;
    state.current.focused = null;
    setRenaming(null); setSelected(id); setFocused(null);
  }, []);
  const previousOrder = useRef<string[]>([]);
  useEffect(() => {
    const order = tabOrder(rows);
    if (!selected || !order.includes(selected)) {
      const index = selected ? previousOrder.current.indexOf(selected) : -1;
      const initial = snapshot.tabs?.find(t => t.focused)?.tab_id;
      setRenaming(null);
      setSelected(index >= 0 ? order[Math.min(index, order.length - 1)] ?? null : initial ?? order[0] ?? null);
    }
    previousOrder.current = order;
  }, [rows, selected, snapshot]);
  useEffect(() => {
    const layout = snapshot.layouts?.find(l => l.tab_id === selected);
    const panes = snapshot.panes?.filter(p => p.tab_id === selected && (!layout?.zoomed || p.pane_id === layout.focused_pane_id)) ?? [];
    if (!panes.some(p => p.pane_id === focused)) { setRenaming(null); setFocused(panes.find(p => p.focused)?.pane_id ?? panes[0]?.pane_id ?? null); }
  }, [snapshot, selected, focused]);
  const focus = useCallback((id: string) => {
    state.current.focused = id;
    setRenaming(null); setFocused(id);
    controllers.current.get(id)?.focus();
  }, []);
  useEffect(() => {
    if (!pending || !snapshot.tabs?.some(t => t.tab_id === pending.tabId) || !snapshot.panes?.some(p => p.pane_id === pending.paneId && p.tab_id === pending.tabId)) return;
    select(pending.tabId); focus(pending.paneId); setPending(null);
  }, [pending, snapshot, select, focus]);
  const action = useCallback((name: string, label?: string, tabId?: string) => {
    const current = state.current;
    if (name === "next_machine" || name === "prev_machine") {
      const next = cycleMachine(current.machines.map(m => m.name), current.machine.name, name === "next_machine" ? 1 : -1);
      if (next) chooseMachine(next);
      return Promise.resolve();
    }
    if (name === "toggle_docs") {
      if (current.selected) {
        const open = !current.docs.open;
        current.docs = { ...current.docs, open };
        current.docsShown = { ...current.docsShown, [current.docsKey]: open };
        setDocsShown(value => ({ ...value, [current.docsKey]: open }));
        try { localStorage.setItem(current.docsKey, String(open)); } catch (error) { showError(error); }
      }
      return Promise.resolve();
    }
    return runAction(name as Action, {
      machine: current.machine.name, snapshot: current.snapshot, rows: current.rows, selected: current.selected, focused: current.focused,
      api: bridge.api, select, focus, created: (tabId, paneId) => setPending({ tabId, paneId }),
      rename: id => { setSidebarVisible(true); setRenaming(id); }, switcher: () => { setRenaming(null); setSwitcherOpen(value => !value); },
      toggleSidebar: () => { setRenaming(null); setSidebarVisible(value => !value); },
      error: showError, label, tabId,
    });
  }, [select, focus, showError, chooseMachine]);
  const closeSwitcher = () => { setSwitcherOpen(false); const id = state.current.focused; if (id) controllers.current.get(id)?.focus(); };
  const shortcut = useCallback((event: KeyboardEvent) => {
    if (event.type !== "keydown") return false;
    if (event.target instanceof Element && event.target.closest("textarea:not(.xterm-helper-textarea)") &&
        !(event.ctrlKey && !event.altKey && !event.metaKey &&
          ((!event.shiftKey && /^[1-9]$/.test(event.key)) || event.key === "Tab" ||
           (event.shiftKey && ["p", "m", "[", "]", "{", "}"].includes(event.key.toLowerCase()))))) return false;
    const name = actionFor(event);
    if (switcherOpen && name === "switcher") { void action(name).catch(() => {}); return true; }
    if (switcherOpen || renaming) return false;
    if (event.target instanceof Element && event.target.closest('input, [contenteditable="true"]')) return false;
    if (event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey && event.key.toLowerCase() === "m") { controllers.current.get(state.current.focused ?? "")?.toggleChat?.(); return true; }
    if (!name) return false;
    void action(name).catch(() => {});
    return true;
  }, [action, switcherOpen, renaming]);
  useEffect(() => { const handler = (event: KeyboardEvent) => { if (shortcut(event)) { event.preventDefault(); event.stopPropagation(); } }; window.addEventListener("keydown", handler, true); return () => window.removeEventListener("keydown", handler, true); }, [shortcut]);
  const register = useCallback((id: string, value: PaneController | null) => { if (value) controllers.current.set(id, value); else controllers.current.delete(id); }, []);
  control.current = () => {
    const current = state.current;
    const panes = [...controllers.current.values()].filter(p => current.snapshot.panes?.some(info => info.pane_id === p.info().pane_id && info.tab_id === current.selected));
    return { machine: current.machine, machines: current.machines, chooseMachine, selected: current.selected, rows: current.rows, docs: current.docs, panes, focused: panes.find(p => p.info().pane_id === current.focused), open: select, action };
  };
  // A pin lands by priority, so as the Mac's pinAtEnd it then moves to the last place, counted
  // on the owning server after the pin rather than from a snapshot that may be behind.
  const pin = (id: string, pinned: boolean) => { void (async () => {
    const name = machine.name;
    await bridge.api(name, "tab.set_pinned", { tab_id: id, pinned });
    if (!pinned) return;
    const pins = pinCount(await bridge.api(name, "tab.list", {}));
    if (pins > 0) await bridge.api(name, "tab.pin_move", { tab_id: id, pin_index: pins - 1 });
  })().catch(showError); };
  const movePin = useCallback((ids: string[], from: number, to: number) => {
    const plan = pinMovePlan(state.current.snapshot, ids, from, to);
    if (!plan) return;
    const at = Date.now();
    setPendingPins(value => ({ ...value, [plan.section]: { order: plan.order, at } }));
    void bridge.api(state.current.machine.name, "tab.pin_move", { tab_id: plan.tab, pin_index: plan.pinIndex }).catch(error => {
      setPendingPins(value => { if (value[plan.section]?.at !== at) return value; const next = { ...value }; delete next[plan.section]; return next; });
      showError(error);
    });
  }, [showError]);
  return <div className="layout">{sidebarVisible && <Sidebar machines={machines} chooseMachine={chooseMachine} rows={rows} selected={selected} revealed={revealed} machine={machine} notice={notice?.text ?? null} select={select} pin={pin} movePin={movePin} renaming={renaming} startRename={id => { setRenaming(id); }} cancelRename={() => setRenaming(null)} commitRename={async (id, label) => { try { await action("rename_tab", label, id); setRenaming(null); const pane = state.current.focused; if (pane) controllers.current.get(pane)?.focus(); } catch { /* runAction reports through the transient status notice. */ } }} />}<TabView snapshot={snapshot} selected={selected} machine={machine.name} focused={switcherOpen || renaming ? null : focused} onFocus={focus} shortcut={shortcut} register={register} pin={pin} />{docsOpen && docsItems.length > 0 && <DocPanel key={docsKey} machine={machine.name} items={docsItems} active={activeDoc} select={name => setDocsActive(value => ({ ...value, [docsKey]: name }))} error={docsError} />}{switcherOpen && <Switcher rows={rows} selected={selected} machine={machine.name} open={select} close={closeSwitcher} />}{!sidebarVisible && (notice || machine.state !== "up") && <div className="machine-error notice" role="status">{notice?.text ?? machine.error ?? machine.state}</div>}</div>;
}
