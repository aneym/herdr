import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { bridge, fromBase64, toBase64 } from "./bridge";
import type { Mode } from "./bridge";
import type { Pane } from "./model";
import { copy, paste, controlKey, handleKey } from "./keys";
import { Status } from "./Sidebar";
export interface PaneController {
  chat?: (mode: "terminal" | "chat") => Promise<{ ok: boolean; items: number }>;
  toggleChat?: () => void;
  info: () => { pane_id: string; terminal_id: string; mode: "attach" | "observe" | "closed"; cols: number; rows: number; focused: boolean };
  type: (text: string) => Promise<void>; read: () => string; key: (key: string) => Promise<string | null>; wheel: (dy: number) => void; focus: () => void;
}
const theme = {
  background: "#1e1e2e", foreground: "#cdd6f4", cursor: "#f5e0dc", cursorAccent: "#1e1e2e", selectionBackground: "#45475a",
  black: "#45475a", red: "#f38ba8", green: "#a6e3a1", yellow: "#f9e2af", blue: "#89b4fa", magenta: "#f5c2e7", cyan: "#94e2d5", white: "#bac2de",
  brightBlack: "#585b70", brightRed: "#f38ba8", brightGreen: "#a6e3a1", brightYellow: "#f9e2af", brightBlue: "#89b4fa", brightMagenta: "#f5c2e7", brightCyan: "#94e2d5", brightWhite: "#a6adc8",
};
export default function PaneTerm({ pane, machine, focused, onFocus, shortcut, register }: { pane: Pane; machine: string; focused: boolean; onFocus: (id: string) => void; shortcut: (event: KeyboardEvent) => boolean; register: (id: string, value: PaneController | null) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const element = useRef<HTMLDivElement>(null);
  const live = useRef({ focused, onFocus, shortcut });
  live.current = { focused, onFocus, shortcut };
  const [state, setState] = useState<"attach" | "observe" | "closed">("closed");
  const [notice, setNotice] = useState("");
  const [bell, setBell] = useState(false);
  const reconnect = useRef<() => void>(() => {});
  const takeover = useRef<() => void>(() => {});
  useEffect(() => {
    if (!host.current || !element.current) return;
    const term = new Terminal({ fontFamily: "Cascadia Mono, Consolas, monospace", fontSize: 13, theme, scrollback: 0, allowProposedApi: true });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(new WebLinksAddon((event, uri) => { if (event.ctrlKey) void bridge.openUrl(uri).catch(error => setNotice(String(error))); }));
    term.open(host.current);
    try { const webgl = new WebglAddon(); webgl.onContextLoss(() => webgl.dispose()); term.loadAddon(webgl); } catch { /* DOM renderer remains available without WebGL. */ }
    let disposed = false;
    let handle: number | null = null;
    let mode: Mode = { mouse: false, sgrPixels: false, kittyFlags: 0, modifyOtherKeys: 0 };
    let attachMode: "attach" | "observe" | "closed" = "closed";
    let opening = false;
    let generation = 0;
    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    let bellTimer: ReturnType<typeof setTimeout> | undefined;
    let sentCols = 0, sentRows = 0;
    let capture: string[] | null = null;
    let capturePending: Promise<void>[] | null = null;
    const error = (value: unknown) => { if (!disposed) setNotice(String(value)); };
    const sendBytes = async (bytes: Uint8Array) => {
      if (handle == null || attachMode === "closed") throw new Error("Terminal disconnected");
      const b64 = toBase64(bytes);
      capture?.push(b64);
      const pending = bridge.input(handle, b64);
      capturePending?.push(pending);
      await pending;
    };
    const send = (text: string) => sendBytes(new TextEncoder().encode(text));
    const keyTarget = () => ({ term, mode, send, shortcut: (event: KeyboardEvent) => live.current.shortcut(event) });
    term.attachCustomKeyEventHandler(event => {
      const result = handleKey(event, keyTarget());
      if (result.handled) { event.preventDefault(); result.work?.catch(error); return false; }
      return true;
    });
    const data = term.onData(text => { void send(text).catch(error); });
    const binary = term.onBinary(text => { void sendBytes(Uint8Array.from(text, ch => ch.charCodeAt(0))).catch(error); });
    const open = async () => {
      if (opening || disposed) return;
      opening = true;
      const currentGeneration = ++generation;
      setNotice("");
      if (handle != null) { const old = handle; handle = null; await bridge.close(old).catch(error); }
      term.reset();
      mode = { mouse: false, sgrPixels: false, kittyFlags: 0, modifyOtherKeys: 0 };
      fit.fit();
      const openCols = term.cols, openRows = term.rows;
      let closedDuringOpen = false;
      const onEvent = (event: import("./bridge").AttachEvent) => {
        if (disposed || generation !== currentGeneration) return;
        if (event.kind === "bytes" || event.kind === "mode") { term.write(fromBase64(event.b64)); if (event.kind === "mode") mode = event; }
        else if (event.kind === "bell") { setBell(true); clearTimeout(bellTimer); bellTimer = setTimeout(() => setBell(false), 180); }
        else if (event.kind === "notice") setNotice(event.message);
        else { closedDuringOpen = true; attachMode = "closed"; setState("closed"); setNotice(event.reason); }
      };
      try {
        let next: number;
        let nextMode: "attach" | "observe" = "attach";
        try { next = await bridge.attach(machine, pane.terminal_id, openCols, openRows, "attach", onEvent); }
        catch { if (disposed) return; closedDuringOpen = false; nextMode = "observe"; next = await bridge.attach(machine, pane.terminal_id, openCols, openRows, "observe", onEvent); }
        if (disposed) { await bridge.close(next); return; }
        handle = next;
        attachMode = closedDuringOpen ? "closed" : nextMode;
        setState(attachMode);
        sentCols = openCols; sentRows = openRows;
        if (term.cols !== sentCols || term.rows !== sentRows) { sentCols = term.cols; sentRows = term.rows; await bridge.resize(next, sentCols, sentRows); }
        if (live.current.focused) term.focus();
      } catch (value) { error(value); attachMode = "closed"; if (!disposed) setState("closed"); }
      finally { opening = false; }
    };
    reconnect.current = () => { void open(); };
    takeover.current = () => { if (handle != null) void bridge.takeControl(handle).then(() => { if (!disposed) { attachMode = "attach"; setState("attach"); setNotice(""); } }).catch(error); };
    let pendingWheel = 0;
    let wheelFrame = 0;
    const wheel = (dy: number) => {
      pendingWheel += dy;
      if (wheelFrame) return;
      wheelFrame = requestAnimationFrame(() => {
        wheelFrame = 0;
        const lines = Math.floor(Math.abs(pendingWheel));
        if (lines && handle != null) { const up = pendingWheel < 0; pendingWheel += up ? lines : -lines; void bridge.scroll(handle, up, lines).catch(error); }
      });
    };
    const onWheel = (event: WheelEvent) => { if (mode.mouse) return; event.preventDefault(); event.stopPropagation(); wheel(event.deltaY * (event.deltaMode === 1 ? 1 : event.deltaMode === 2 ? term.rows : 1 / 40)); };
    const onContext = (event: MouseEvent) => { event.preventDefault(); void (term.hasSelection() ? copy(term) : paste(term)).catch(error); };
    const controller: PaneController = {
      info: () => ({ pane_id: pane.pane_id, terminal_id: pane.terminal_id, mode: attachMode, cols: term.cols, rows: term.rows, focused: live.current.focused }),
      type: send,
      read: () => Array.from({ length: term.rows }, (_, i) => term.buffer.active.getLine(term.buffer.active.viewportY + i)?.translateToString(true) ?? "").join("\n"),
      key: async value => {
        capture = [];
        capturePending = [];
        try {
          const event = controlKey(value);
          const result = handleKey(event, keyTarget());
          if (result.handled) await result.work;
          else if (event.ctrlKey && event.key.toLowerCase() === "c") await send("\x03");
          else throw new Error(`Unsupported key: ${value}`);
          await Promise.all(capturePending);
          const bytes = capture.flatMap(b64 => [...fromBase64(b64)]);
          return bytes.length ? toBase64(Uint8Array.from(bytes)) : null;
        } finally { capture = null; capturePending = null; }
      },
      wheel: dy => { if (mode.mouse) host.current?.querySelector(".xterm-viewport")?.dispatchEvent(new WheelEvent("wheel", { deltaY: dy, bubbles: true, cancelable: true })); else wheel(dy / 40); },
      focus: () => term.focus(),
    };
    register(pane.pane_id, controller);
    const node = element.current;
    node.addEventListener("wheel", onWheel, { capture: true, passive: false });
    node.addEventListener("contextmenu", onContext);
    const observer = new ResizeObserver(() => { clearTimeout(resizeTimer); resizeTimer = setTimeout(() => { if (disposed) return; fit.fit(); if (handle != null && (term.cols !== sentCols || term.rows !== sentRows)) { sentCols = term.cols; sentRows = term.rows; void bridge.resize(handle, sentCols, sentRows).catch(error); } }, 50); });
    observer.observe(host.current);
    void open();
    return () => { disposed = true; register(pane.pane_id, null); observer.disconnect(); clearTimeout(resizeTimer); clearTimeout(bellTimer); cancelAnimationFrame(wheelFrame); node.removeEventListener("wheel", onWheel, true); node.removeEventListener("contextmenu", onContext); data.dispose(); binary.dispose(); if (handle != null) void bridge.close(handle).catch(() => {}); term.dispose(); };
  }, [machine, pane.pane_id, pane.terminal_id, register]);
  useEffect(() => { if (focused) host.current?.querySelector<HTMLTextAreaElement>("textarea")?.focus(); }, [focused]);
  return <div ref={element} className={`pane ${bell ? "bell" : ""}`} onMouseDown={() => { live.current.onFocus(pane.pane_id); host.current?.querySelector<HTMLTextAreaElement>("textarea")?.focus(); }}>
    <div className="pane-cap"><Status status={pane.agent_status || "unknown"} /><span className="label">{[pane.agent, pane.terminal_title_stripped || pane.title].filter(Boolean).join(" · ")}</span>{state === "observe" && <button onClick={() => takeover.current()}>Take control</button>}</div>
    <div className="term-host" ref={host} />
    {state === "closed" && <button className="disconnected" title={notice} onClick={() => reconnect.current()}>Disconnected — click to reconnect</button>}
    {notice && state !== "closed" && <div className="notice">{notice}</div>}
  </div>;
}
