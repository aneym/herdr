// @vitest-environment happy-dom
// TabView owns chip measurement: the pure geometry table cannot catch shrink-to-fit at an edge.
// Only browser layout/animation and the native IPC edge are faked; the drag and view are real.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import TabView from "./TabView";
import type { PaneDrag } from "./paneDrag";
import type { Layout } from "./model";
const fsModule = "node:fs";
const { readFileSync } = await import(/* @vite-ignore */ fsModule) as { readFileSync(path: string, encoding: "utf8"): string };
const styles = readFileSync("src/styles.css", "utf8");

vi.mock("./bridge", () => ({ bridge: { api: vi.fn(async () => null) } }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let dispose = () => {};
afterEach(() => { dispose(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("measures a multi-word chip at full width before flipping at the bottom-right and cancels from there", async () => {
  const bounds = { width: 800, height: 600 }, fullWidth = 240, height = 28;
  const label = "Review the pane drag changes";
  // happy-dom has no layout. Model the browser's constrained absolute-position auto width:
  // near the right edge it is squeezed, unless the view asks for the intrinsic width.
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(function (this: HTMLElement) {
    if (!this.classList.contains("pane-drag-chip")) return 0;
    // React starts at pointer + offset. happy-dom may discard that calc with a CSS variable.
    const left = Number(this.style.left.match(/-?\d+(?:\.\d+)?/)?.[0] ?? 808);
    return this.style.width === "max-content" || left + fullWidth <= bounds.width ? fullWidth : 40;
  });
  vi.spyOn(HTMLElement.prototype, "offsetLeft", "get").mockImplementation(function (this: HTMLElement) { return parseFloat(this.style.left) || 0; });
  vi.spyOn(HTMLElement.prototype, "offsetTop", "get").mockImplementation(function (this: HTMLElement) { return parseFloat(this.style.top) || 0; });
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(height);
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(bounds.width);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(bounds.height);
  const animate = vi.spyOn(HTMLElement.prototype, "animate").mockReturnValue({} as Animation);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  const layout: Layout = { tab_id: "t1", area: { x: 0, y: 0, width: 80, height: 30 }, panes: [
    { pane_id: "A", rect: { x: 0, y: 0, width: 40, height: 30 } },
    { pane_id: "B", rect: { x: 40, y: 0, width: 40, height: 30 } },
  ] };
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  dispose = () => { act(() => root.unmount()); host.remove(); };
  let drag: PaneDrag | null = null;
  await act(async () => root.render(<TabView snapshot={{ panes: [], layouts: [layout] }} selected="t1" machine="studio" focused="A" onFocus={() => {}} shortcut={() => false} register={() => {}} pin={() => {}} registerDrag={value => { drag = value; }} />));
  expect(drag).not.toBeNull();
  const controller = drag! as PaneDrag;
  await act(async () => {
    expect(controller.press({ pane: "A", label, point: { x: 200, y: 10 }, layout, size: bounds, supported: true })).toBe(true);
    controller.move({ x: 796, y: 596 });
  });
  const chip = host.querySelector<HTMLElement>(".pane-drag-chip")!;
  expect(chip.textContent).toContain(label);
  expect(chip.offsetWidth).toBe(fullWidth);
  expect(parseFloat(chip.style.left) + chip.offsetWidth).toBeLessThanOrEqual(bounds.width - 4);
  expect(parseFloat(chip.style.top) + chip.offsetHeight).toBeLessThanOrEqual(bounds.height - 4);
  expect(chip.style.left).toBe("544px");
  const left = parseFloat(chip.style.left), top = parseFloat(chip.style.top);
  animate.mockClear();
  await act(async () => controller.cancel());
  const cancellation = animate.mock.calls.find((_, index) => (animate.mock.contexts[index] as unknown) === chip);
  expect(cancellation?.[0]).toEqual([{ opacity: 1, transform: "translate(0, 0)" }, { opacity: 0, transform: `translate(${-left}px, ${-top}px)` }]);
});

it("keeps the pane drag chip label on one line", () => {
  const rule = styles.match(/\.pane-drag-chip\s*\{([^}]*)\}/)?.[1];
  expect(rule).toMatch(/white-space:\s*nowrap\s*;/);
});
