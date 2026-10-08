import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Layout } from "./model";
// Review-mandated controller integration scenarios: only the Tauri/server edge and clock are faked.
const edge = vi.hoisted(() => ({ api: vi.fn(), snapshot: vi.fn() }));
vi.mock("./bridge", () => ({ bridge: edge }));
import { PaneDrag } from "./paneDrag";
const area = { x: 0, y: 0, width: 120, height: 40 };
const layout = (tab_id: string, ids: string[]): Layout => ({ tab_id, area, panes: ids.map((pane_id, i) => ({ pane_id, rect: { x: i * 60, y: 0, width: 60, height: 40 } })) });
const origin = layout("origin", ["A", "B"]), target = layout("target", ["X", "Y"]), foreign = layout("foreign", ["M", "N"]);
const point = { x: -100, y: 120 };
const focusCalls = () => edge.api.mock.calls.filter(c => c[1] === "tab.focus").map(c => c[2].tab_id);
let view: string, known: Set<string>, drag: PaneDrag;
function start() {
  expect(drag.press({ pane: "A", label: "shell", point: { x: 300, y: 10 }, layout: origin, size: { width: 1200, height: 800 }, supported: true })).toBe(true);
  drag.move({ x: 300, y: 60 });
}
function spring(to: Layout) {
  drag.move(point, { kind: "tab", tab_id: to.tab_id });
  vi.advanceTimersByTime(450);
  drag.layoutChanged(to);
}
beforeEach(() => {
  vi.useFakeTimers(); edge.api.mockReset(); edge.snapshot.mockReset();
  edge.api.mockResolvedValue({}); edge.snapshot.mockResolvedValue({ tabs: [{ tab_id: "foreign", focused: true }] });
  view = "origin"; known = new Set(["origin", "target", "foreign"]);
  drag = new PaneDrag("studio", { onSpring: id => {
    if (!known.has(id)) throw new Error(`Unknown tab: ${id}`);
    view = id;
  } });
  start();
});
afterEach(() => { drag.cancel(); vi.useRealTimers(); });
describe("spring edge recovery", () => {
  it("1: a closed origin cannot stick cancel or a later press, or send a restore", () => {
    spring(target); known.delete("origin");
    expect(() => drag.cancel()).not.toThrow();
    expect(drag.state.phase).toBe("idle"); expect(focusCalls()).toEqual(["target"]);
    expect(view).toBe("target"); expect(() => drag.cancel()).not.toThrow();
    expect(() => start()).not.toThrow();
  });
  it("2: a rejected spring after the view switched ends quietly and resyncs server focus", async () => {
    let reject!: (error: unknown) => void;
    edge.api.mockReturnValueOnce(new Promise((_resolve, no) => { reject = no; }));
    spring(target); reject(new Error("closed target"));
    await vi.waitFor(() => expect(drag.state.phase).toBe("idle"));
    expect(drag.state.end).toBe("quiet"); expect(view).toBe("foreign");
    expect(edge.snapshot).toHaveBeenCalledWith("studio"); expect(focusCalls()).toEqual(["target"]);
  });
  it("3: a foreign tab switch ends without restoring or undoing that view", () => {
    spring(target); view = "foreign"; drag.layoutChanged(foreign);
    expect(drag.state.phase).toBe("idle"); expect(view).toBe("foreign");
    expect(focusCalls()).toEqual(["target"]);
  });
  it("4: cancel after springing back to origin sends no redundant focus", () => {
    spring(target); spring(origin); drag.cancel();
    expect(drag.state.phase).toBe("idle"); expect(view).toBe("origin");
    expect(focusCalls()).toEqual(["target", "origin"]);
  });
  it.each(["changed:false", "error"])("5: refused drop (%s) restores origin", async refusal => {
    spring(target);
    edge.api.mockImplementation((_machine, method) => method === "pane.place"
      ? refusal === "error" ? Promise.reject(new Error("refused")) : Promise.resolve({ place: { changed: false } })
      : Promise.resolve({}));
    drag.move(point, { kind: "tab", tab_id: "foreign" });
    expect(drag.release()).toBe("dropped");
    await vi.waitFor(() => expect(drag.state.phase).toBe("idle"));
    expect(view).toBe("origin"); expect(focusCalls()).toEqual(["target", "origin"]);
  });
  it("6: source tab row is never an into-tab zone, but still springs back", () => {
    spring(target);
    drag.move(point, { kind: "tab", tab_id: "origin" });
    expect(drag.state.zone).toBeNull();
    vi.advanceTimersByTime(450); drag.layoutChanged(origin);
    expect(view).toBe("origin"); expect(focusCalls()).toEqual(["target", "origin"]);
    expect(drag.state.phase).toBe("dragging"); expect(drag.state.zone).toBeNull();
  });
});
