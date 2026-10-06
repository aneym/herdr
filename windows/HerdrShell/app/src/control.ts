import { bridge } from "./bridge";
import type { MachineStatus } from "./bridge";
import type { SidebarRow } from "./model";
import type { PaneController } from "./PaneTerm";
export interface ControlState { machine: MachineStatus; selected: string | null; rows: SidebarRow[]; panes: PaneController[]; focused: PaneController | undefined; open: (id: string) => void; action: (name: string) => Promise<void> }
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
  watch("ui", async () => { const state = get(); return { ok: true, machine: state.machine, selected_tab: state.selected, rows: state.rows.map(({ kind, id, label, status, hotkey }) => ({ kind, id, label, status, hotkey })), panes: state.panes.map(p => p.info()) }; });
  watch<{ tab_id: string }>("open", async payload => { get().open(payload.tab_id); return { ok: true }; });
  watch<{ key: string }>("key", async payload => ({ ok: true, sent_b64: await focused().key(payload.key) }));
  watch<{ name: string }>("action", async payload => { await get().action(payload.name); return { ok: true }; });
  watch<{ pane_id?: string; mode: "terminal" | "chat" }>("chat", async payload => {
    const pane = payload.pane_id ? get().panes.find(p => p.info().pane_id === payload.pane_id) : focused();
    if (!pane?.chat) throw new Error("Pane unavailable");
    return pane.chat(payload.mode);
  });
  watch<{ dy: number }>("wheel", async payload => { focused().wheel(payload.dy); return { ok: true }; });
  return () => { disposed = true; listeners.forEach(unlisten => unlisten()); };
}
