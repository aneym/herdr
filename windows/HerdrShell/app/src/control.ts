import { showUpdateError } from "./UpdatePill";
import { bridge } from "./bridge";
import type { DocsState } from "./docs";
import type { MachineStatus } from "./bridge";
import type { SidebarRow } from "./model";
import type { Cell } from "./termCopy";
import type { PaneController } from "./PaneTerm";
import { appTheme } from "./theme";
import type { Appearance } from "./theme";
export interface ControlState { machine: MachineStatus; machines: MachineStatus[]; chooseMachine: (name: string) => MachineStatus; selected: string | null; docs: DocsState; rows: SidebarRow[]; panes: PaneController[]; focused: PaneController | undefined; open: (id: string) => void; action: (name: string) => Promise<void> }
export function installControl(get: () => ControlState): () => void {
  let disposed = false;
  const listeners: (() => void)[] = [];
  const watch = <T,>(cmd: string, fn: (payload: T) => Promise<unknown>, read = false, reply = true) => {
    void bridge.controlEvent<T>(cmd, payload => {
      void fn(payload).then(result => {
        if (reply) return read ? bridge.readResult(String(result)) : bridge.controlResult(cmd, result);
      }).catch(error => { if (reply) void (read ? bridge.readResult(`Error: ${String(error)}`) : bridge.controlResult(cmd, { ok: false, error: String(error) })).catch(() => {}); });
    }).then(unlisten => { if (disposed) unlisten(); else listeners.push(unlisten); }).catch(() => {});
  };
  const focused = () => { const pane = get().focused; if (!pane) throw new Error("No focused pane"); return pane; };
  watch<string>("type", async text => { await focused().type(text); }, false, false);
  watch("read", async () => focused().read(), true);
  watch<{ action?: "apply" | "rollback" } | null>("update", async payload => {
    if (payload?.action === "apply" || payload?.action === "rollback") {
      const status = await bridge.updateStatus();
      if (payload.action === "apply" ? !status.available : !status.previous) return { ok: false, error: `nothing to ${payload.action}` };
      // The app exits once the updater starts, so reply first and start it just after.
      const run = payload.action === "apply" ? bridge.updateApply : bridge.updateRollback;
      setTimeout(() => { void run().catch(showUpdateError); }, 300);
      return { ok: true, started: payload.action, status };
    }
    return bridge.updateStatus();
  });
  watch("ui", async () => { const state = get(); return { ok: true, machine: state.machine, machines: state.machines.map(({ name, state }) => ({ name, state })), selected_tab: state.selected, appearance: { override: appTheme().override, mode: appTheme().mode }, docs: state.docs, rows: state.rows.map(({ kind, id, label, status, hotkey }) => ({ kind, id, label, status, hotkey })), panes: state.panes.map(p => p.info()) }; });
  watch<{ name: string }>("machine", async payload => { return { ok: true, machine: get().chooseMachine(payload.name) }; });
  watch<{ tab_id: string }>("open", async payload => { get().open(payload.tab_id); return { ok: true }; });
  watch<{ key: string }>("key", async payload => ({ ok: true, sent_b64: await focused().key(payload.key) }));
  watch<{ name: string }>("action", async payload => { await get().action(payload.name); return { ok: true }; });
  watch<{ pane_id?: string; mode: "terminal" | "chat" }>("chat", async payload => {
    const pane = payload.pane_id ? get().panes.find(p => p.info().pane_id === payload.pane_id) : focused();
    if (!pane?.chat) throw new Error("Pane unavailable");
    return pane.chat(payload.mode);
  });
  // {"cmd":"link_click","pane_id"?:id,"row":n,"col":n,"ctrl":bool}: click a viewport cell without opening a browser.
  watch<{ pane_id?: string; row: number; col: number; ctrl: boolean }>("link_click", async payload => {
    const pane = payload.pane_id ? get().panes.find(p => p.info().pane_id === payload.pane_id) : focused();
    if (!pane) throw new Error("Pane unavailable");
    return { ok: true, opened: await pane.linkClick(payload.row, payload.col, payload.ctrl) };
  });
  // {"cmd":"copy_selection","pane_id"?:id,"from":{"row":n,"col":n},"to":{"row":n,"col":n}}: capture a drag's copy.
  watch<{ pane_id?: string; from: Cell; to: Cell }>("copy_selection", async payload => {
    const pane = payload.pane_id ? get().panes.find(p => p.info().pane_id === payload.pane_id) : focused();
    if (!pane) throw new Error("Pane unavailable");
    return { ok: true, ...await pane.copySelection(payload.from, payload.to) };
  });
  // The window theme (title bar, WebView scheme) is set natively before this runs; the
  // override lasts for this run only, like the Mac shell's --appearance.
  watch<{ mode?: Appearance }>("appearance", async payload => { appTheme().setOverride(payload.mode ?? "system"); return { ok: true, override: appTheme().override, mode: appTheme().mode }; });
  // {"cmd":"drag_pin","tab_id":id,"section":"pinned"|"agent","rows":n|"dy":px,"steps":8,"interval_ms":40,"esc":false}:
  // drag a sidebar pin row from its centre.
  watch<DragPayload & { tab_id: string; section?: "pinned" | "agent"; rows?: number; dy?: number }>("drag_pin", async payload => {
    const row = document.querySelector<HTMLElement>(`[data-row="${payload.section ?? "pinned"}:${CSS.escape(payload.tab_id)}"]`);
    if (!row) throw new Error(`No ${payload.section ?? "pinned"} row ${payload.tab_id}`);
    return pointerDrag(row, 0, payload.dy ?? (payload.rows ?? 0) * row.getBoundingClientRect().height, payload);
  });
  // {"cmd":"drag_divider","split_id":id,"delta":px,"steps":8,"interval_ms":40}: drag a split
  // divider along its axis (right or down positive).
  watch<DragPayload & { split_id: string; delta: number }>("drag_divider", async payload => {
    const strip = document.querySelector<HTMLElement>(`[data-split="${CSS.escape(payload.split_id)}"]`);
    if (!strip) throw new Error(`No divider ${payload.split_id}`);
    const vertical = strip.classList.contains("vertical");
    return pointerDrag(strip, vertical ? payload.delta : 0, vertical ? 0 : payload.delta, payload);
  });
  watch<{ dy: number }>("wheel", async payload => { focused().wheel(payload.dy); return { ok: true }; });
  return () => { disposed = true; listeners.forEach(unlisten => unlisten()); };
}
interface DragPayload { steps?: number; interval_ms?: number; esc?: boolean }
// Press on the centre of `el`, move by (dx, dy) in steps and release, as a physical mouse does:
// events go to the element under each point, and the release clicks the nearest element the
// press and release share, so the app's own handlers tell a drag from a click.
async function pointerDrag(el: HTMLElement, dx: number, dy: number, payload: DragPayload) {
  const box = el.getBoundingClientRect();
  const x = box.left + box.width / 2, y = box.top + box.height / 2;
  const steps = Math.max(1, Math.min(20, payload.steps ?? 8));
  const pause = () => new Promise(resolve => setTimeout(resolve, Math.max(0, Math.min(100, payload.interval_ms ?? 40))));
  const at = (px: number, py: number) => document.elementFromPoint(px, py) ?? document.body;
  const fire = (type: string, target: Element, px: number, py: number) => target.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, composed: true, clientX: px, clientY: py, pointerId: 1, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerup" ? 0 : 1 }));
  const down = at(x, y);
  fire("pointerdown", down, x, y);
  for (let i = 1; i <= steps; i++) { await pause(); fire("pointermove", at(x + dx * i / steps, y + dy * i / steps), x + dx * i / steps, y + dy * i / steps); }
  if (payload.esc) { await pause(); (document.activeElement ?? document.body).dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })); }
  await pause();
  const up = at(x + dx, y + dy);
  fire("pointerup", up, x + dx, y + dy);
  let shared: Element | null = down;
  while (shared && !shared.contains(up)) shared = shared.parentElement;
  shared?.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, composed: true, clientX: x + dx, clientY: y + dy, button: 0 }));
  await new Promise(resolve => requestAnimationFrame(resolve));
  return { ok: true, from: [x, y], to: [x + dx, y + dy] };
}
