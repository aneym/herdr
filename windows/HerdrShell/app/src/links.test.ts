// @vitest-environment happy-dom
import { Terminal } from "@xterm/xterm";
import type { ILink, ILinkProvider } from "@xterm/xterm";
import { afterEach, describe, expect, it, vi } from "vitest";
import { installLinks, openTarget } from "./links";
import type { LinkRegion, LinkServer } from "./links";
// A link reaches the PC as the attach stream's OSC 8 bytes or as plain text, and the server may
// resolve, refuse or extend it. The real xterm providers decide which link owns a cell, so this
// drives them with a stand-in for the server's two link calls (the network edge).
const COLS = 40, ROWS = 4;
const terms: Terminal[] = [];
afterEach(() => { terms.splice(0).forEach(term => term.dispose()); document.body.innerHTML = ""; vi.restoreAllMocks(); });
async function pane(bytes: string, server: Partial<LinkServer> = {}) {
  const term = new Terminal({ cols: COLS, rows: ROWS, allowProposedApi: true });
  terms.push(term);
  const host = document.createElement("div");
  document.body.append(host);
  term.open(host);
  // happy-dom lays nothing out; give the grid one 10x20 px cell per column and row.
  vi.spyOn(term.element!.querySelector(".xterm-screen")!, "getBoundingClientRect").mockReturnValue({ left: 0, top: 0, width: COLS * 10, height: ROWS * 20, right: COLS * 10, bottom: ROWS * 20, x: 0, y: 0, toJSON: () => ({}) });
  const calls = { resolve: vi.fn(server.resolve ?? (async () => null)), activate: vi.fn(server.activate ?? (async () => null)) };
  const open = vi.fn();
  installLinks(term, calls, open);
  await new Promise<void>(resolve => term.write(bytes, resolve));
  return { term, open, calls };
}
const at = (row: number, col: number, ctrlKey = true) => new MouseEvent("mouseup", { ctrlKey, clientX: col * 10 + 5, clientY: row * 20 + 10, bubbles: true });
// xterm keeps its provider list (OSC 8 first, then addons) off the public API; its Linkifier
// hands the click to the first provider with a link under the pointer.
async function xtermLink(term: Terminal, row: number, col: number): Promise<ILink | undefined> {
  const providers = (term as unknown as { _core: { _linkProviderService: { linkProviders: ILinkProvider[] } } })._core._linkProviderService.linkProviders;
  const replies = await Promise.all(providers.map(p => new Promise<ILink[] | undefined>(resolve => p.provideLinks(row + 1, resolve))));
  const x = col + 1, y = row + 1;
  const under = ({ range: { start, end } }: ILink) => (y > start.y || (y === start.y && x >= start.x)) && (y < end.y || (y === end.y && x <= end.x));
  return replies.map(links => links?.find(under)).find(Boolean);
}
async function click(term: Terminal, row: number, col: number, ctrlKey = true) {
  const event = at(row, col, ctrlKey), link = await xtermLink(term, row, col);
  link?.activate(event, link.text);
  term.element!.querySelector(".xterm-screen")!.dispatchEvent(event);
  await new Promise(resolve => setTimeout(resolve, 0));
  return link;
}
describe("terminal link clicks", () => {
  // Bytes as src/protocol/render_ansi.rs writes a hyperlinked run with a label.
  const osc8 = "\x1b[1;1H\x1b]8;;https://github.com/aneym/herdr/pull/1\x1b\\PR #1\x1b]8;;\x1b\\ merged";
  it("Ctrl-click on an OSC 8 label opens its target without a WebView prompt; a plain click opens nothing", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const popup = vi.spyOn(window, "open").mockReturnValue(null);
    const { term, open, calls } = await pane(osc8);
    expect(await click(term, 0, 2, false)).toBeDefined();
    expect(open).not.toHaveBeenCalled();
    await click(term, 0, 2);
    expect(calls.activate).toHaveBeenCalledWith(0, 2);
    expect(open).toHaveBeenCalledOnce();
    expect(open).toHaveBeenCalledWith("https://github.com/aneym/herdr/pull/1");
    expect(confirm).not.toHaveBeenCalled();
    expect(popup).not.toHaveBeenCalled();
  });
  it("a refused activation opens the URL the click resolved; a plugin that took it opens nothing", async () => {
    const plain = "see https://example.com/a?b=1 now";
    let run = await pane(plain);
    await click(run.term, 0, 10);
    expect(run.open).toHaveBeenCalledWith("https://example.com/a?b=1");
    run = await pane(plain, { activate: async () => ({ url: "https://example.com/a?b=1", handled: true }) });
    await click(run.term, 0, 10);
    expect(run.open).not.toHaveBeenCalled();
  });
  // A 52-column URL in a 40-column pane, drawn as the attach stream draws rows: positioned, not wrapped.
  const long = "https://example.com/" + "a".repeat(32);
  const wrapped = `\x1b[1;1H${long.slice(0, COLS)}\x1b[2;1H${long.slice(COLS)} done`;
  const regions: LinkRegion[] = [{ row: 0, start_col: 0, end_col: COLS - 1 }, { row: 1, start_col: 0, end_col: long.length - COLS - 1 }];
  it("the server's activation extends a URL clipped at the row edge", async () => {
    const { term, open } = await pane(wrapped, { activate: async () => ({ url: long, handled: false }) });
    await click(term, 0, 30);
    expect(open).toHaveBeenCalledWith(long);
  });
  it("Ctrl-click on a wrapped URL's continuation row opens the URL the server's regions spell", async () => {
    const { term, open, calls } = await pane(wrapped, { resolve: async () => regions });
    expect(await click(term, 1, 4)).toBeUndefined();
    expect(calls.resolve).toHaveBeenCalledWith(1, 4);
    expect(open).toHaveBeenCalledWith(long);
  });
  it("a continuation-row click opens nothing when the server has no link there", async () => {
    const { term, open } = await pane(wrapped, { resolve: async () => [] });
    await click(term, 1, 4);
    expect(open).not.toHaveBeenCalled();
  });
});
// Golden policy table: the same cases as macos/HerdrShell/scripts/check_terminal_links.py.
describe("open target policy", () => {
  it.each([
    ["refused activation retains resolved URL", "https://example.com/a", null, false, "https://example.com/a"],
    ["activation cannot replace resolved URL", "https://example.com/a", "https://example.com/b", false, "https://example.com/a"],
    ["activation continuing a viewport-clipped URL supplies the full URL", "https://example.com/abcdefghijklmnopqrst", "https://example.com/abcdefghijklmnopqrstuv", false, "https://example.com/abcdefghijklmnopqrstuv"],
    ["uncached activation supplies full URL", null, "https://example.com/full", false, "https://example.com/full"],
    ["plugin handled opens nothing twice", "https://example.com/a", "https://example.com/a", true, null],
    ["no resolved target and refusal is a miss", null, null, false, null],
  ] as const)("%s", (_name, resolved, activated, handled, expected) => {
    expect(openTarget(resolved, activated, handled)).toBe(expected);
  });
});
