import { describe, expect, it } from "vitest";
import { latestAttentionTab, neighbor, runAction, type ActionContext } from "./actions";
import type { Layout, Snapshot } from "./model";
const pane = (pane_id: string, x: number, y: number, width = 10, height = 10) => ({ pane_id, rect: { x, y, width, height } });
const layout = (panes: Layout["panes"]): Layout => ({ tab_id: "t", area: { x: 0, y: 0, width: 100, height: 100 }, panes });
// Pure geometric ranking needs adversarial gaps, overlap, ties and zoom cases.
// These expected neighbors are independent of array order and protect Mac parity.
describe("directional layout navigation", () => {
  const cross = layout([pane("center", 10, 10), pane("left", 0, 10), pane("right", 20, 10), pane("up", 10, 0), pane("down", 10, 20), pane("diagonal", 20, 20)]);
  it.each(["left", "right", "up", "down"] as const)("selects the facing %s pane", dir => expect(neighbor(cross, "center", dir)).toBe(dir));
  it("does not wrap, navigate hidden zoom panes, or accept missing sources", () => {
    expect(neighbor(cross, "left", "left")).toBeNull();
    expect(neighbor(cross, "missing", "right")).toBeNull();
    expect(neighbor({ ...cross, zoomed: true }, "center", "right")).toBeNull();
    expect(neighbor(layout([pane("only", 0, 0)]), "only", "down")).toBeNull();
  });
  it("prefers distance, then greater overlap regardless of pane order", () => {
    const panes = [pane("source", 0, 0, 10, 20), pane("far", 30, 0, 10, 20), pane("small", 10, 0, 10, 5), pane("large", 10, 5, 10, 15), pane("corner", 10, 20)];
    expect(neighbor(layout(panes), "source", "right")).toBe("large");
    expect(neighbor(layout([...panes].reverse()), "source", "right")).toBe("large");
  });
  it("allows subpixel edge tolerance but rejects overlapping and non-overlapping candidates", () => {
    expect(neighbor(layout([pane("s", 0, 0), pane("near", 9.6, 0), pane("overlap", 9, 0), pane("diagonal", 10, 10)]), "s", "right")).toBe("near");
    expect(neighbor(layout([pane("s", 0, 0), pane("overlap", 9, 0), pane("diagonal", 10, 10)]), "s", "right")).toBeNull();
  });
});

// Pure trail ranking has deleted ids, hidden agents and exhausted-trail edge cases;
// dispatch also exercises the real fallback action rather than a duplicate algorithm.
describe("attention trail navigation", () => {
  it("skips deleted tabs, then falls back to next attention when no trail tab survives", async () => {
    const snapshot: Snapshot = { tabs: [
      { tab_id: "hidden", workspace_id: "w", number: 1, role: "agent", hidden: true },
      { tab_id: "live", workspace_id: "w", number: 2 },
      { tab_id: "blocked", workspace_id: "w", number: 3 },
    ] };
    expect(latestAttentionTab(["deleted", "hidden", "live"], snapshot)).toBe("live");
    expect(latestAttentionTab(["deleted", "hidden"], snapshot)).toBeUndefined();
    const selected: string[] = [];
    const ctx: ActionContext = {
      machine: "test", snapshot, selected: null, focused: null,
      rows: [{ kind: "tab", id: "blocked", label: "Blocked", status: "blocked", hotkey: null, section: "w" }],
      attentionTrail: ["deleted", "hidden", "live"],
      select: id => { selected.push(id); }, api: async () => ({}),
      focus: () => {}, created: () => {}, rename: () => {}, switcher: () => {}, toggleSidebar: () => {},
      error: error => { throw error; },
    };
    await runAction("attention_jump", ctx);
    await runAction("attention_jump", { ...ctx, attentionTrail: ["deleted", "hidden"] });
    expect(selected).toEqual(["live", "blocked"]);
  });
});
