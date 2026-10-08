import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { cycleMachine } from "./machines";
import { bridge } from "./bridge";
import { attentionTransition, observeActiveAttention } from "./notify";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow, UserAttentionType } from "@tauri-apps/api/window";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import type { MachineStatus } from "./bridge";
import { buildSidebar, pinCount, selectionAfterClose, tabOrder } from "./model";
import type { SidebarRow, Snapshot } from "./model";
import Sidebar, { useSelectionReveal, useSidebarNavigation } from "./Sidebar";
import { useLaneFiles } from "./laneFiles";
import { useAgentCards } from "./AgentFace";
import Switcher from "./Switcher";
import { moveModeKey, actionFor } from "./keys";
import type { Action } from "./keys";
import { runAction } from "./actions";
import TabView from "./TabView";
import DocPanel, { useDocs } from "./DocPanel";
import { useDesk } from "./docs";
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
import type { PaneDrag, PaneDragState } from "./paneDrag";
interface ViewSelection { selected: string | null; focused: string | null }
export default function App() {
  const [machines, setMachines] = useState<MachineStatus[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [active, setActive] = useState(() => { try { return localStorage.getItem("herdr-shell.machine") || "studio"; } catch { return "studio"; } });
  const latestAttention = useRef<Record<string, string>>({});
  const recordAttention = useCallback((machine: string, tab: string) => { latestAttention.current[machine] = tab; }, []);
  const [snapshots, setSnapshots] = useState<Record<string, Snapshot>>({});
  const selections = useRef(new Map<string, ViewSelection>());
  const [notificationSelection, setNotificationSelection] = useState<{ machine: string; tab: string | null } | null>(null);
  const onSelection = useCallback((machine: string, tab: string | null) => setNotificationSelection(value => value?.machine === machine && value.tab === tab ? value : { machine, tab }), []);
  const [windowFocused, setWindowFocused] = useState(false);
  const attention = useRef(false);
  const updateAttention = useCallback((needsAttention: boolean) => {
    const action = attentionTransition(attention.current, needsAttention);
    attention.current = needsAttention;
    if (action && isTauri()) {
      // Windows uses quiet taskbar attention instead of the Mac's red Dock count.
      void getCurrentWindow().requestUserAttention(action === "request" ? UserAttentionType.Informational : null).catch(error => console.warn("Taskbar attention failed", error));
    }
  }, []);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const window = getCurrentWindow();
    void window.onFocusChanged(({ payload }) => { if (!disposed) setWindowFocused(payload); }).then(fn => { if (disposed) fn(); else unlisten = fn; });
    void window.isFocused().then(focused => { if (!disposed) setWindowFocused(focused); });
    return () => { disposed = true; unlisten?.(); };
  }, []);
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
  return <><AttentionObserver recordAttention={recordAttention} machine={machine} snapshots={snapshots} selected={notificationSelection?.machine === active ? notificationSelection.tab : null} windowFocused={windowFocused} updateAttention={updateAttention} /><MachineView latestAttention={latestAttention} key={active} machine={machine} machines={machines} snapshot={snapshots[active] ?? {}} chooseMachine={chooseMachine} selections={selections.current} control={control} onSelection={onSelection} /></>;
}
function MachineView({ latestAttention, machine, machines, snapshot, chooseMachine, selections, control, onSelection }: { latestAttention: MutableRefObject<Record<string, string>>; machine: MachineStatus; machines: MachineStatus[]; snapshot: Snapshot; chooseMachine: (name: string) => MachineStatus; selections: Map<string, ViewSelection>; control: MutableRefObject<(() => ControlState) | null>; onSelection: (machine: string, tab: string | null) => void }) {
  const [selected, setSelected] = useState<string | null>(selections.get(machine.name)?.selected ?? null);
  useEffect(() => onSelection(machine.name, selected), [machine.name, selected, onSelection]);
  const [focused, setFocused] = useState<string | null>(selections.get(machine.name)?.focused ?? null);
  const savedSelection = useRef({ selected, focused });
  savedSelection.current = { selected, focused };
  useEffect(() => () => { selections.set(machine.name, savedSelection.current); }, [selections, machine.name]);
  const paneDrag = useRef<PaneDrag | null>(null);
  const liftPane = useRef<(() => boolean) | undefined>();
  const [paneDragState, setPaneDragState] = useState<PaneDragState | null>(null);
  const registerDrag = useCallback((drag: PaneDrag | null, lift?: () => boolean) => { paneDrag.current = drag; liftPane.current = lift; }, []);
  const [sidebarVisible, setSidebarVisible] = useState(true);
  const [switcherOpen, setSwitcherOpen] = useState(false);
  const [switcherQuery, setSwitcherQuery] = useState("");
  const [selectQuery, setSelectQuery] = useState(false);
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
  const revealed = useSelectionReveal(selected);
  // A dropped pin order shows until the snapshot agrees or PENDING_LIFETIME_MS passes.
  const [pendingPins, setPendingPins] = useState<PendingOrders>({});
  const catalog = useLaneFiles(machine.name, machine.state === "up");
  const navigation = useSidebarNavigation(snapshot, catalog, selected);
  const navigationRef = useRef(navigation); navigationRef.current = navigation;
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
  const storedDocs = () => { try { return localStorage.getItem(docsKey) === "true"; } catch { return false; } };
  const docsOpen = selected !== null && (docsShown[docsKey] ?? storedDocs());
  const { items: catalogDocs, error: docsError } = useDocs(machine.name, selected, snapshot, docsOpen);
  const showDesk = (tab: string) => {
    const key = `herdr-shell.docs.${machine.name}.${tab}`;
    setDocsShown(value => ({ ...value, [key]: true }));
    try { localStorage.setItem(key, "true"); } catch (error) { showError(error); }
  };
  const { items: docsItems, active: activeDoc, select: selectDoc } = useDesk(machine.name, selected, snapshot, catalogDocs, showDesk);
  useEffect(() => {
    const opened = (event: Event) => { const detail = (event as CustomEvent<{ machine: string; tab: string }>).detail; if (detail.machine === machine.name) showDesk(detail.tab); };
    window.addEventListener("herdr-desk-opened", opened);
    return () => window.removeEventListener("herdr-desk-opened", opened);
  }, [machine.name]);
  const docs: DocsState = { open: docsOpen, items: docsItems.map(item => item.name), active: docsItems.find(item => (item.id ?? item.name) === activeDoc)?.name ?? null };
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
  const previousRows = useRef<SidebarRow[]>([]);
  useEffect(() => {
    const order = tabOrder(rows);
    if (!selected || !order.includes(selected)) {
      const initial = snapshot.tabs?.find(t => t.focused)?.tab_id;
      setRenaming(null);
      setSelected(selectionAfterClose(previousRows.current, rows, selected, initial));
    }
    previousRows.current = rows;
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
    if (name === "move_pane_mode") { liftPane.current?.(); return Promise.resolve(); }
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
      navigation: navigationRef.current, latestAttention: latestAttention.current[current.machine.name],
      api: bridge.api, select, focus, created: (tabId, paneId) => setPending({ tabId, paneId }),
      rename: id => { setSidebarVisible(true); setRenaming(id); }, switcher: mode => { setRenaming(null); setSelectQuery(mode === "goto"); setSwitcherOpen(value => mode === "search" || mode === "goto" ? true : !value); },
      toggleSidebar: () => { setRenaming(null); setSidebarVisible(value => !value); },
      error: showError, label, tabId,
    });
  }, [select, focus, showError, chooseMachine]);
  const closeSwitcher = () => { setSwitcherOpen(false); const id = state.current.focused; if (id) controllers.current.get(id)?.focus(); };
  const shortcut = useCallback((event: KeyboardEvent) => {
    if (event.type !== "keydown") return false;
    const drag = paneDrag.current;
    if (drag?.state.keyboard) {
      const key = moveModeKey(event);
      if (key?.kind === "target") drag.key(key.side, key.edge);
      else if (key?.kind === "drop") drag.release();
      else if (key?.kind === "cancel") drag.cancel();
      return true;
    }
    if (event.key === "Escape" && drag && drag.state.phase !== "idle") { drag.cancel(); return true; }
    if (event.target instanceof Element && event.target.closest("textarea:not(.xterm-helper-textarea)") &&
        !(event.ctrlKey && !event.altKey && !event.metaKey &&
          ((!event.shiftKey && /^[1-9]$/.test(event.key)) || event.key === "Tab" ||
           (event.shiftKey && (["p", "m", "[", "]", "{", "}"].includes(event.key.toLowerCase()) || event.code === "BracketLeft" || event.code === "BracketRight"))))) return false;
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
    return { paneDrag: paneDrag.current, machine: current.machine, machines: current.machines, chooseMachine, selected: current.selected, rows: current.rows, docs: current.docs, panes, focused: panes.find(p => p.info().pane_id === current.focused), open: select, action };
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
  return <div className="layout">{sidebarVisible && <Sidebar navigation={navigation} paneDropRow={paneDragState?.zone?.kind === "into_tab" ? `tab:${paneDragState.zone.tab_id}` : paneDragState?.zone?.kind === "new_tab_in" ? `space:${paneDragState.zone.workspace_id}` : null} snapshot={snapshot} catalog={catalog} machines={machines} chooseMachine={chooseMachine} rows={rows} selected={selected} revealed={revealed} machine={machine} notice={notice?.text ?? null} select={select} pin={pin} movePin={movePin} renaming={renaming} startRename={id => { setRenaming(id); }} cancelRename={() => setRenaming(null)} commitRename={async (id, label) => { try { await action("rename_tab", label, id); setRenaming(null); const pane = state.current.focused; if (pane) controllers.current.get(pane)?.focus(); } catch { /* runAction reports through the transient status notice. */ } }} />}<TabView onSpring={select} online={machine.state === "up"} registerDrag={registerDrag} onDragChange={setPaneDragState} snapshot={snapshot} selected={selected} machine={machine.name} focused={switcherOpen || renaming ? null : focused} onFocus={focus} shortcut={shortcut} register={register} pin={pin} onError={showError} />{docsOpen && docsItems.length > 0 && <DocPanel key={docsKey} machine={machine.name} tab={selected} items={docsItems} active={activeDoc} select={selectDoc} error={docsError} />}{switcherOpen && <Switcher query={switcherQuery} changeQuery={setSwitcherQuery} selectQuery={selectQuery} rows={rows} selected={selected} machine={machine.name} open={select} close={closeSwitcher} />}{!sidebarVisible && (notice || machine.state !== "up") && <div className="machine-error notice" role="status">{notice?.text ?? machine.error ?? machine.state}</div>}</div>;
}

// Only the displayed machine supplies notifications, like the Mac model.snapshot.
function AttentionObserver({ recordAttention, machine, snapshots, selected, windowFocused, updateAttention }: { recordAttention: (machine: string, tab: string) => void; machine: MachineStatus; snapshots: Record<string, Snapshot>; selected: string | null; windowFocused: boolean; updateAttention: (attention: boolean) => void }) {
  const catalog = useLaneFiles(machine.name, machine.state === "up");
  const previous = useRef<{ machine: string; snapshot: Snapshot }>();
  const lastSent = useRef<Record<string, number>>({});
  // Serialize permission prompting and delivery so a burst cannot open multiple prompts.
  const delivery = useRef(Promise.resolve());
  useEffect(() => {
    const snapshot = snapshots[machine.name];
    const now = Date.now();
    const result = observeActiveAttention(previous.current?.machine === machine.name ? previous.current.snapshot : undefined, snapshots, machine.name, selected, windowFocused, new Set(Object.keys(catalog.parked)), Object.fromEntries(Object.entries(lastSent.current).filter(([key]) => key.startsWith(`${machine.name}:`)).map(([key, value]) => [key.slice(machine.name.length + 1), value])), now);
    // Reuse the notifier's transition detector without delivery throttling or foreground suppression.
    const transitions = observeActiveAttention(previous.current?.machine === machine.name ? previous.current.snapshot : undefined, snapshots, machine.name, null, false, new Set(Object.keys(catalog.parked)), {}, now);
    for (const note of transitions.notifications) recordAttention(machine.name, note.tab);
    previous.current = snapshot ? { machine: machine.name, snapshot } : undefined;
    updateAttention(result.attention);
    for (const note of result.notifications) {
      lastSent.current[`${machine.name}:${note.tab}`] = now;
      if (!isTauri()) continue; // Browser checks must not raise a host OS banner.
      delivery.current = delivery.current.then(async () => {
        if (!await isPermissionGranted() && await requestPermission() !== "granted") return;
        // Tauri notification action callbacks are mobile-only; Windows cannot select a tab on click.
        sendNotification({ title: note.title, body: note.body });
      }).catch(error => console.warn("Notification delivery failed", error));
    }
  }, [snapshots, selected, windowFocused, catalog, machine.name, updateAttention, recordAttention]);
  useEffect(() => () => updateAttention(false), [updateAttention]);
  return null;
}
