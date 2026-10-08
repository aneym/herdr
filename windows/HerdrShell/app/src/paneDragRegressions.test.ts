// @vitest-environment happy-dom
// Post-merge must-fixes: exercise the real clip in React and real DOM cancellation;
// only the external Tauri API is faked. Geometry tables cover outer-edge clipping.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createElement, act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { Layout, Rect } from "./model";
const api = vi.hoisted(() => vi.fn());
vi.mock("./bridge", () => ({ bridge: { api: (...args: unknown[]) => api(...args) } }));
import { PaneDrag } from "./paneDrag";
import { PaneClip } from "./TabView";
import { blockCancelContextMenu, clipRect } from "./paneClip";
const size = { width: 1000, height: 800 };
const layout: Layout = { tab_id: "t", area: { x: 0, y: 0, width: 100, height: 40 }, panes: [
  { pane_id: "a", rect: { x: 0, y: 0, width: 50, height: 40 } },
  { pane_id: "b", rect: { x: 50, y: 0, width: 50, height: 40 } },
] };
const swapped: Layout = { ...layout, panes: layout.panes.map((p, i) => ({ ...p, rect: layout.panes[1 - i].rect })) };
function pressed() {
  const drag = new PaneDrag("studio");
  drag.press({ pane: "a", label: "shell", point: { x: 200, y: 10 }, layout, size, supported: true });
  return drag;
}
let root: Root | undefined;
let host: HTMLDivElement;
beforeEach(() => {
  vi.useFakeTimers(); api.mockReset(); api.mockReturnValue(new Promise(() => {}));
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => setTimeout(() => callback(0), 16));
  vi.stubGlobal("cancelAnimationFrame", (id: number) => clearTimeout(id));
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  host = document.createElement("div"); document.body.append(host);
});
afterEach(() => { act(() => root?.unmount()); root = undefined; host.remove(); vi.useRealTimers(); vi.unstubAllGlobals(); });
function renderClip(box: Rect, animateLayout: boolean) {
  root ??= createRoot(host);
  act(() => root!.render(createElement(PaneClip, { id: "a", box: clipRect(box, size), lifted: false, animateLayout,
    children: settling => createElement("span", { "data-settling": String(settling) }) })));
}
describe("S7a pane drag regressions", () => {
  it.each([
    [0, 0, 499, 399], [500, 0, 500, 399], [0, 400, 499, 400], [500, 400, 500, 400],
  ])("keeps the split gap at (%i, %i) in both clip and content", (x, y, width, height) => {
    renderClip({ x, y, width: 500, height: 400 }, false);
    const clip = host.querySelector<HTMLElement>(".pane-clip")!;
    expect(clip.style.width).toBe(`${width}px`); expect(clip.style.height).toBe(`${height}px`);
    expect((clip.firstElementChild as HTMLElement).style.width).toBe(`${width}px`);
    expect((clip.firstElementChild as HTMLElement).style.height).toBe(`${height}px`);
  });
  it.each([false, true])("applies non-drop geometry immediately, reduced motion=%s", reduced => {
    vi.stubGlobal("matchMedia", () => ({ matches: reduced }));
    const drag = pressed();
    renderClip({ x: 0, y: 0, width: 500, height: 800 }, false);
    expect(drag.shouldAnimateLayout(swapped)).toBe(false);
    renderClip({ x: 500, y: 0, width: 500, height: 800 }, drag.shouldAnimateLayout(swapped));
    const clip = host.querySelector<HTMLElement>(".pane-clip")!;
    expect(clip.style.transform).toBe("translate(500px, 0px)"); expect(clip.style.transition).toBe("");
    expect(host.querySelector("span")?.getAttribute("data-settling")).toBe("false");
  });
  it("picks a zone on the threshold crossing move so a one-move flick drops", () => {
    const drag = pressed(); drag.move({ x: 750, y: 400 });
    expect(drag.state.zone).toEqual({ kind: "centre", target: "b" });
    expect(drag.release()).toBe("dropped");
    expect(api).toHaveBeenCalledWith("studio", "pane.swap", { source_pane_id: "a", target_pane_id: "b" });
    drag.cancel();
  });
  it("does not draw a zone when layout changes under an unlifted press", () => {
    const drag = pressed(); drag.layoutChanged(swapped);
    expect(drag.state).toMatchObject({ phase: "pressed", zone: null, ghost: null });
    expect(api).not.toHaveBeenCalled(); expect(drag.release()).toBe("click");
  });
  it("blocks a right-click menu through cancellation and that button's release", () => {
    const drag = pressed(); drag.move({ x: 750, y: 400 });
    const stop = blockCancelContextMenu(window); drag.cancel();
    const menu = () => window.dispatchEvent(new Event("contextmenu", { cancelable: true }));
    expect(menu()).toBe(false);
    window.dispatchEvent(new PointerEvent("pointerup", { button: 0 })); vi.runOnlyPendingTimers();
    expect(menu()).toBe(false);
    window.dispatchEvent(new PointerEvent("pointerup", { button: 2 }));
    expect(menu()).toBe(false); vi.runOnlyPendingTimers(); expect(menu()).toBe(true); stop();
  });
});
