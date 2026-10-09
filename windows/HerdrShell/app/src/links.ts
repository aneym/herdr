import { bridge } from "./bridge";
import type { Terminal } from "@xterm/xterm";
import { WebLinksAddon } from "@xterm/addon-web-links";
// Ctrl-click opens a terminal link, as Cmd-click does on the Mac (TerminalLinks.swift). Agents
// print most links as OSC 8 hyperlinks (panes report TERM_PROGRAM=ghostty and the attach stream
// forwards them), and xterm's own OSC 8 provider outranks every addon: without a linkHandler it
// asks window.confirm and calls window.open, which the WebView never hands to the browser.
//
// The attach stream redraws rows with cursor positioning, so xterm sees a soft-wrapped URL as
// unrelated rows: its match stops at the row edge and the continuation rows match nothing. The
// server keeps the pane's wrap-aware screen and answers pane.link.resolve and pane.link.activate
// for a viewport cell; a refused or failed call keeps what the client resolved itself.
export interface LinkRegion { row: number; start_col: number; end_col: number }
export interface LinkServer {
  resolve: (row: number, col: number) => Promise<LinkRegion[] | null>;
  activate: (row: number, col: number) => Promise<{ url?: string | null; handled: boolean } | null>;
}
export const webUrl = (text: string) => { try { const url = new URL(text); return (url.protocol === "http:" || url.protocol === "https:") && !!url.host; } catch { return false; } };
// TerminalLinkDecision.openTarget: the client's target stands unless a plugin took the link or
// the server's target continues it (the client holds only the visible cells).
export function openTarget(resolved: string | null, activated: string | null, handled: boolean): string | null {
  if (handled) return null;
  if (resolved == null) return activated;
  return activated != null && activated.length > resolved.length && activated.startsWith(resolved) ? activated : resolved;
}
export function cellAt(term: Terminal, event: MouseEvent): { row: number; col: number } | null {
  const rect = term.element?.querySelector(".xterm-screen")?.getBoundingClientRect();
  if (!rect || !rect.width || !rect.height) return null;
  const col = Math.floor((event.clientX - rect.left) / (rect.width / term.cols)), row = Math.floor((event.clientY - rect.top) / (rect.height / term.rows));
  return col >= 0 && row >= 0 && col < term.cols && row < term.rows ? { row, col } : null;
}
const spanText = (term: Terminal, regions: LinkRegion[]) => [...regions].sort((a, b) => a.row - b.row || a.start_col - b.start_col)
  .map(r => term.buffer.active.getLine(term.buffer.active.viewportY + r.row)?.translateToString(false, r.start_col, r.end_col + 1) ?? "").join("");
// One Ctrl+left press and its release on the same cell. Mouse reports the program would get
// for it are held until the link settles: a hit consumes them, a miss or a drag replays them,
// as the Mac buffers a native gesture and replays it only on a miss. Reports that arrive while
// an earlier click resolves queue behind it, so the program sees them in order.
interface Gesture { cell: { row: number; col: number }; open: (url: string) => void; settled?: boolean; hit?: boolean }
export interface LinkGate { hold: (data: string, flush: () => void) => boolean; dispose: () => void }
export const RESOLVE_MS = 1500;
const mouseReport = (data: string) => data.startsWith("\x1b[<") || data.startsWith("\x1b[M");
// Call after term.open. A click xterm does not link (a wrapped URL's continuation rows) is
// resolved through the server from the terminal's own mouseup.
export function installLinks(term: Terminal, server: LinkServer, open: (url: string) => void, gestureOpen: (event: MouseEvent) => ((url: string) => void) = () => open): LinkGate {
  let disposed = false;
  const linked = new WeakMap<MouseEvent, string>();
  // The open gesture: pressed, or released this event turn. Only it holds reports and only it
  // can be cancelled; a released click is settled once, by its resolution or, if the server is
  // slow, as a miss at the deadline so the reports queued behind it are not held.
  let gesture: Gesture | null = null;
  const queue: { g: Gesture | null; flush: () => void }[] = [];
  const settle = (g: Gesture, hit: boolean) => {
    if (gesture === g) gesture = null;
    if (g.settled) return;
    g.settled = true;
    g.hit = hit;
    while (queue.length && (!queue[0].g || queue[0].g.settled)) { const entry = queue.shift()!; if (!entry.g?.hit) entry.flush(); }
  };
  const finish = async (g: Gesture, cell: { row: number; col: number }, resolved: string | null) => {
    // Activation runs server-side link plugins; a click already replayed must not reach them.
    if (disposed || g.settled) return false;
    const answer = await server.activate(cell.row, cell.col).catch(() => null);
    const target = openTarget(resolved, answer?.url ?? null, answer?.handled ?? false);
    if (disposed || g.settled) return false;
    if (target && /^(https?:|file:|mailto:)/i.test(target)) { g.open(target); return true; }
    return answer?.handled ?? false;
  };
  const resolve = async (g: Gesture, cell: { row: number; col: number }) => {
    const regions = await server.resolve(cell.row, cell.col).catch(() => null);
    if (!regions?.some(r => r.row === cell.row && r.start_col <= cell.col && cell.col <= r.end_col)) return false;
    const text = spanText(term, regions);
    return finish(g, cell, webUrl(text) ? text : null);
  };
  // xterm activates on a release anywhere in the pressed link; the gesture decides in `up`.
  const activate = (event: MouseEvent, uri: string) => { if (event.ctrlKey) linked.set(event, uri); };
  // xterm otherwise drops file/mailto OSC 8 links before activate; finish keeps
  // the allowed-scheme gate, so arbitrary OSC URI schemes still cannot open.
  term.options.linkHandler = { activate, allowNonHttpProtocols: true };
  term.loadAddon(new WebLinksAddon(activate));
  const screen = term.element?.querySelector(".xterm-screen");
  // Registered after xterm's Linkifier on the same element, so its activation is already known.
  const down = (event: Event) => {
    const mouse = event as MouseEvent, cell = mouse.ctrlKey && mouse.button === 0 ? cellAt(term, mouse) : null;
    if (cell) {
      const external = gestureOpen(mouse);
      const context = term.element?.closest<HTMLElement>("[data-desk-machine][data-desk-tab]");
      const machine = context?.dataset.deskMachine, tab = context?.dataset.deskTab;
      const pane = term.element?.closest<HTMLElement>("[data-pane]")?.dataset.pane;
      const shift = mouse.shiftKey;
      gesture = { cell, open: url => {
        if (!machine || !tab || shift || /^mailto:/i.test(url)) { external(url); return; }
        let ref = url;
        if (/^file:/i.test(url)) { try { const parsed = new URL(url); ref = decodeURIComponent(parsed.pathname); if (/^\/[a-z]:/i.test(ref)) ref = ref.slice(1); } catch { external(url); return; } }
        void bridge.api(machine, "desk.open", { ...(pane ? { pane_id: pane } : { tab_id: tab }), ref, opened_by: "user" }).then(() => {
          window.dispatchEvent(new CustomEvent("herdr-desk-opened", { detail: { machine, tab } }));
        }).catch(() => external(url));
      } };
    }
  };
  const up = (event: Event) => {
    const g = gesture, mouse = event as MouseEvent, cell = cellAt(term, mouse);
    if (!g || mouse.button !== 0) return;
    if (!mouse.ctrlKey || !cell || cell.row !== g.cell.row || cell.col !== g.cell.col) { settle(g, false); return; }
    // xterm reports the release from the document after this listener; close the hold after it.
    setTimeout(() => { if (gesture === g) gesture = null; }, 0);
    const uri = linked.get(mouse);
    setTimeout(() => settle(g, false), RESOLVE_MS);
    void (uri != null ? finish(g, cell, uri) : resolve(g, cell)).then(hit => settle(g, hit), () => settle(g, false));
  };
  // A release outside the grid ends the gesture before xterm reports it from the document; a
  // window that loses focus or is hidden, or a cancelled pointer, may never see the release at all.
  const away = (event: Event) => { if (gesture && !(screen && event.target instanceof Node && screen.contains(event.target))) settle(gesture, false); };
  const cancel = () => { if (gesture) settle(gesture, false); };
  screen?.addEventListener("mousedown", down);
  screen?.addEventListener("mouseup", up);
  window.addEventListener("mouseup", away, true);
  window.addEventListener("blur", cancel);
  window.addEventListener("pointercancel", cancel, true);
  document.addEventListener("visibilitychange", cancel);
  return {
    hold: (data, flush) => { if (!mouseReport(data) || (!gesture && !queue.length)) return false; queue.push({ g: gesture, flush }); return true; },
    dispose: () => { disposed = true; screen?.removeEventListener("mousedown", down); screen?.removeEventListener("mouseup", up); window.removeEventListener("mouseup", away, true); window.removeEventListener("blur", cancel); window.removeEventListener("pointercancel", cancel, true); document.removeEventListener("visibilitychange", cancel); },
  };
}
