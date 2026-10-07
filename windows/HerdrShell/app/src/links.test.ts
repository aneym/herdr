// @vitest-environment happy-dom
import { Terminal } from "@xterm/xterm";
import type { ILink, ILinkProvider } from "@xterm/xterm";
import { afterEach, describe, expect, it, vi } from "vitest";
import { installLinks } from "./links";
// A link reaches the PC as the attach stream's OSC 8 bytes or as plain text. The real xterm
// providers decide which link owns a cell, so this drives them rather than the handler alone.
const terms: Terminal[] = [];
afterEach(() => { terms.splice(0).forEach(term => term.dispose()); vi.restoreAllMocks(); });
async function linksAt(bytes: string, col: number) {
  const term = new Terminal({ cols: 60, rows: 4, allowProposedApi: true });
  terms.push(term);
  const open = vi.fn();
  installLinks(term, open);
  await new Promise<void>(resolve => term.write(bytes, resolve));
  // xterm keeps its provider list (OSC 8 first, then addons) off the public API.
  const providers = (term as unknown as { _core: { _linkProviderService: { linkProviders: ILinkProvider[] } } })._core._linkProviderService.linkProviders;
  const replies = await Promise.all(providers.map(p => new Promise<ILink[] | undefined>(resolve => p.provideLinks(1, resolve))));
  // Linkifier order: the first provider with a link under the pointer wins.
  const link = replies.map(links => links?.find(l => l.range.start.x <= col && col <= l.range.end.x)).find(Boolean);
  return { link, open };
}
const click = (ctrlKey: boolean) => new MouseEvent("mouseup", { ctrlKey });
describe("terminal links", () => {
  // Bytes as src/protocol/render_ansi.rs writes a hyperlinked run.
  const osc8 = "\x1b[1;1H\x1b]8;;https://github.com/aneym/herdr/pull/1\x1b\\PR #1\x1b]8;;\x1b\\ merged";
  it.each([
    ["an OSC 8 hyperlink", osc8, 2, "https://github.com/aneym/herdr/pull/1"],
    ["a plain URL", "see https://example.com/a?b=1 now", 10, "https://example.com/a?b=1"],
  ])("Ctrl-click on %s opens it in the browser without a WebView prompt", async (_name, bytes, col, url) => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    const popup = vi.spyOn(window, "open").mockReturnValue(null);
    const { link, open } = await linksAt(bytes, col);
    expect(link).toBeDefined();
    link!.activate(click(false), link!.text);
    expect(open).not.toHaveBeenCalled();
    link!.activate(click(true), link!.text);
    expect(open).toHaveBeenCalledWith(url);
    expect(confirm).not.toHaveBeenCalled();
    expect(popup).not.toHaveBeenCalled();
  });
});
