// @vitest-environment happy-dom
// S7c regressions through the real TabView: its window pointer listeners, PaneClip and PaneDrag.
// Faked: the Tauri bridge (external) and PaneTerm, whose xterm/WebGL terminal cannot run in happy-dom;
// the stand-in renders only the cap and wires onCapPointerDown as PaneTerm does.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { Layout, Snapshot } from "./model";
import type { PaneDragState } from "./paneDrag";
const api = vi.hoisted(() => vi.fn());
vi.mock("./bridge", () => ({ bridge: { api: (...args: unknown[]) => api(...args) } }));
vi.mock("./PaneTerm", () => ({ default: (props: { pane: { pane_id: string }; onCapPointerDown?: (event: unknown) => void }) => <div className="pane-cap" data-cap={props.pane.pane_id} onPointerDown={event => props.onCapPointerDown?.(event)} /> }));
vi.mock("./Chat", () => ({ default: () => null }));
const { default: TabView } = await import("./TabView");

let size = { width: 1000, height: 800 };
let root: Root | undefined;
let host: HTMLDivElement;
let phases: PaneDragState["phase"][] = [];
const last = () => phases[phases.length - 1];
beforeEach(() => {
  vi.useFakeTimers(); api.mockReset(); phases = [];
  // Probes and dry runs answer; a real drop stays pending, as a slow server would.
  api.mockImplementation((_machine: string, _method: string, params: { dry_run?: boolean }) => params?.dry_run ? Promise.resolve({ place: { changed: true } }) : new Promise(() => {}));
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => setTimeout(() => callback(0), 16));
  vi.stubGlobal("cancelAnimationFrame", (id: number) => clearTimeout(id));
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {} });
  vi.stubGlobal("ResizeObserver", class { constructor(private callback: (entries: { contentRect: typeof size }[]) => void) {} observe() { this.callback([{ contentRect: size }]); } disconnect() {} });
  if (!document.elementFromPoint) document.elementFromPoint = () => null;
  if (!Element.prototype.animate) Element.prototype.animate = () => ({}) as Animation;
  host = document.createElement("div"); document.body.append(host);
});
afterEach(() => { act(() => root?.unmount()); root = undefined; host.remove(); vi.useRealTimers(); vi.unstubAllGlobals(); });

const two: Layout = { tab_id: "t", area: { x: 0, y: 0, width: 100, height: 40 }, panes: [
  { pane_id: "a", rect: { x: 0, y: 0, width: 50, height: 40 } },
  { pane_id: "b", rect: { x: 50, y: 0, width: 50, height: 40 } },
] };
function snapshot(layout: Layout): Snapshot {
  return { layouts: [layout], tabs: [], panes: layout.panes.map(p => ({ pane_id: p.pane_id, terminal_id: "term-" + p.pane_id, tab_id: layout.tab_id, workspace_id: "w" })) } as Snapshot;
}
async function mount(layout: Layout) {
  root ??= createRoot(host);
  const render = (next: Layout) => act(() => root!.render(<TabView snapshot={snapshot(next)} selected={next.tab_id} machine="studio" focused={null} onFocus={() => {}} shortcut={() => false} register={() => {}} pin={() => {}} onDragChange={state => phases.push(state.phase)} />));
  render(layout);
  await act(async () => { await vi.advanceTimersByTimeAsync(0); }); // the pane.place probe answers
  return render;
}
const fire = (target: EventTarget, type: string, init: PointerEventInit) => act(() => { target.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 1, ...init })); });
const menuBlocked = () => !window.dispatchEvent(new Event("contextmenu", { cancelable: true }));
const changes = () => api.mock.calls.filter(([, method, params]) => !(method === "pane.place" && params?.dry_run));
async function dragAOverB(layout = two) {
  const render = await mount(layout);
  fire(host.querySelector('[data-cap="a"]')!, "pointerdown", { button: 0, buttons: 1, clientX: 200, clientY: 10 });
  fire(window, "pointermove", { button: -1, buttons: 1, clientX: 750, clientY: 400 });
  expect(last()).toBe("dragging");
  return render;
}

it("keeps an outer-edge pane full width when its scaled rect falls short by float error", async () => {
  // 100/700 x 900 = 128.57142857142856 and 600/700 x 900 = 771.4285714285713: they sum to 899.9999999999999.
  size = { width: 900, height: 800 };
  await mount({ tab_id: "t", area: { x: 0, y: 0, width: 700, height: 40 }, panes: [
    { pane_id: "a", rect: { x: 0, y: 0, width: 100, height: 40 } },
    { pane_id: "b", rect: { x: 100, y: 0, width: 600, height: 40 } },
  ] });
  size = { width: 1000, height: 800 };
  const clip = (id: string) => host.querySelector<HTMLElement>(`.pane-clip[data-pane="${id}"]`)!;
  expect(parseFloat(clip("b").style.width)).toBeCloseTo(771.4285714285713, 6);
  expect(parseFloat(clip("a").style.width)).toBeCloseTo(127.57142857142856, 6); // the split gap stays on inner edges
});

it("cancels on a right press chorded onto the held left button and blocks its menu until the right release", async () => {
  await dragAOverB();
  fire(window, "pointermove", { button: 2, buttons: 3, clientX: 750, clientY: 400 });
  expect(last()).toBe("idle");
  expect(menuBlocked()).toBe(true);
  fire(window, "pointermove", { button: 0, buttons: 2, clientX: 750, clientY: 400 }); // left up first, in the chord
  act(() => vi.runOnlyPendingTimers());
  expect(menuBlocked()).toBe(true);
  fire(window, "pointerup", { button: 2, buttons: 0, clientX: 750, clientY: 400 });
  expect(menuBlocked()).toBe(true);
  act(() => vi.runOnlyPendingTimers());
  expect(menuBlocked()).toBe(false);
  expect(changes()).toEqual([]);
});

it("ends the drag on the last button up even when that button is the right one", async () => {
  await dragAOverB();
  // The chorded right press went unseen; the left went up while the right was held, so the last pointerup is the right's.
  fire(window, "pointerup", { button: 2, buttons: 0, clientX: 750, clientY: 400 });
  expect(last()).toBe("idle");
  expect(menuBlocked()).toBe(true);
  act(() => vi.runOnlyPendingTimers());
  expect(menuBlocked()).toBe(false);
  fire(window, "pointermove", { button: -1, buttons: 0, clientX: 300, clientY: 400 });
  expect(last()).toBe("idle");
  expect(changes()).toEqual([]);
});
