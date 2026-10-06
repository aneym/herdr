import { useCallback, useRef, useState } from "react";
import PaneTerm from "./PaneTerm";
import type { PaneController } from "./PaneTerm";
import type { Pane } from "./model";
import Chat from "./Chat";
export type PaneMode = "terminal" | "chat";
export default function PaneSurface(props: { pane: Pane; machine: string; focused: boolean; onFocus: (id: string) => void; shortcut: (event: KeyboardEvent) => boolean; register: (id: string, controller: PaneController | null) => void; hasAgent: boolean }) {
  const { pane, machine, hasAgent, register } = props;
  const storageKey = `herdr-shell.mode.${machine}.${pane.pane_id}`;
  const [mode, setMode] = useState<PaneMode>(() => { try { return localStorage.getItem(storageKey) === "chat" ? "chat" : "terminal"; } catch { return "terminal"; } });
  const [mounted, setMounted] = useState(mode === "chat");
  const live = useRef({ mode, hasAgent }); live.current = { mode, hasAgent };
  const count = useRef<number | null>(null);
  const set = useCallback((next: PaneMode) => {
    if (next !== "terminal" && next !== "chat") throw new Error("Mode must be terminal or chat");
    if (next === "chat" && !live.current.hasAgent) throw new Error("No agent in this pane");
    try { localStorage.setItem(storageKey, next); } catch { /* The current view still switches when storage is unavailable. */ }
    if (next === "chat") setMounted(true);
    live.current.mode = next; setMode(next);
  }, [storageKey]);
  const wrappedRegister = useCallback((id: string, controller: PaneController | null) => register(id, controller && { ...controller, chat: async next => {
    set(next);
    if (next === "chat") {
      const deadline = Date.now() + 2200;
      while (count.current === null && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 25));
      if (count.current === null) throw new Error("Chat opened; transcript has not loaded yet");
    }
    return { ok: true, items: count.current ?? 0 };
  }, toggleChat: () => { if (live.current.hasAgent) set(live.current.mode === "chat" ? "terminal" : "chat"); }, focus: () => {
    if (live.current.mode === "terminal" || !live.current.hasAgent) controller.focus();
  } }), [register, set]);
  const onItems = useCallback((items: number) => { count.current = items; }, []);
  const chat = mode === "chat" && hasAgent;
  return <div className="pane-surface" onMouseDown={() => props.onFocus(pane.pane_id)}>
    <div className={`terminal-surface ${chat ? "terminal-hidden" : ""}`} aria-hidden={chat}><PaneTerm {...props} focused={props.focused && !chat} register={wrappedRegister} /></div>
    {mounted && hasAgent && <div className={`chat-surface ${chat ? "" : "chat-hidden"}`}><Chat machine={machine} pane={pane.pane_id} focused={props.focused} visible={chat} onItems={onItems} /></div>}
    {hasAgent && <button className="pane-mode" title="Toggle chat (Ctrl+Shift+M)" onClick={() => set(chat ? "terminal" : "chat")}>{chat ? "Terminal" : "Chat"}</button>}
  </div>;
}
