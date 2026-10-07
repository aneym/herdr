import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebglAddon } from "@xterm/addon-webgl";
import { bridge, fromBase64, toBase64 } from "./bridge";
import { installLinks, RESOLVE_MS } from "./links";
import type { LinkRegion } from "./links";
import type { Mode } from "./bridge";
import type { Pane } from "./model";
import { paste, controlKey, handleKey } from "./keys";
import { PaneCopy, ordered } from "./termCopy";
import type { Cell } from "./termCopy";
import { terminalFont } from "./tokens";
import { Status } from "./Sidebar";
import { appTheme, terminalThemes } from "./theme";
export interface PaneController {
  linkClick: (row: number, col: number, ctrl: boolean) => Promise<string | null>;
  copySelection: (from: Cell, to: Cell) => Promise<{ text: string; copied: boolean }>;
  chat?: (mode: "terminal" | "chat") => Promise<{ ok: boolean; items: number }>;
  toggleChat?: () => void;
  info: () => { pane_id: string; terminal_id: string; mode: "attach" | "observe" | "closed"; cols: number; rows: number; focused: boolean; background?: string };
  type: (text: string) => Promise<void>; read: () => string; key: (key: string) => Promise<string | null>; wheel: (dy: number) => void; focus: () => void;
}
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
    const term = new Terminal({ fontFamily: terminalFont.family, fontSize: terminalFont.size, theme: terminalThemes[appTheme().mode], scrollback: 0, allowProposedApi: true });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.open(host.current);
    const hookOpeners = new WeakMap<MouseEvent, (uri: string) => void>();
    let hookDispatch = false;
    const openTarget = (uri: string) => { void bridge.openUrl(uri).catch(error => setNotice(String(error))); };
    const links = installLinks(term, {
      resolve: async (viewport_row, col) => (await bridge.api(machine, "pane.link.resolve", { pane_id: pane.pane_id, viewport_row, col }) as { regions: LinkRegion[] }).regions,
      activate: async (viewport_row, col) => await bridge.api(machine, "pane.link.activate", { pane_id: pane.pane_id, viewport_row, col }) as { url?: string; handled: boolean },
    }, openTarget, event => hookOpeners.get(event) ?? openTarget);
    const unsubscribeTheme = appTheme().subscribe(mode => { term.options.theme = terminalThemes[mode]; });
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
    const copier = new PaneCopy(term, (method, params) => bridge.api(machine, method, params), () => pane.pane_id, text => bridge.clipboardWrite(text));
    const keyTarget = () => ({ term, mode, send, copier, shortcut: (event: KeyboardEvent) => live.current.shortcut(event) });
    term.attachCustomKeyEventHandler(event => {
      const result = handleKey(event, keyTarget());
      if (result.handled) { event.preventDefault(); result.work?.catch(error); return false; }
      return true;
    });
    const data = term.onData(text => { if (hookDispatch) return; const flush = () => { void send(text).catch(error); }; if (!links.hold(text, flush)) flush(); });
    const binary = term.onBinary(text => { if (hookDispatch) return; const flush = () => { void sendBytes(Uint8Array.from(text, ch => ch.charCodeAt(0))).catch(error); }; if (!links.hold(text, flush)) flush(); });
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
        else if (event.kind === "clipboard") void copier.programWrote(event.b64).catch(error);
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
    const onContext = (event: MouseEvent) => { event.preventDefault(); void (copier.has() ? copier.copy() : paste(term)).catch(error); };
    // The viewport cell under the pointer, clamped to the grid.
    const cellAt = (event: MouseEvent) => {
      const box = host.current?.querySelector(".xterm-screen")?.getBoundingClientRect();
      if (!box || !box.width || !box.height) return null;
      const clamp = (value: number, size: number) => Math.min(size - 1, Math.max(0, Math.floor(value)));
      return { col: clamp((event.clientX - box.left) / (box.width / term.cols), term.cols), row: clamp((event.clientY - box.top) / (box.height / term.rows), term.rows) };
    };
    // Shift makes xterm select even while the program owns the mouse, as Ghostty does.
    const onMove = (event: MouseEvent) => { const cell = cellAt(event); if (cell) copier.move(cell); };
    const onUp = (event: MouseEvent) => { window.removeEventListener("mousemove", onMove, true); window.removeEventListener("mouseup", onUp, true); const cell = cellAt(event); if (cell) copier.release(cell); };
    const onDown = (event: MouseEvent) => {
      if (event.button !== 0) return;
      const cell = cellAt(event);
      if (!cell) return;
      copier.press(cell, event.detail, mode.mouse && !event.shiftKey);
      window.addEventListener("mousemove", onMove, true); window.addEventListener("mouseup", onUp, true);
    };
    // Capture calls share callbacks; serialize them so each reply belongs to its own gesture.
    let hookTail: Promise<unknown> = Promise.resolve();
    const runHook = <T,>(work: () => Promise<T>): Promise<T> => {
      const pending = hookTail.then(() => { if (disposed) throw new Error("Pane unavailable"); return work(); });
      hookTail = pending.catch(() => {});
      return pending;
    };
    const validateCell = (cell: Cell) => {
      if (!Number.isInteger(cell.row) || !Number.isInteger(cell.col) || cell.row < 0 || cell.row >= term.rows || cell.col < 0 || cell.col >= term.cols) throw new Error("Cell outside viewport");
    };
    const controller: PaneController = {
      info: () => ({ pane_id: pane.pane_id, terminal_id: pane.terminal_id, mode: attachMode, cols: term.cols, rows: term.rows, focused: live.current.focused, background: term.options.theme?.background }),
      linkClick: (row, col, ctrl) => runHook(async () => {
        validateCell({ row, col });
        const screen = term.element?.querySelector(".xterm-screen");
        const box = screen?.getBoundingClientRect();
        if (!screen || !box?.width || !box.height) throw new Error("Terminal screen unavailable");
        let opened: string | null = null;
        let hit: () => void = () => {};
        const settled = new Promise<void>(resolve => { hit = resolve; });
        let active = true;
        const captureOpen = (uri: string) => { if (active && !disposed) { opened = uri; hit(); } };
        const dispatch = (type: string, init: MouseEventInit) => {
          const event = new MouseEvent(type, init);
          hookOpeners.set(event, captureOpen);
          hookDispatch = true;
          try { screen.dispatchEvent(event); } finally { hookDispatch = false; }
        };
        let deadline: ReturnType<typeof setTimeout> | undefined;
        try {
          const event = { bubbles: true, cancelable: true, button: 0, ctrlKey: ctrl, clientX: box.left + (col + 0.5) * box.width / term.cols, clientY: box.top + (row + 0.5) * box.height / term.rows };
          // Populate xterm's hover link, just as moving the physical pointer to the cell does.
          dispatch("mousemove", event);
          // A hidden or occluded WebView pauses frames; never wait on one for longer than 50 ms.
          await new Promise<void>(resolve => { requestAnimationFrame(() => resolve()); setTimeout(resolve, 50); });
          if (disposed) throw new Error("Pane unavailable");
          dispatch("mousedown", { ...event, buttons: 1, detail: 1 });
          dispatch("mouseup", { ...event, buttons: 0, detail: 1 });
          if (ctrl) await Promise.race([settled, new Promise<void>(resolve => { deadline = setTimeout(resolve, RESOLVE_MS); })]);
          return opened;
        } finally { active = false; clearTimeout(deadline); }
      }),
      copySelection: (from, to) => runHook(async () => {
        validateCell(from); validateCell(to);
        let text = "";
        const hookCopier = new PaneCopy(term, (method, params) => bridge.api(machine, method, params), () => pane.pane_id, async value => { text = value; });
        try {
          term.clearSelection(); hookCopier.clear();
          if (mode.mouse) { hookCopier.press(from, 1, true); hookCopier.move(to); hookCopier.release(to); }
          else {
            const { start, end } = ordered(from, to);
            term.select(start.col, term.buffer.active.viewportY + start.row, (end.row - start.row) * term.cols + end.col - start.col + 1);
          }
          const copied = await hookCopier.copy();
          return { text, copied };
        } finally { try { term.clearSelection(); } finally { hookCopier.clear(); } }
      }),
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
    node.addEventListener("mousedown", onDown, true);
    const observer = new ResizeObserver(() => { clearTimeout(resizeTimer); resizeTimer = setTimeout(() => { if (disposed) return; fit.fit(); if (handle != null && (term.cols !== sentCols || term.rows !== sentRows)) { sentCols = term.cols; sentRows = term.rows; void bridge.resize(handle, sentCols, sentRows).catch(error); } }, 50); });
    observer.observe(node);
    void open();
    return () => { disposed = true; unsubscribeTheme(); register(pane.pane_id, null); observer.disconnect(); clearTimeout(resizeTimer); clearTimeout(bellTimer); cancelAnimationFrame(wheelFrame); node.removeEventListener("wheel", onWheel, true); node.removeEventListener("contextmenu", onContext); node.removeEventListener("mousedown", onDown, true); window.removeEventListener("mousemove", onMove, true); window.removeEventListener("mouseup", onUp, true); data.dispose(); binary.dispose(); links.dispose(); if (handle != null) void bridge.close(handle).catch(() => {}); term.dispose(); };
  }, [machine, pane.pane_id, pane.terminal_id, register]);
  useEffect(() => { if (focused) host.current?.querySelector<HTMLTextAreaElement>("textarea")?.focus(); }, [focused]);
  return <div ref={element} className={`pane ${bell ? "bell" : ""}`} onMouseDown={() => { live.current.onFocus(pane.pane_id); host.current?.querySelector<HTMLTextAreaElement>("textarea")?.focus(); }}>
    <div className="pane-cap"><Status status={pane.agent_status || "unknown"} /><span className="label">{[pane.agent, pane.terminal_title_stripped || pane.title].filter(Boolean).join(" · ")}</span>{state === "observe" && <button onClick={() => takeover.current()}>Take control</button>}</div>
    <div className="term-host" ref={host} />
    {state === "closed" && <button className="disconnected" title={notice} onClick={() => reconnect.current()}>Disconnected — click to reconnect</button>}
    {notice && state !== "closed" && <div className="notice">{notice}</div>}
  </div>;
}
