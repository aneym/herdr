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
  it("replies with an error for an unavailable explicit pane", async () => {
    expect(await command("link_click", { pane_id: "missing", row: 0, col: 0, ctrl: true })).toEqual({ ok: false, error: "Error: Pane unavailable" });
    expect(await command("copy_selection", { pane_id: "missing", from: { row: 0, col: 0 }, to: { row: 0, col: 1 } })).toEqual({ ok: false, error: "Error: Pane unavailable" });
  });
});
