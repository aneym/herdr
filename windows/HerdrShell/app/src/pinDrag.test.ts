import { describe, expect, it } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import { PENDING_LIFETIME_MS, pinMovePlan, slotAt } from "./pinDrag";
// Drop-slot geometry and the slot-to-pin_index mapping are pure rules with interacting edge
// cases (section ends, role blocks sharing one server numbering, a pending permutation). A
// wrong answer sends tab.pin_move to the wrong index and reorders Alex's real pins; the
// e2e check (scripts/check_pin_drag.py) proves one drag on the PC, this table the rules.
const rows = [0, 1, 2].map(i => ({ top: 100 + 26 * i, bottom: 126 + 26 * i, left: 8, right: 252 }));
const snapshot: Snapshot = {
  workspaces: [{ workspace_id: "w1", number: 1 }],
  tabs: [
    { tab_id: "a1", workspace_id: "w1", number: 1, role: "agent", pin_index: 0 },
    { tab_id: "a2", workspace_id: "w1", number: 2, role: "agent", pin_index: 1 },
    { tab_id: "p1", workspace_id: "w1", number: 3, pin_index: 2 },
    { tab_id: "p2", workspace_id: "w1", number: 4, pin_index: 3 },
    { tab_id: "p3", workspace_id: "w1", number: 5, pin_index: 4 },
  ],
};
describe("pin drag", () => {
  it.each([
    ["inside the first row", 130, 110, 0],
    ["inside the last row", 130, 175, 2],
    ["just above the section", 130, 90, 0],
    ["just below the section", 130, 190, 2],
    ["more than a row above", 130, 70, null],
    ["more than a row below", 130, 205, null],
    ["off the sidebar column", 300, 130, null],
  ])("slot %s", (_, x, y, slot) => { expect(slotAt(rows, x, y)).toBe(slot); });
  it("has no slot without a block", () => { expect(slotAt([], 0, 0)).toBeNull(); });
  it("maps a section slot to that role block's pin_index", () => {
    expect(pinMovePlan(snapshot, ["p1", "p2", "p3"], 2, 0)).toEqual({ tab: "p3", pinIndex: 2, order: ["p3", "p1", "p2"], section: "pinned" });
    expect(pinMovePlan(snapshot, ["p1", "p2", "p3"], 0, 2)).toEqual({ tab: "p1", pinIndex: 4, order: ["p2", "p3", "p1"], section: "pinned" });
    expect(pinMovePlan(snapshot, ["a1", "a2"], 1, 0)).toEqual({ tab: "a2", pinIndex: 0, order: ["a2", "a1"], section: "agent" });
    // A pending permutation still names slots by position.
    expect(pinMovePlan(snapshot, ["p3", "p1", "p2"], 0, 1)?.pinIndex).toBe(3);
  });
  it("refuses a move in place, out of range or across role blocks", () => {
    expect(pinMovePlan(snapshot, ["p1", "p2", "p3"], 1, 1)).toBeNull();
    expect(pinMovePlan(snapshot, ["p1", "p2", "p3"], 0, 3)).toBeNull();
    expect(pinMovePlan(snapshot, ["a2", "p1"], 0, 1)).toBeNull();
  });
  it("shows a dropped order until the snapshot agrees or the order expires", () => {
    const pending = { pinned: { order: ["p3", "p1", "p2"], at: 1000 } };
    const ids = (now: number) => buildSidebar(snapshot, pending, now).filter(r => r.kind === "pinned").map(r => [r.id, r.hotkey]);
    expect(ids(1000)).toEqual([["p3", 3], ["p1", 4], ["p2", 5]]);
    expect(ids(1000 + PENDING_LIFETIME_MS)).toEqual([["p1", 3], ["p2", 4], ["p3", 5]]);
    // An order for a different set of pins (one was unpinned meanwhile) is not applied.
    expect(buildSidebar(snapshot, { pinned: { order: ["p3", "p1"], at: 1000 } }, 1000).filter(r => r.kind === "pinned").map(r => r.id)).toEqual(["p1", "p2", "p3"]);
  });
});
