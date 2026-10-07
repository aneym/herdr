import { useCallback, useRef, useState } from "react";
import PaneTerm from "./PaneTerm";
import type { PaneController } from "./PaneTerm";
import type { Pane } from "./model";
import Chat from "./Chat";
export type PaneMode = "terminal" | "chat";
/** The tab's pin as a pushpin; the slash marks the click that unpins. */
function PinGlyph({ pinned }: { pinned: boolean }) {
  return <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 2.5h4M7 2.5v4L4.5 9h7L9 6.5v-4M8 9v4.5" />{pinned && <path d="M3 3l10 10" />}</svg>;
}
export default function PaneSurface(props: { pane: Pane; machine: string; focused: boolean; onFocus: (id: string) => void; shortcut: (event: KeyboardEvent) => boolean; register: (id: string, controller: PaneController | null) => void; hasAgent: boolean; pinned?: boolean; onPin?: () => void }) {
  const { pane, machine, hasAgent, register, pinned, onPin } = props;
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
  const tools = (hasAgent ? 1 : 0) + (pinned !== undefined ? 1 : 0);
  return <div className={`pane-surface ${tools ? `tools-${tools}` : ""}`} onMouseDown={() => props.onFocus(pane.pane_id)}>
    <div className={`terminal-surface ${chat ? "terminal-hidden" : ""}`} aria-hidden={chat}><PaneTerm {...props} focused={props.focused && !chat} register={wrappedRegister} /></div>
    {mounted && hasAgent && <div className={`chat-surface ${chat ? "" : "chat-hidden"}`}><Chat machine={machine} pane={pane.pane_id} focused={props.focused} visible={chat} onItems={onItems} /></div>}
    {(hasAgent || pinned !== undefined) && <div className="pane-tools">
      {hasAgent && <button className="pane-mode" title="Toggle chat" onClick={() => set(chat ? "terminal" : "chat")}>{chat ? "Terminal" : "Chat"}</button>}
      {pinned !== undefined && <button className={`pane-pin ${pinned ? "is-pinned" : ""}`} aria-pressed={pinned} aria-label={pinned ? "Unpin this tab" : "Pin this tab"} title={pinned ? "Unpin this chat" : "Pin this chat to the end of Pinned"} onClick={onPin}><PinGlyph pinned={pinned} /></button>}
    </div>}
  </div>;
}
