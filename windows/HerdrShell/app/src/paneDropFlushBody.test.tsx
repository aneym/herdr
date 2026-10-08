// @vitest-environment happy-dom
// S10e review regression: exercise the real TabView/PaneSurface/PaneDrag pointer path.
// Only the external Tauri bridge and the WebGL terminal renderer are faked.
import { afterEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { Layout, Rect, Snapshot } from "./model";
const api = vi.hoisted(() => vi.fn());
vi.mock("./bridge", () => ({ bridge: { api: (...args: unknown[]) => api(...args) } }));
vi.mock("./PaneTerm", () => ({ default: (props: { pane: { pane_id: string }; onCapPointerDown?: (event: unknown) => void }) => <div data-cap={props.pane.pane_id} onPointerDown={event => props.onCapPointerDown?.(event)} /> }));
const { default: TabView } = await import("./TabView");
let root: Root | undefined;
let host: HTMLDivElement;
afterEach(() => {
  act(() => root?.unmount()); host?.remove();
  document.documentElement.style.removeProperty("--shell-space-pane-cap-height");
  vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks();
});

it("focuses C without sending a drop when flushing a pending swap moves its old cap press into its body", async () => {
  vi.useFakeTimers();
  api.mockImplementation((_machine: string, method: string, params: { dry_run?: boolean }) => {
    if (method === "pane.place" && params.dry_run) return Promise.resolve({ place: { changed: false, reason: "same_pane" } });
    return new Promise(() => {}); // Hold the A/B swap reply throughout the second press and release.
  });
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => setTimeout(() => callback(0), 16));
  vi.stubGlobal("cancelAnimationFrame", (id: number) => clearTimeout(id));
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {} });
  vi.stubGlobal("ResizeObserver", class {
    constructor(private callback: (entries: { contentRect: { width: number; height: number } }[]) => void) {}
    observe() { this.callback([{ contentRect: { width: 1000, height: 800 } }]); }
    disconnect() {}
  });
  Element.prototype.animate = vi.fn(() => ({}) as Animation);
  document.documentElement.style.setProperty("--shell-space-pane-cap-height", "36px");
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  const focus = vi.fn();
  const r = (x: number, y: number, width: number, height: number): Rect => ({ x, y, width, height });
  const render = (cTop: number) => {
    const layout: Layout = { tab_id: "t", area: r(0, 0, 100, 40), panes: [
      { pane_id: "a", rect: r(0, 0, 50, 40) },
      { pane_id: "b", rect: r(50, 0, 50, cTop) },
      { pane_id: "c", rect: r(50, cTop, 50, 40 - cTop) },
    ] };
    const snapshot = { layouts: [layout], tabs: [], panes: layout.panes.map(p => ({ pane_id: p.pane_id, terminal_id: "term-" + p.pane_id, tab_id: "t", workspace_id: "w" })) } as Snapshot;
    act(() => root!.render(<TabView snapshot={snapshot} selected="t" machine="studio" focused={null} onFocus={focus} shortcut={() => false} register={() => {}} pin={() => {}} />));
  };
  const fire = (target: EventTarget, type: string, init: PointerEventInit) => act(() => { target.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 1, ...init })); });
  const tick = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });
  render(20); await tick(0);
  fire(host.querySelector('[data-cap="a"]')!, "pointerdown", { button: 0, buttons: 1, clientX: 200, clientY: 10 });
  fire(window, "pointermove", { button: -1, buttons: 1, clientX: 300, clientY: 400 });
  fire(window, "pointermove", { button: -1, buttons: 1, clientX: 750, clientY: 200 });
  await tick(0);
  fire(window, "pointerup", { button: 0, buttons: 0, clientX: 750, clientY: 200 });
  expect(api).toHaveBeenCalledWith("studio", "pane.swap", { source_pane_id: "a", target_pane_id: "b" });
  render(15); await tick(40); // C's top moves from 400 to 300 px, buffered until the press.
  expect(host.querySelector<HTMLElement>('[data-pane="c"]')!.style.transform).toBe("translate(500px, 400px)");
  api.mockClear();
  fire(host.querySelector('[data-cap="c"]')!, "pointerdown", { button: 0, buttons: 1, clientX: 700, clientY: 410 });
  fire(window, "pointermove", { button: -1, buttons: 1, clientX: 600, clientY: 500 });
  fire(window, "pointermove", { button: -1, buttons: 1, clientX: 250, clientY: 400 });
  await tick(0);
  fire(window, "pointerup", { button: 0, buttons: 0, clientX: 250, clientY: 400 });
  expect(api.mock.calls.filter(([, method]) => method === "pane.swap" || method === "pane.place")).toEqual([]);
  expect(focus).toHaveBeenCalledWith("c"); // PaneSurface handles a body press by focusing its pane.
});
