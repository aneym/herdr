import { describe, expect, it } from "vitest";
import { buildSidebar, revealFold, spaceOpen, tabStatus } from "./model";
import type { Snapshot } from "./model";
import { encodeShiftEnter } from "./keys";
// Pure ranking/deduplication has interacting edge cases: this inline contract table
// protects section order, status authority and numbering without a test-only seam.
// The S0a demo has no sidebar coverage; wrong ordering changes Ctrl+N's target.
const snapshot: Snapshot = {
  workspaces: [
    { workspace_id: "w1", number: 1, label: "ordinary", tokens: {} },
    { workspace_id: "w2", number: 2, label: "pinned space", tokens: { pinned: "true" } },
    { workspace_id: "w3", number: 3, label: "secret", tokens: { hidden: "true" } },
  ],
  tabs: [
    { tab_id: "w1:t1", workspace_id: "w1", number: 1, label: "agent", role: "agent", pin_index: 1, work_status: "working" },
    { tab_id: "w2:t1", workspace_id: "w2", number: 1, label: "pinned", pin_index: 0, work_status: "done" },
    ...Array.from({ length: 9 }, (_, i) => ({ tab_id: `w1:t${i + 2}`, workspace_id: "w1", number: i + 2, agent_status: "idle" })),
    { tab_id: "w3:t1", workspace_id: "w3", number: 1, work_status: "blocked" },
  ],
  panes: [{ pane_id: "p1", terminal_id: "term1", workspace_id: "w1", tab_id: "w1:t2", terminal_title_stripped: "shell" }],
  agents: [{ terminal_id: "term1", pane_id: "p1", workspace_id: "w1", tab_id: "w1:t2", agent: "codex", agent_status: "blocked" }],
};
describe("sidebar contract", () => {
  it("orders sections, excludes agents from spaces and numbers unique tabs at most nine", () => {
    const rows = buildSidebar(snapshot);
    // As the Mac's pinTabs: a pinned agent is listed once, in AGENTS, never again in PINNED.
    expect(rows.slice(0, 3).map(r => [r.kind, r.id, r.hotkey])).toEqual([["agent", "w1:t1", 1], ["pinned", "w2:t1", 2], ["space", "w2", null]]);
    expect(rows.filter(r => r.kind === "space").map(r => [r.id, r.status, !!r.hidden])).toEqual([["w2", "done", false], ["w1", "blocked", false], ["w3", "blocked", true]]);
    expect(rows.filter(r => r.kind === "tab").map(r => [r.id, r.hotkey])).toEqual([["w2:t1", 2], ["w1:t2", 3], ["w1:t3", 4], ["w1:t4", 5], ["w1:t5", 6], ["w1:t6", 7], ["w1:t7", 8], ["w1:t8", 9], ["w1:t9", null], ["w1:t10", null], ["w3:t1", null]]);
    expect(rows.find(r => r.id === "w1:t2")?.label).toBe("shell");
    expect(rows.filter(r => r.hidden).map(r => r.id)).toEqual(["w3", "w3:t1"]);
    expect(buildSidebar({})).toEqual([]);
    for (const [statuses, expected] of [
      [["idle", "done", "working", "blocked"], "blocked"],
      [["done", "idle", "working"], "working"],
      [["idle", "done"], "done"],
      [["idle"], "idle"],
    ] as const) {
      const ranked: Snapshot = { workspaces: snapshot.workspaces?.slice(0, 1), tabs: statuses.map((work_status, number) => ({ tab_id: `w1:t${number}`, workspace_id: "w1", number, work_status })) };
      expect(buildSidebar(ranked).find(r => r.kind === "space")?.status).toBe(expected);
    }
  });
  // Priority order and parking interact with the pin partition, the hidden group and the user's
  // folds; the Mac's PRIORITY-ORDER check holds the same contract for its tree.
  it("ranks spaces and tabs by the server's sort_rank inside the pin partition, and parks spaces folded", () => {
    const ranked: Snapshot = {
      workspaces: [
        { workspace_id: "a", number: 1, label: "a", sort_rank: 5 },
        { workspace_id: "b", number: 2, label: "b", sort_rank: 1 },
        { workspace_id: "p", number: 3, label: "pinned", sort_rank: 9, tokens: { pinned: "true" } },
        { workspace_id: "r", number: 4, label: "rails", sort_rank: 9, parked: true },
        { workspace_id: "c", number: 5, label: "c" },
      ],
      tabs: [
        { tab_id: "b:1", workspace_id: "b", number: 1, sort_rank: 2 },
        { tab_id: "b:2", workspace_id: "b", number: 2, sort_rank: 0 },
        { tab_id: "b:3", workspace_id: "b", number: 3 },
        { tab_id: "r:1", workspace_id: "r", number: 1, work_status: "working" },
      ],
    };
    const rows = buildSidebar(ranked);
    expect(rows.filter(r => r.kind === "space").map(r => r.id)).toEqual(["p", "c", "b", "a", "r"]);
    expect(rows.filter(r => r.kind === "tab").map(r => r.id)).toEqual(["b:2", "b:3", "b:1", "r:1"]);
    const space = (id: string) => rows.find(r => r.kind === "space" && r.id === id)!;
    // A parked space stays folded through live work and selection, and keeps its own fold key.
    expect(space("r").parked).toBe(true);
    expect(spaceOpen(space("r"), rows, "r:1", {})).toBe(false);
    expect(spaceOpen(space("r"), rows, null, { "parked:r": true })).toBe(true);
    expect(spaceOpen(space("r"), rows, null, { r: true })).toBe(false);
    expect(spaceOpen(space("b"), rows, "b:1", {})).toBe(true);
    expect(spaceOpen(space("b"), rows, "b:1", { b: false })).toBe(false);
    // Selecting a tab the tree hides opens its space; a shown one needs nothing.
    expect(revealFold(rows, "r:1", {})).toBe("parked:r");
    expect(revealFold(rows, "r:1", { "parked:r": true })).toBeNull();
    expect(revealFold(rows, "b:1", { b: false })).toBe("b");
    expect(revealFold(rows, "b:1", {})).toBeNull();
    expect(revealFold(rows, "missing", {})).toBeNull();
  });
  it("uses work status before first agent, then tab agent status and unknown", () => {
    const tab = { tab_id: "w1:t2", workspace_id: "w1", number: 2, agent_status: "done" };
    expect(tabStatus(snapshot, { ...tab, work_status: "idle" })).toBe("idle");
    expect(tabStatus(snapshot, tab)).toBe("blocked");
    expect(tabStatus({}, tab)).toBe("done");
    expect(tabStatus({}, { tab_id: "x", workspace_id: "w", number: 1 })).toBe("unknown");
  });
});
// Prompt bytes are an independent terminal protocol contract; each branch must
// preserve its exact encoding, including kitty precedence when both are active.
it.each([
  [{ kittyFlags: 1, modifyOtherKeys: 2 }, "\x1b[13;2u"],
  [{ kittyFlags: 0, modifyOtherKeys: 2 }, "\x1b[27;2;13~"],
  [{ kittyFlags: 0, modifyOtherKeys: 1 }, "\x1b\r"],
])("encodes Shift+Enter for %j", (mode, expected) => { expect(encodeShiftEnter(mode)).toBe(expected); });
