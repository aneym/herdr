import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Layout, Rect } from "./model";
// Owner-written scenario for pane drag S7 (spec pane-drag-rearrange-2026-10-07, S3 steps 1-7 on
// the Windows Shell). The controller is DOM-free; the only fake is bridge.api, the Tauri IPC edge
// to the herdr server. A wrong call here rearranges Alex's real panes, a missing cancel leaves a
// pane lifted, and a stale dry-run answer draws the ghost in the wrong place. The PC scenario
// (scripts/check_pane_drag.py) proves one real drag; this table owns the rules.
const api = vi.hoisted(() => vi.fn());
vi.mock("./bridge", () => ({ bridge: { api: (...args: unknown[]) => api(...args) } }));
import { canDragPane, PaneDrag, prefersReducedMotion, probePlace, transitionFor } from "./paneDrag";
import type { PaneDragPress, PaneDragState } from "./paneDrag";
import { actionFor, moveModeKey } from "./keys";
import type { KeyEvent } from "./keys";

// Tab t1: A | (B / C), shown at 1200 x 800 px, so one column is 10 px and one row 20 px.
const three: Layout = {
  tab_id: "t1", area: { x: 0, y: 0, width: 120, height: 40 }, focused_pane_id: "A", panes: [
    { pane_id: "A", rect: { x: 0, y: 0, width: 60, height: 40 } },
    { pane_id: "B", rect: { x: 60, y: 0, width: 60, height: 20 } },
    { pane_id: "C", rect: { x: 60, y: 20, width: 60, height: 20 } },
  ],
};
const lone: Layout = { tab_id: "t3", area: three.area, focused_pane_id: "X", panes: [{ pane_id: "X", rect: three.area }] };
const size = { width: 1200, height: 800 };
const at = (x: number, y: number) => ({ x, y });
const px = (x: number, y: number, width: number, height: number): Rect => ({ x, y, width, height });
const B_PX = px(600, 0, 600, 400);
const press = (over: Partial<PaneDragPress> = {}): PaneDragPress => ({ pane: "A", label: "shell", point: at(300, 10), layout: three, size, supported: true, ...over });
/** Press on A's cap and travel well past the threshold, still over A (the source: no zone). */
function lifted(drag = new PaneDrag("studio")) {
  expect(drag.press(press())).toBe(true);
  drag.move(at(300, 60));
  return drag;
}
const placed = (rect: Rect, extra: Record<string, unknown> = {}) => ({ place: {
  changed: true, dry_run: true, pane_id: "A", previous_pane_id: "A", placed_rect: rect, focused_pane_id: "A",
  target_layout: { workspace_id: "w1", tab_id: "t1", zoomed: false, area: three.area, focused_pane_id: "A", panes: [], splits: [] }, ...extra,
} });
const calls = () => api.mock.calls.map(([machine, method, params]) => ({ machine, method, params }));
const dryRun = (target: object, side: string) => ({ machine: "studio", method: "pane.place", params: { pane_id: "A", target, side, dry_run: true } });
const toB = { type: "pane", pane_id: "B" }, toC = { type: "pane", pane_id: "C" }, toTab = { type: "tab", tab_id: "t1" };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
const flush = () => new Promise(resolve => setTimeout(resolve, 0));

beforeEach(() => { api.mockReset(); api.mockReturnValue(new Promise(() => {})); });
afterEach(() => { vi.unstubAllGlobals(); });

describe("pane drag: zones and dry runs", () => {
  it("lifts past the threshold and sends one dry run per zone change (step 1)", () => {
    const changes: PaneDragState[] = [];
    const drag = lifted(new PaneDrag("studio", { onChange: state => changes.push(state) }));
    expect(drag.state).toMatchObject({ phase: "dragging", keyboard: false, source: "A", label: "shell", sourceRect: px(0, 0, 600, 800), zone: null, ghost: null });
    expect(api).not.toHaveBeenCalled();
    drag.move(at(1150, 200));
    drag.move(at(1140, 210));
    expect(calls()).toEqual([dryRun(toB, "right")]);
    // Until the server answers, the ghost heads for the local estimate: B's right half.
    expect(drag.state).toMatchObject({ zone: { kind: "pane_edge", target: "B", side: "right" }, ghost: px(900, 0, 300, 400), exact: false, pending: true });
    // The view renders from onChange: each change is a fresh state object, the last one current.
    expect(changes[changes.length - 1]).toBe(drag.state);
    expect(new Set(changes).size).toBe(changes.length);
  });
  it("keeps one dry run in flight and lets the newest zone win", async () => {
    const first = deferred<unknown>(), second = deferred<unknown>();
    api.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const drag = lifted();
    drag.move(at(1150, 200)); // B right
    drag.move(at(640, 200)); // B left, superseded before it is sent
    drag.move(at(900, 430)); // C up
    expect(calls()).toEqual([dryRun(toB, "right")]);
    expect(drag.state).toMatchObject({ zone: { kind: "pane_edge", target: "C", side: "up" }, ghost: px(600, 400, 600, 200), pending: true });
    first.resolve(placed({ x: 60, y: 0, width: 60, height: 20 }));
    await flush();
    expect(calls()).toEqual([dryRun(toB, "right"), dryRun(toC, "up")]);
    // B's late answer does not move the ghost off C's zone.
    expect(drag.state).toMatchObject({ zone: { kind: "pane_edge", target: "C", side: "up" }, ghost: px(600, 400, 600, 200), exact: false, pending: true });
    second.resolve(placed({ x: 0, y: 20, width: 120, height: 10 }));
    await flush();
    expect(drag.state).toMatchObject({ ghost: px(0, 400, 1200, 200), exact: true, pending: false });
  });
  it("retargets the ghost to placed_rect and reuses answers until the layout changes (step 2)", async () => {
    api.mockResolvedValue(placed({ x: 60, y: 0, width: 60, height: 20 }));
    const drag = lifted();
    drag.move(at(1150, 200));
    await flush();
    // Removing A reflows B to the full width first, so A lands as the right HALF of the tab.
    expect(drag.state).toMatchObject({ ghost: B_PX, exact: true, pending: false });
    drag.move(at(900, 200));
    expect(drag.state).toMatchObject({ zone: { kind: "centre", target: "B" }, ghost: B_PX, exact: true });
    drag.move(at(1150, 200));
    expect(api).toHaveBeenCalledTimes(1);
    expect(drag.state).toMatchObject({ zone: { kind: "pane_edge", target: "B", side: "right" }, ghost: B_PX, exact: true, pending: false });
    // A snapshot with the same rects changes nothing; a reflow drops the cache and asks again.
    drag.layoutChanged({ ...three, panes: three.panes.map(p => ({ ...p, rect: { ...p.rect } })) });
    expect(api).toHaveBeenCalledTimes(1);
    drag.layoutChanged({ ...three, panes: [three.panes[0], { pane_id: "B", rect: { x: 60, y: 0, width: 60, height: 24 } }, { pane_id: "C", rect: { x: 60, y: 24, width: 60, height: 16 } }] });
    expect(calls()).toEqual([dryRun(toB, "right"), dryRun(toB, "right")]);
  });
  it("takes the tab edge first, in pixels from the motion tokens", () => {
    const drag = lifted();
    drag.move(at(1195, 600)); // 5 px from the right edge: within tabEdgePx (12), though inside C
    expect(calls()).toEqual([dryRun(toTab, "right")]);
    expect(drag.state).toMatchObject({ zone: { kind: "tab_edge", side: "right" }, ghost: px(800, 0, 400, 800) });
  });
  it("shows no zone where the server reports no_change", async () => {
    api.mockResolvedValue(placed({ x: 80, y: 0, width: 40, height: 40 }, { changed: false, reason: "no_change" }));
    const drag = lifted();
    drag.move(at(1195, 600));
    await flush();
    expect(drag.state).toMatchObject({ zone: null, ghost: null, pending: false });
    drag.move(at(1196, 610));
    expect(api).toHaveBeenCalledTimes(1);
    expect(drag.release()).toBe("cancelled");
    expect(api).toHaveBeenCalledTimes(1);
  });
});

describe("pane drag: drop", () => {
  it("drops with pane.place dry_run false and focus true; the ghost holds until the server replies (step 3)", async () => {
    const reply = deferred<unknown>();
    api.mockResolvedValueOnce(placed({ x: 60, y: 0, width: 60, height: 20 })).mockReturnValueOnce(reply.promise);
    const drag = lifted();
    drag.move(at(1150, 200));
    await flush();
    expect(drag.release()).toBe("dropped");
    expect(calls()[1]).toEqual({ machine: "studio", method: "pane.place", params: { pane_id: "A", target: toB, side: "right", focus: true, dry_run: false } });
    await flush();
    expect(drag.state).toMatchObject({ phase: "dropped", ghost: B_PX });
    const after = { ...three, panes: [{ pane_id: "B", rect: { x: 0, y: 0, width: 60, height: 20 } }, { pane_id: "A", rect: { x: 60, y: 0, width: 60, height: 20 } }, { pane_id: "C", rect: { x: 0, y: 20, width: 120, height: 20 } }] };
    drag.layoutChanged(after);
    expect(drag.state).toMatchObject({ phase: "dropped", ghost: B_PX });
    reply.resolve(placed({ x: 60, y: 0, width: 60, height: 20 }, { dry_run: false, target_layout: after }));
    await flush();
    expect(drag.state).toMatchObject({ phase: "idle", source: null, zone: null, ghost: null, end: "settle" });
    expect(api).toHaveBeenCalledTimes(2);
  });
  it("swaps on a centre release, with no dry run (step 5)", () => {
    const drag = lifted();
    drag.move(at(900, 200));
    expect(drag.state).toMatchObject({ zone: { kind: "centre", target: "B" }, ghost: B_PX, exact: true, pending: false });
    expect(drag.release()).toBe("dropped");
    expect(calls()).toEqual([{ machine: "studio", method: "pane.swap", params: { source_pane_id: "A", target_pane_id: "B" } }]);
  });
  it("moves into another tab's right third from its row, never into its own (step 6)", () => {
    const drag = lifted();
    drag.move(at(-120, 80), { kind: "tab", tab_id: "t1" });
    expect(drag.state.zone).toBeNull();
    drag.move(at(-120, 110), { kind: "tab", tab_id: "t2" });
    expect(drag.state).toMatchObject({ zone: { kind: "into_tab", tab_id: "t2" }, ghost: null });
    expect(api).not.toHaveBeenCalled();
    expect(drag.release()).toBe("dropped");
    expect(calls()).toEqual([{ machine: "studio", method: "pane.place", params: { pane_id: "A", target: { type: "tab", tab_id: "t2" }, side: "right", focus: true, dry_run: false } }]);
  });
  it("opens the pane as a new tab in a space from its header (step 7)", () => {
    const drag = lifted();
    drag.move(at(-120, 40), { kind: "space", workspace_id: "w2" });
    expect(drag.state).toMatchObject({ zone: { kind: "new_tab_in", workspace_id: "w2" }, ghost: null });
    expect(drag.release()).toBe("dropped");
    expect(calls()).toEqual([{ machine: "studio", method: "pane.move", params: { pane_id: "A", destination: { type: "new_tab", workspace_id: "w2" }, focus: true } }]);
  });
  it("clears the drag and reports a drop the server refuses", async () => {
    api.mockRejectedValueOnce(Object.assign(new Error("pane gone"), { code: "pane_not_found" }));
    const onError = vi.fn();
    const drag = lifted(new PaneDrag("studio", { onError }));
    drag.move(at(900, 200));
    expect(drag.release()).toBe("dropped");
    await flush();
    expect(drag.state).toMatchObject({ phase: "idle", ghost: null });
    expect(onError).toHaveBeenCalledOnce();
  });
});

describe("pane drag: cancel and refusals", () => {
  it("cancels (Esc, right click) with nothing sent and the lift cleared (step 4)", () => {
    const drag = lifted();
    drag.move(at(900, 200));
    drag.cancel();
    expect(drag.state).toMatchObject({ phase: "idle", source: null, sourceRect: null, zone: null, ghost: null });
    expect(drag.release()).toBe("none");
    expect(api).not.toHaveBeenCalled();
  });
  it("ignores a dry-run answer that lands after a cancel", async () => {
    const answer = deferred<unknown>();
    api.mockReturnValueOnce(answer.promise);
    const drag = lifted();
    drag.move(at(1150, 200));
    drag.cancel();
    answer.resolve(placed({ x: 60, y: 0, width: 60, height: 20 }));
    await flush();
    expect(drag.state).toMatchObject({ phase: "idle", zone: null, ghost: null });
    expect(calls()).toEqual([dryRun(toB, "right")]);
  });
  it.each([["over the source", at(300, 400)], ["outside the tab area", at(1300, 400)]])("cancels a release %s without a call", (_, point) => {
    const drag = lifted();
    drag.move(at(900, 200));
    drag.move(point);
    expect(drag.state.zone).toBeNull();
    expect(drag.release()).toBe("cancelled");
    expect(drag.state.phase).toBe("idle");
    expect(api).not.toHaveBeenCalled();
  });
  it("cancels when the tab changes or the source leaves the layout", () => {
    const other = lifted();
    other.move(at(900, 200));
    other.layoutChanged({ ...three, tab_id: "t2" });
    expect(other.state.phase).toBe("idle");
    const gone = lifted();
    gone.move(at(900, 200));
    gone.layoutChanged({ ...three, panes: three.panes.filter(p => p.pane_id !== "A") });
    expect(gone.state.phase).toBe("idle");
    expect(gone.release()).toBe("none");
    expect(api).not.toHaveBeenCalled();
  });
  it("treats a 3 px move as a click and starts the drag at 4 px", () => {
    const drag = new PaneDrag("studio");
    expect(drag.press(press())).toBe(true);
    drag.move(at(303, 10));
    expect(drag.state.phase).toBe("pressed");
    expect(drag.release()).toBe("click");
    expect(drag.state.phase).toBe("idle");
    expect(drag.press(press())).toBe(true);
    drag.move(at(304, 10));
    expect(drag.state.phase).toBe("dragging");
    expect(api).not.toHaveBeenCalled();
  });
  it.each([
    ["an endpoint without pane.place", press({ supported: false })],
    ["a lone pane", press({ pane: "X", layout: lone })],
    ["a zoomed tab", press({ layout: { ...three, zoomed: true } })],
  ])("never drags %s", (_, input) => {
    expect(canDragPane(input.layout, input.supported)).toBe(false);
    const drag = new PaneDrag("studio");
    expect(drag.press(input)).toBe(false);
    drag.move(at(1150, 200));
    expect(drag.state.phase).toBe("idle");
    expect(drag.release()).toBe("none");
    expect(drag.lift({ pane: input.pane, label: input.label, layout: input.layout, size, supported: input.supported })).toBe(false);
    expect(drag.state.phase).toBe("idle");
    expect(api).not.toHaveBeenCalled();
    expect(canDragPane(three, true)).toBe(true);
  });
  it.each([
    ["an answer (same_pane)", () => Promise.resolve(placed({ x: 0, y: 0, width: 60, height: 40 }, { changed: false, reason: "same_pane" })), true],
    ["pane_not_found", () => Promise.reject(Object.assign(new Error("no pane"), { code: "pane_not_found" })), true],
    ["invalid_request (a herdr without pane.place)", () => Promise.reject(Object.assign(new Error("bad"), { code: "invalid_request" })), false],
    ["unknown_method", () => Promise.reject(Object.assign(new Error("unknown"), { code: "unknown_method" })), false],
    ["no answer (machine down)", () => Promise.reject(new Error("connection closed")), null],
  ])("probes pane.place support: %s", async (_, answer, expected) => {
    api.mockImplementationOnce(answer);
    await expect(probePlace("studio", "A")).resolves.toBe(expected);
    expect(calls()).toEqual([{ machine: "studio", method: "pane.place", params: { pane_id: "A", target: { type: "pane", pane_id: "A" }, side: "right", dry_run: true } }]);
  });
});

describe("pane drag: keyboard move mode", () => {
  const key = (k: string, mods: Partial<KeyEvent> = {}): KeyEvent => ({ key: k, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...mods });
  it("Ctrl+Alt+M opens move mode; arrows pick a target, Shift an edge, Enter drops", async () => {
    expect(actionFor(key("m", { ctrlKey: true, altKey: true }))).toBe("move_pane_mode");
    api.mockResolvedValue(placed({ x: 0, y: 0, width: 40, height: 40 }));
    const drag = new PaneDrag("studio");
    expect(drag.lift({ pane: "A", label: "shell", layout: three, size, supported: true })).toBe(true);
    // The nearest neighbour, tried right, down, left, up: B (C ties on overlap; layout order wins).
    expect(drag.state).toMatchObject({ phase: "dragging", keyboard: true, source: "A", zone: { kind: "centre", target: "B" }, ghost: B_PX });
    drag.key("down", false);
    expect(drag.state.zone).toEqual({ kind: "centre", target: "C" });
    drag.key("right", true);
    expect(drag.state.zone).toEqual({ kind: "pane_edge", target: "C", side: "right" });
    expect(calls()).toEqual([dryRun(toC, "right")]);
    await flush();
    drag.key("left", false); // back onto the source: no zone
    expect(drag.state.zone).toBeNull();
    drag.key("left", true); // Shift+arrow on the source: that tab edge
    expect(drag.state).toMatchObject({ zone: { kind: "tab_edge", side: "left" }, ghost: px(0, 0, 400, 800) });
    await flush();
    expect(drag.release()).toBe("dropped");
    expect(calls()).toEqual([dryRun(toC, "right"), dryRun(toTab, "left"), { machine: "studio", method: "pane.place", params: { pane_id: "A", target: toTab, side: "left", focus: true, dry_run: false } }]);
  });
  it("Esc leaves move mode with nothing sent", () => {
    const drag = new PaneDrag("studio");
    drag.lift({ pane: "A", label: "shell", layout: three, size, supported: true });
    drag.key("down", false);
    drag.cancel();
    expect(drag.state).toMatchObject({ phase: "idle", keyboard: false, zone: null });
    expect(api).not.toHaveBeenCalled();
  });
  it.each([
    ["ArrowLeft", {}, { kind: "target", side: "left", edge: false }], ["h", {}, { kind: "target", side: "left", edge: false }],
    ["ArrowDown", {}, { kind: "target", side: "down", edge: false }], ["j", {}, { kind: "target", side: "down", edge: false }],
    ["ArrowUp", {}, { kind: "target", side: "up", edge: false }], ["k", {}, { kind: "target", side: "up", edge: false }],
    ["ArrowRight", {}, { kind: "target", side: "right", edge: false }], ["l", {}, { kind: "target", side: "right", edge: false }],
    ["ArrowRight", { shiftKey: true }, { kind: "target", side: "right", edge: true }], ["L", { shiftKey: true }, { kind: "target", side: "right", edge: true }],
    ["H", { shiftKey: true }, { kind: "target", side: "left", edge: true }],
    ["Enter", {}, { kind: "drop" }], [" ", {}, { kind: "drop" }], ["Escape", {}, { kind: "cancel" }],
    ["x", {}, null], ["ArrowLeft", { ctrlKey: true }, null], ["l", { altKey: true }, null],
  ] as [string, Partial<KeyEvent>, unknown][])("move mode reads %s %j", (k, mods, expected) => {
    expect(moveModeKey(key(k, mods))).toEqual(expected);
  });
});

describe("pane drag: motion", () => {
  it("prefers-reduced-motion turns every movement into an opacity crossfade", () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: query === "(prefers-reduced-motion: reduce)", media: query }));
    expect(prefersReducedMotion()).toBe(true);
    for (const kind of ["zone", "settle", "cancel", "lift"] as const) {
      const value = transitionFor(kind);
      expect(value).toContain("opacity var(--shell-motion-reduced-fade-ms)");
      expect(value).not.toMatch(/transform|width|height|left|top/);
    }
  });
  it("full motion moves boxes on the token timing and curve", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    expect(prefersReducedMotion()).toBe(false);
    const zone = transitionFor("zone"), settle = transitionFor("settle");
    for (const part of ["transform", "width", "height"]) {
      expect(zone).toContain(`${part} var(--shell-motion-zone-morph-ms) var(--shell-motion-ease)`);
      expect(settle).toContain(`${part} var(--shell-motion-settle-ms) var(--shell-motion-ease)`);
    }
    expect(transitionFor("cancel")).toContain("transform var(--shell-motion-cancel-ms) var(--shell-motion-ease)");
    expect(transitionFor("lift")).toContain("opacity var(--shell-motion-fade-ms) var(--shell-motion-ease)");
    expect(transitionFor("lift")).not.toMatch(/transform/);
  });
});
