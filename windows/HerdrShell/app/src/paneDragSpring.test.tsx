import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Layout, Rect } from "./model";
// Owner-written scenario for pane drag S8, spring-loaded tabs on the Windows Shell (spec
// pane-drag-rearrange-2026-10-07, sections "Spring-loaded tabs", "Cancel", "Where it lands" and S8). Implementers may
// not edit this file. Run: npm test -- paneDragSpring -t spring
//
// Through the DOM-free controller only; the fakes are bridge.api (the Tauri IPC edge to the herdr server) and the
// clock (vitest fake timers). The one interface S8 adds for this check is a constructor option:
//
//   new PaneDrag(machine, { onChange?, onError?, onSpring?: (tabId: string) => void })
//
// While a lifted drag rests on a same-machine sidebar tab row (`move(point, { kind: "tab", tab_id })`) for
// motion.springLoadMs (450 ms) with under 4 px of travel, the controller sends `tab.focus { tab_id }` once and calls
// `onSpring(tabId)` so the view shows that tab. The view then reports that tab's layout through `layoutChanged`, and
// the drag continues in it: its zones and its tab edge apply. A cancel after a spring-load sends `tab.focus` back to
// the origin tab and calls `onSpring(origin)`. Leaving the row, or 4 px of travel, restarts the dwell.
const api = vi.hoisted(() => vi.fn());
vi.mock("./bridge", () => ({ bridge: { api: (...args: unknown[]) => api(...args) } }));
import { PaneDrag } from "./paneDrag";
import type { PaneDragPress } from "./paneDrag";

// Tab t1: A | (B / C); tab t2: X | Y. Both shown at 1200 x 800 px, so one column is 10 px and one row 20 px.
const area = { x: 0, y: 0, width: 120, height: 40 };
const three: Layout = {
  tab_id: "t1", area, focused_pane_id: "A", panes: [
    { pane_id: "A", rect: { x: 0, y: 0, width: 60, height: 40 } },
    { pane_id: "B", rect: { x: 60, y: 0, width: 60, height: 20 } },
    { pane_id: "C", rect: { x: 60, y: 20, width: 60, height: 20 } },
  ],
};
const two: Layout = {
  tab_id: "t2", area, focused_pane_id: "X", panes: [
    { pane_id: "X", rect: { x: 0, y: 0, width: 60, height: 40 } },
    { pane_id: "Y", rect: { x: 60, y: 0, width: 60, height: 40 } },
  ],
};
const size = { width: 1200, height: 800 };
const at = (x: number, y: number) => ({ x, y });
const press = (): PaneDragPress => ({ pane: "A", label: "shell", point: at(300, 10), layout: three, size, supported: true });
const toT2 = { kind: "tab" as const, tab_id: "t2" };
// Where the pointer sits on t2's sidebar row, in the host's coordinates (the sidebar is left of the host).
const ROW = at(-150, 120);
const B_CENTRE = at(900, 200);
const Y_RIGHT_BAND = at(1150, 400);
const Y_PX: Rect = { x: 600, y: 0, width: 600, height: 800 };
const DWELL = 450;

type Options = NonNullable<ConstructorParameters<typeof PaneDrag>[1]> & { onSpring?: (tabId: string) => void };
let springs: string[] = [];
function controller() {
  springs = [];
  const options: Options = { onSpring: tabId => { springs.push(tabId); } };
  return new PaneDrag("studio", options);
}
/** Press on A's cap and travel past the threshold, still over A (the source: no zone). */
function lifted() {
  const drag = controller();
  expect(drag.press(press())).toBe(true);
  drag.move(at(300, 60));
  expect(drag.state.phase).toBe("dragging");
  return drag;
}
const calls = () => api.mock.calls.map(([machine, method, params]) => ({ machine, method, params }));
const last = () => calls()[calls().length - 1];
const focusCalls = () => calls().filter(c => c.method === "tab.focus").map(c => c.params);
const focus = (tab_id: string) => ({ machine: "studio", method: "tab.focus", params: { tab_id } });
/** Lift A, rest on t2's row for the dwell, and show t2 as the view does once it switched. */
function springToT2() {
  const drag = lifted();
  drag.move(ROW, toT2);
  vi.advanceTimersByTime(DWELL);
  expect(calls()).toEqual([focus("t2")]);
  drag.layoutChanged(two);
  expect(drag.state.phase).toBe("dragging");
  return drag;
}

beforeEach(() => {
  vi.useFakeTimers();
  api.mockReset();
  // tab.focus answers at once; dry runs and drops stay pending unless a test answers them.
  api.mockImplementation((_machine: string, method: string) => method === "tab.focus" ? Promise.resolve({}) : new Promise(() => {}));
});
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("pane drag: spring-loaded tabs", () => {
  it("spring: a 450 ms dwell on a tab row sends tab.focus once, and the drop uses that tab's zones", () => {
    const drag = lifted();
    drag.move(ROW, toT2);
    expect(drag.state.zone).toEqual({ kind: "into_tab", tab_id: "t2" });
    vi.advanceTimersByTime(DWELL - 1);
    expect(calls()).toEqual([]);
    expect(springs).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(calls()).toEqual([focus("t2")]);
    expect(springs).toEqual(["t2"]);
    expect(drag.state).toMatchObject({ phase: "dragging", source: "A" });
    vi.advanceTimersByTime(2000);
    expect(focusCalls()).toEqual([{ tab_id: "t2" }]);

    // The view shows t2: the drag continues, and t2's panes are the zones.
    drag.layoutChanged(two);
    expect(drag.state).toMatchObject({ phase: "dragging", source: "A" });
    drag.move(Y_RIGHT_BAND);
    expect(drag.state.zone).toEqual({ kind: "pane_edge", target: "Y", side: "right" });
    expect(calls()).toEqual([focus("t2"), { machine: "studio", method: "pane.place",
      params: { pane_id: "A", target: { type: "pane", pane_id: "Y" }, side: "right", dry_run: true } }]);
    drag.move(at(900, 400));
    expect(drag.state).toMatchObject({ zone: { kind: "centre", target: "Y" }, ghost: Y_PX });
    drag.move(Y_RIGHT_BAND);
    expect(drag.release()).toBe("dropped");
    expect(last()).toEqual({ machine: "studio", method: "pane.place",
      params: { pane_id: "A", target: { type: "pane", pane_id: "Y" }, side: "right", focus: true, dry_run: false } });
    expect(calls().some(c => c.method === "pane.place" && (c.params as { target?: { type?: string } }).target?.type === "tab")).toBe(false);
  });

  it("spring: after a spring-load the tab edge is the new tab's edge", () => {
    const drag = springToT2();
    drag.move(at(1195, 600));
    expect(drag.state.zone).toEqual({ kind: "tab_edge", side: "right" });
    expect(last()).toEqual({ machine: "studio", method: "pane.place",
      params: { pane_id: "A", target: { type: "tab", tab_id: "t2" }, side: "right", dry_run: true } });
  });

  it("spring: leaving the row at 300 ms sends no focus, and a return starts a full dwell", () => {
    const drag = lifted();
    drag.move(ROW, toT2);
    vi.advanceTimersByTime(300);
    drag.move(B_CENTRE);
    expect(drag.state.zone).toEqual({ kind: "centre", target: "B" });
    vi.advanceTimersByTime(1000);
    expect(focusCalls()).toEqual([]);
    expect(springs).toEqual([]);
    drag.move(ROW, toT2);
    vi.advanceTimersByTime(DWELL - 1);
    expect(focusCalls()).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(focusCalls()).toEqual([{ tab_id: "t2" }]);
  });

  it("spring: travel under 4 px keeps the dwell, 4 px restarts it", () => {
    let drag = lifted();
    drag.move(ROW, toT2);
    vi.advanceTimersByTime(300);
    drag.move(at(ROW.x + 3, ROW.y), toT2);
    vi.advanceTimersByTime(DWELL - 300);
    expect(focusCalls()).toEqual([{ tab_id: "t2" }]);

    drag.cancel(); api.mockClear();
    drag = lifted();
    drag.move(ROW, toT2);
    vi.advanceTimersByTime(300);
    drag.move(at(ROW.x + 4, ROW.y), toT2);
    vi.advanceTimersByTime(DWELL - 1);
    expect(focusCalls()).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(focusCalls()).toEqual([{ tab_id: "t2" }]);
  });

  it("spring: a cancel after a spring-load sends tab.focus back to the origin tab and nothing else", () => {
    const cases: [string, (drag: PaneDrag) => void, boolean][] = [
      ["cancel (Esc, right click)", drag => drag.cancel(), true],
      ["cancel before the view switched", drag => drag.cancel(), false],
      ["release over no zone", drag => { drag.move(at(-50, 700)); expect(drag.release()).toBe("cancelled"); }, true],
    ];
    for (const [name, end, shown] of cases) {
      api.mockClear();
      const drag = lifted();
      drag.move(ROW, toT2);
      vi.advanceTimersByTime(DWELL);
      if (shown) drag.layoutChanged(two);
      const before = calls().length;
      end(drag);
      expect(calls().slice(before), name).toEqual([focus("t1")]);
      expect(springs, name).toEqual(["t2", "t1"]);
      expect(drag.state.phase, name).toBe("idle");
      vi.advanceTimersByTime(2000);
      expect(calls().slice(before), name).toEqual([focus("t1")]);
    }
  });

  it("spring: a cancel before the dwell ends sends no tab.focus, then or later", () => {
    const drag = lifted();
    drag.move(ROW, toT2);
    vi.advanceTimersByTime(300);
    drag.cancel();
    vi.advanceTimersByTime(2000);
    expect(calls()).toEqual([]);
    expect(springs).toEqual([]);
  });
});
