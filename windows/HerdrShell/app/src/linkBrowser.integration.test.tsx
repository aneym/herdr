// @vitest-environment happy-dom
// Integration: real PaneTerm, xterm providers and DOM gestures to the native
// boundary. Only Tauri I/O and missing browser layout are supplied by the harness.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { Window } from "happy-dom";
import PaneTerm from "./PaneTerm";
import DocPanel from "./DocPanel";
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, Channel: class { onmessage?: (event: unknown) => void } }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let cleanup = () => {};
afterEach(() => { cleanup(); vi.restoreAllMocks(); native.invoke.mockReset(); document.body.innerHTML = ""; });
it("opens OSC 8, detected and server-resolved Ctrl+Shift links externally; Ctrl alone opens the desk", async () => {
  // happy-dom has no layout engine. Supply character measurement and grid geometry,
  // not link providers or their activation callbacks.
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(function (this: HTMLElement) { return this.classList.contains("xterm-char-measure-element") ? 320 : 800; });
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(20);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ x: 0, y: 0, left: 0, top: 0, width: 800, height: 480, right: 800, bottom: 480, toJSON: () => ({}) });
  const style = document.createElement("style"); style.textContent = ".xterm, .xterm-screen { padding: 0px; }"; document.head.append(style);
  let channel: { onmessage: (event: unknown) => void };
  let serverOnly = false;
  native.invoke.mockImplementation(async (cmd: string, args: any) => {
    if (cmd === "attach_open") { channel = args.onEvent; return 1; }
    if (cmd === "api_request" && args.method === "pane.link.resolve") return { regions: serverOnly ? [{ row: 0, start_col: 0, end_col: 3 }] : [] };
    if (cmd === "api_request" && args.method === "pane.link.activate") return { handled: false, url: serverOnly ? "https://example.com/server" : null };
    return undefined;
  });
  const host = document.createElement("main"); host.dataset.deskMachine = "pc"; host.dataset.deskTab = "t1"; document.body.append(host);
  const root = createRoot(host);
  cleanup = () => { act(() => root.unmount()); style.remove(); };
  await act(async () => root.render(<PaneTerm pane={{ pane_id: "p1", terminal_id: "term1", tab_id: "t1", workspace_id: "w1" }} machine="pc" focused={false} onFocus={() => {}} shortcut={() => false} register={() => {}} />));
  await vi.waitFor(() => expect(channel!).toBeDefined());
  await act(async () => { await new Promise(done => setTimeout(done, 300)); });
  const screen = host.querySelector<HTMLElement>(".xterm-screen")!;
  const click = async (shiftKey: boolean) => {
    await act(async () => {
      for (const type of ["mousemove", "mousedown", "mouseup"]) {
        screen.dispatchEvent(new MouseEvent(type, { ctrlKey: true, shiftKey, button: 0, clientX: 15, clientY: 10, bubbles: true }));
        if (type === "mousemove") await new Promise(done => setTimeout(done, 20));
      }
    });
  };
  for (const kind of ["osc", "detected", "server", "file"] as const) {
    serverOnly = kind === "server";
    const url = kind === "file" ? "file:///C:/notes.txt" : `https://example.com/${kind}`;
    const bytes = kind === "osc" || kind === "file" ? `\x1b]8;;${url}\x1b\\link\x1b]8;;\x1b\\` : kind === "server" ? "link" : url;
    await act(async () => { channel!.onmessage({ kind: "bytes", b64: btoa(`\x1bc${bytes}`) }); await new Promise(done => setTimeout(done, 100)); });
    screen.dispatchEvent(new MouseEvent("mousemove", { clientX: 755, clientY: 110, bubbles: true }));
    native.invoke.mockClear();
    await click(true);
    if (kind === "file") {
      // Pane files are on the pane's machine, so Shift still opens them on the desk.
      await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("api_request", { machine: "pc", method: "desk.open", params: { tab_id: "t1", ref: "C:/notes.txt", opened_by: "user" } }));
      expect(native.invoke.mock.calls.filter(([cmd]) => cmd === "open_url")).toHaveLength(0);
    } else {
      await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("open_url", { url }));
      expect(native.invoke.mock.calls.filter(([cmd, args]) => cmd === "api_request" && args.method === "desk.open")).toHaveLength(0);
      expect(native.invoke.mock.calls.filter(([cmd]) => cmd === "open_url")).toHaveLength(1);
    }
    native.invoke.mockClear();
    await click(false);
    await vi.waitFor(() => expect(native.invoke).toHaveBeenCalledWith("api_request", { machine: "pc", method: "desk.open", params: { tab_id: "t1", ref: kind === "file" ? "C:/notes.txt" : url, opened_by: "user" } }));
    expect(native.invoke.mock.calls.filter(([cmd]) => cmd === "open_url")).toHaveLength(0);
  }
});
it("intercepts Ctrl+Shift links in an accessible desk page without navigating it", async () => {
  native.invoke.mockResolvedValue(undefined);
  (window as unknown as Window).happyDOM.settings.disableIframePageLoading = true;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); cleanup = () => act(() => root.unmount());
  await act(async () => root.render(<DocPanel machine="pc" tab="t1" items={[{ name: "Page", kind: "web", url: "https://example.com" }]} active="Page" select={() => {}} error="" />));
  const frame = host.querySelector("iframe")!;
  // Use a local page fixture rather than issuing a real web request.
  (window as unknown as Window).happyDOM.settings.disableIframePageLoading = false;
  frame.src = "about:blank";
  const doc = frame.contentDocument!;
  doc.body.innerHTML = '<a href="https://example.com/out">Web</a>';
  frame.dispatchEvent(new Event("load"));
  for (const anchor of doc.querySelectorAll("a")) {
    const click = new MouseEvent("click", { ctrlKey: true, shiftKey: true, button: 0, bubbles: true, cancelable: true });
    anchor.dispatchEvent(click);
    expect(click.defaultPrevented).toBe(true);
    expect(native.invoke).toHaveBeenCalledWith("open_url", { url: anchor.href });
  }
  expect(native.invoke.mock.calls.filter(([cmd, args]) => cmd === "api_request" && args.method === "desk.open")).toHaveLength(0);
});
