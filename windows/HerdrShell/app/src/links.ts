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
// Call after term.open: a click xterm does not link (a wrapped URL's continuation rows) is
// resolved from the terminal element.
export function installLinks(term: Terminal, server: LinkServer, open: (url: string) => void): void {
  const claimed = new WeakSet<MouseEvent>();
  const finish = async (cell: { row: number; col: number } | null, resolved: string | null) => {
    const answer = cell ? await server.activate(cell.row, cell.col).catch(() => null) : null;
    const target = openTarget(resolved, answer?.url ?? null, answer?.handled ?? false);
    if (target && webUrl(target)) open(target);
  };
  const activate = (event: MouseEvent, uri: string) => { if (!event.ctrlKey) return; claimed.add(event); void finish(cellAt(term, event), uri); };
  term.options.linkHandler = { activate };
  term.loadAddon(new WebLinksAddon(activate));
  term.element?.addEventListener("mouseup", event => {
    if (!event.ctrlKey || event.button !== 0 || claimed.has(event)) return;
    const cell = cellAt(term, event);
    if (!cell) return;
    void (async () => {
      const regions = await server.resolve(cell.row, cell.col).catch(() => null);
      if (!regions?.some(r => r.row === cell.row && r.start_col <= cell.col && cell.col <= r.end_col)) return;
      const text = spanText(term, regions);
      await finish(cell, webUrl(text) ? text : null);
    })();
  });
}
