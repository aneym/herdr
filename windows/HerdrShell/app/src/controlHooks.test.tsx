// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { bridge, utf8Base64 } from "./bridge";
import type { AttachEvent } from "./bridge";
import type { PaneController } from "./PaneTerm";
import type { ControlState } from "./control";

const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => void stored.set(key, value) } });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const { default: PaneTerm } = await import("./PaneTerm");
const { installControl } = await import("./control");

// The control pipe's reply must come from the mounted pane, not a mock controller.
// Missing handlers produce no reply; missing captures open the browser or write the clipboard.
// Existing links tests do not traverse control delivery or PaneTerm's callback routing.
describe("control-pipe terminal gestures", () => {
  let host: HTMLDivElement, root: Root, stop: () => void;
  const panes = new Map<string, PaneController>();
  const events = new Map<string, (payload: unknown) => void>();
  const streams = new Map<string, (event: AttachEvent) => void>();
  const register = (id: string, pane: PaneController | null) => { if (pane) panes.set(id, pane); else panes.delete(id); };
  const result = vi.fn(async (_cmd: string, _value: unknown) => {});
  const rect = (width: number, height: number): DOMRect => ({ left: 0, top: 0, width, height, right: width, bottom: height, x: 0, y: 0, toJSON: () => ({}) });
  beforeEach(async () => {
    events.clear(); streams.clear(); panes.clear(); result.mockClear();
    // happy-dom has no layout. Supply font/grid geometry, not xterm's mouse or link logic.
    vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(function (this: HTMLElement) { return this.classList.contains("xterm-char-measure-element") ? 320 : 800; });
    vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (this: HTMLElement) { return this.classList.contains("xterm-char-measure-element") ? 20 : 480; });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) { return rect(this.offsetWidth, this.offsetHeight); });
    const style = document.createElement("style"); style.textContent = ".xterm-screen { padding: 0px; }"; document.head.append(style);
    vi.spyOn(bridge, "controlEvent").mockImplementation(async (cmd, fn) => { events.set(cmd, fn as (payload: unknown) => void); return () => { events.delete(cmd); }; });
    vi.spyOn(bridge, "controlResult").mockImplementation(result);
    vi.spyOn(bridge, "openUrl").mockResolvedValue();
    vi.spyOn(bridge, "clipboardWrite").mockResolvedValue();
    vi.spyOn(bridge, "input").mockResolvedValue();
    vi.spyOn(bridge, "resize").mockResolvedValue();
    vi.spyOn(bridge, "close").mockResolvedValue();
    vi.spyOn(bridge, "attach").mockImplementation(async (_machine, id, _cols, _rows, _mode, fn) => { streams.set(id, fn); return streams.size; });
    vi.spyOn(bridge, "api").mockResolvedValue({});
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
    await act(async () => root.render(<>{["focused", "other"].map(id => <PaneTerm key={id} pane={{ pane_id: id, terminal_id: id, workspace_id: "w", tab_id: "t" }} machine="studio" focused={id === "focused"} onFocus={() => {}} shortcut={() => false} register={register} />)}</>));
    stop = installControl(() => ({ machine: { name: "studio", state: "up" }, machines: [], selected: "t", docs: { open: false, items: [], active: null }, rows: [], panes: [...panes.values()], focused: panes.get("focused"), chooseMachine: () => ({ name: "studio", state: "up" }), open: () => {}, action: async () => {} } satisfies ControlState));
    await write("focused", "https://example.com");
    await write("other", "other pane");
  });
  afterEach(() => { stop?.(); act(() => root.unmount()); host.remove(); document.head.querySelector("style")?.remove(); vi.restoreAllMocks(); });
  async function write(id: string, text: string, mouse = false) {
    await act(async () => streams.get(id)!({ kind: "mode", b64: utf8Base64(`\x1b[H${text}`), mouse, sgrPixels: false, kittyFlags: 0, modifyOtherKeys: 0 }));
    await vi.waitFor(() => expect(panes.get(id)?.read()).toContain(text));
  }
  async function command(cmd: string, payload: unknown) {
    result.mockClear();
    await act(async () => {
      events.get(cmd)?.(payload);
      await vi.waitFor(() => expect(result).toHaveBeenCalled(), { timeout: 2500 });
    });
    expect(result.mock.calls[0][0]).toBe(cmd);
    return result.mock.calls[0][1];
  }
  it("Ctrl-click uses real xterm web links and captures the opener; plain click opens nothing", async () => {
    // No server link regions or URL: the URL must come from the real WebLinksAddon.
    expect(await command("link_click", { row: 0, col: 4, ctrl: true })).toEqual({ ok: true, opened: "https://example.com" });
    expect(await command("link_click", { pane_id: "focused", row: 0, col: 4, ctrl: false })).toEqual({ ok: true, opened: null });
    expect(bridge.openUrl).not.toHaveBeenCalled();
    expect(bridge.api).toHaveBeenCalledWith("studio", "pane.link.activate", { pane_id: "focused", viewport_row: 0, col: 4 });
  });
  it("copies the addressed or focused pane through xterm and program-owned selections without clipboard writes", async () => {
    const selection = { from: { row: 0, col: 0 }, to: { row: 0, col: 4 } };
    expect(await command("copy_selection", { pane_id: "other", ...selection })).toEqual({ ok: true, text: "other", copied: true });
    expect(await command("copy_selection", selection)).toEqual({ ok: true, text: "https", copied: true });
    await write("other", "owned drag", true);
    expect(await command("copy_selection", { pane_id: "other", ...selection })).toEqual({ ok: true, text: "owned", copied: true });
    expect(await command("copy_selection", { pane_id: "other", from: selection.from, to: selection.from })).toEqual({ ok: true, text: "", copied: false });
    expect(bridge.clipboardWrite).not.toHaveBeenCalled();
    // Clearing native and shadow selections makes normal Ctrl+C interrupt afterward.
    expect(await panes.get("other")!.key("ctrl+c")).toBe(utf8Base64("\x03"));
  });
  // Integration regressions: real xterm gestures and bridge-edge effects expose isolation failures.
  const deferred = <T,>() => {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>(done => { resolve = done; });
    return { promise, resolve };
  };
  function click(col: number, ctrl = true) {
    const screen = host.querySelector(".xterm-screen")!;
    const { cols, rows } = panes.get("focused")!.info();
    const event = { bubbles: true, cancelable: true, button: 0, ctrlKey: ctrl, clientX: (col + 0.5) * 800 / cols, clientY: 240 / rows, detail: 1 };
    screen.dispatchEvent(new MouseEvent("mousedown", { ...event, buttons: 1 }));
    screen.dispatchEvent(new MouseEvent("mouseup", { ...event, buttons: 0 }));
  }
  it("drops hook mouse reports but preserves real reports with DECSET reporting enabled", async () => {
    await act(async () => streams.get("focused")!({ kind: "mode", b64: utf8Base64("\x1b[?1000h\x1b[?1006h"), mouse: true, sgrPixels: false, kittyFlags: 0, modifyOtherKeys: 0 }));
    await new Promise(resolve => setTimeout(resolve, 30));
    expect(await command("link_click", { row: 0, col: 40, ctrl: false })).toEqual({ ok: true, opened: null });
    expect(await command("link_click", { row: 0, col: 40, ctrl: true })).toEqual({ ok: true, opened: null });
    expect(bridge.input).not.toHaveBeenCalled();
    click(40, false);
    await vi.waitFor(() => expect(bridge.input).toHaveBeenCalled());
  });
  it("keeps a real click separate while the hook activation is pending", async () => {
    const activation = deferred<{ url: string; handled: boolean }>();
    vi.mocked(bridge.api).mockImplementation(async (_machine, method, params) => {
      if (method === "pane.link.activate") return (params as { col: number }).col === 4 ? activation.promise : { url: "https://real.example", handled: false };
      if (method === "pane.link.resolve") return { regions: [{ row: 0, start_col: 30, end_col: 45 }] };
      return {};
    });
    const hook = panes.get("focused")!.linkClick(0, 4, true);
    await vi.waitFor(() => expect(bridge.api).toHaveBeenCalledWith("studio", "pane.link.activate", { pane_id: "focused", viewport_row: 0, col: 4 }));
    click(30);
    await vi.waitFor(() => expect(bridge.openUrl).toHaveBeenCalledWith("https://real.example"));
    activation.resolve({ url: "https://example.com/hook", handled: false });
    expect(await hook).toBe("https://example.com/hook");
    expect(bridge.openUrl).toHaveBeenCalledTimes(1);
  });
  it.each([false, true])("never opens a late hook activation after timeout (unmount=%s)", async unmount => {
    const activation = deferred<{ url: string; handled: boolean }>();
    vi.mocked(bridge.api).mockImplementation(async (_machine, method) => method === "pane.link.activate" ? activation.promise : {});
    const hook = panes.get("focused")!.linkClick(0, 4, true);
    await vi.waitFor(() => expect(bridge.api).toHaveBeenCalledWith("studio", "pane.link.activate", { pane_id: "focused", viewport_row: 0, col: 4 }));
    if (unmount) {
      vi.mocked(bridge.api).mockImplementation(async (_machine, method) => method === "pane.link.resolve" ? { regions: [{ row: 0, start_col: 30, end_col: 45 }] } : { url: "https://real.example", handled: false });
      click(30);
      await vi.waitFor(() => expect(bridge.openUrl).toHaveBeenCalledWith("https://real.example"));
      await act(async () => root.unmount());
      activation.resolve({ url: "https://late.example", handled: false });
    }
    expect(await hook).toBeNull();
    activation.resolve({ url: "https://late.example", handled: false });
    await new Promise(resolve => setTimeout(resolve, 20));
    expect(bridge.openUrl).toHaveBeenCalledTimes(unmount ? 1 : 0);
  });
  it("rejects a hook disposed during hover setup without dispatching the click", async () => {
    const frame = vi.spyOn(window, "requestAnimationFrame").mockReturnValue(1);
    const controller = panes.get("focused")!;
    const screen = host.querySelector(".xterm-screen")!;
    const down = vi.fn();
    screen.addEventListener("mousedown", down);
    const hook = controller.linkClick(0, 4, true);
    const rejected = expect(hook).rejects.toThrow("Pane unavailable");
    await act(async () => { await Promise.resolve(); root.unmount(); });
    await rejected;
    frame.mockRestore();
    expect(down).not.toHaveBeenCalled();
    expect(bridge.openUrl).not.toHaveBeenCalled();
    expect(bridge.input).not.toHaveBeenCalled();
  });
  it("keeps real OSC 52 writes separate from a pending selection copy", async () => {
    const selection = deferred<{ text: string }>();
    vi.mocked(bridge.api).mockImplementation(async (_machine, method) => method === "pane.selection.read" ? selection.promise : {});
    const hook = panes.get("focused")!.copySelection({ row: 0, col: 0 }, { row: 0, col: 4 });
    await vi.waitFor(() => expect(bridge.api).toHaveBeenCalledWith("studio", "pane.selection.read", expect.anything()));
    await act(async () => streams.get("focused")!({ kind: "clipboard", b64: utf8Base64("real program copy") }));
    expect(bridge.clipboardWrite).toHaveBeenCalledWith("real program copy");
    selection.resolve({ text: "hook copy" });
    expect(await hook).toEqual({ text: "hook copy", copied: true });
    expect(bridge.clipboardWrite).toHaveBeenCalledTimes(1);
  });
  it("restores normal clipboard behavior after a selection setup exception", async () => {
    // Leave a real native selection in place so the hook's initial clear refreshes it.
    const screen = host.querySelector(".xterm-screen")!;
    const { cols, rows } = panes.get("focused")!.info();
    const event = { bubbles: true, cancelable: true, button: 0, buttons: 1, clientX: 0.5 * 800 / cols, clientY: 240 / rows, detail: 1 };
    screen.dispatchEvent(new MouseEvent("mousedown", event));
    document.dispatchEvent(new MouseEvent("mousemove", { ...event, clientX: 5.5 * 800 / cols }));
    document.dispatchEvent(new MouseEvent("mouseup", { ...event, buttons: 0, clientX: 5.5 * 800 / cols }));
    await new Promise(resolve => setTimeout(resolve, 30));
    const frame = vi.spyOn(window, "requestAnimationFrame").mockImplementation(() => { throw new Error("selection refresh failed"); });
    await expect(panes.get("focused")!.copySelection({ row: 0, col: 0 }, { row: 0, col: 4 })).rejects.toThrow("selection refresh failed");
    frame.mockRestore();
    await act(async () => streams.get("focused")!({ kind: "clipboard", b64: utf8Base64("after failure") }));
    expect(bridge.clipboardWrite).toHaveBeenCalledWith("after failure");
    expect(await panes.get("focused")!.copySelection({ row: 0, col: 0 }, { row: 0, col: 4 })).toEqual({ text: "https", copied: true });
  });
  it("replies with an error for an unavailable explicit pane", async () => {
    expect(await command("link_click", { pane_id: "missing", row: 0, col: 0, ctrl: true })).toEqual({ ok: false, error: "Error: Pane unavailable" });
    expect(await command("copy_selection", { pane_id: "missing", from: { row: 0, col: 0 }, to: { row: 0, col: 1 } })).toEqual({ ok: false, error: "Error: Pane unavailable" });
  });
});
