import { useCallback, useEffect, useRef, useState } from "react";
import PaneTerm from "./PaneTerm";
import type { PaneController } from "./PaneTerm";
import type { Pane } from "./model";
import Chat from "./Chat";
import { bridge } from "./bridge";
export type PaneMode = "terminal" | "chat";
/** The tab's pin as a pushpin; the slash marks the click that unpins. */
function PinGlyph({ pinned }: { pinned: boolean }) {
  return <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 2.5h4M7 2.5v4L4.5 9h7L9 6.5v-4M8 9v4.5" />{pinned && <path d="M3 3l10 10" />}</svg>;
}
export default function PaneSurface(props: { pane: Pane; machine: string; focused: boolean; onFocus: (id: string) => void; shortcut: (event: KeyboardEvent) => boolean; register: (id: string, controller: PaneController | null) => void; hasAgent: boolean; pinned?: boolean; onPin?: () => void; onError?: (error: unknown) => void }) {
  const { pane, machine, hasAgent, register, pinned, onPin } = props;
  const toolsRef = useRef<HTMLDivElement>(null);
  const [menu, setMenu] = useState(false);
  const [confirmRestart, setConfirmRestart] = useState(false);
  const [restarting, setRestarting] = useState(false);
  const restart = async (force = false) => {
    if (restarting) return;
    setMenu(false); setConfirmRestart(false); setRestarting(true);
    try {
      await bridge.api(machine, "agent.restart", { pane_id: pane.pane_id, ...(force ? { force: true } : {}) });
    } catch (error) {
      // The native bridge preserves server codes in its error text.
      const value = error as { code?: string; message?: string };
      const busy = value?.code === "busy" || String(error).startsWith("herdr api error busy:");
      if (busy && !force) setConfirmRestart(true);
      else props.onError?.(value?.message ?? error);
    } finally { setRestarting(false); }
  };
  useEffect(() => {
    if (!menu) return;
    const close = (event: MouseEvent) => { if (!toolsRef.current?.contains(event.target as Node)) setMenu(false); };
    document.addEventListener("mousedown", close);
    toolsRef.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
    return () => document.removeEventListener("mousedown", close);
  }, [menu]);
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
  const tools = 1 + (hasAgent ? 1 : 0) + (pinned !== undefined ? 1 : 0);
  return <div className={`pane-surface ${tools ? `tools-${tools}` : ""}`} onMouseDown={() => props.onFocus(pane.pane_id)}>
    <div className={`terminal-surface ${chat ? "terminal-hidden" : ""}`} aria-hidden={chat}><PaneTerm {...props} focused={props.focused && !chat} register={wrappedRegister} /></div>
    {mounted && hasAgent && <div className={`chat-surface ${chat ? "" : "chat-hidden"}`}><Chat machine={machine} pane={pane.pane_id} focused={props.focused} visible={chat} onItems={onItems} /></div>}
    <div className="pane-tools" ref={toolsRef}>
      {hasAgent && <button className="pane-mode" title="Toggle chat" onClick={() => set(chat ? "terminal" : "chat")}>{chat ? "Terminal" : "Chat"}</button>}
      {pinned !== undefined && <button className={`pane-pin ${pinned ? "is-pinned" : ""}`} aria-pressed={pinned} aria-label={pinned ? "Unpin this tab" : "Pin this tab"} title={pinned ? "Unpin this chat" : "Pin this chat to the end of Pinned"} onClick={onPin}><PinGlyph pinned={pinned} /></button>}
      <button className="pane-pin" aria-label="Pane actions" aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu(value => !value)} onKeyDown={event => { if (event.key === "Escape") setMenu(false); }}><svg viewBox="0 0 16 16" aria-hidden="true"><circle cx="3" cy="8" r=".7" /><circle cx="8" cy="8" r=".7" /><circle cx="13" cy="8" r=".7" /></svg></button>
      {menu && <div className="pane-menu" role="menu" onKeyDown={event => { if (event.key === "Escape") setMenu(false); }}><button role="menuitem" disabled={!hasAgent || restarting} onClick={() => void restart()}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M12.5 5.5A5 5 0 1 0 13 9M12.5 2v3.5H9" /></svg>Restart agent</button></div>}
    </div>
    {confirmRestart && <div className="pane-menu restart-confirm" role="dialog" aria-label="Restart agent"><p>Agent is working. Restart anyway? It will resume the same chat.</p><button onClick={() => void restart(true)}>Restart</button><button onClick={() => setConfirmRestart(false)}>Cancel</button></div>}
  </div>;
}
