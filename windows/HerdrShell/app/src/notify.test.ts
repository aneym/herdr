import { describe, expect, it } from "vitest";
import type { Snapshot } from "./model";
import { observeAttention } from "./notify";

// Pure transition algorithm: pane identity, focus, parking, priority and time gates
// have distinct edge cases; no existing shell test owns notification decisions.
const snapshot = (status: string, hidden = false): Snapshot => ({
  tabs: [{ tab_id: "t", workspace_id: "w", number: 1, label: "Build", hidden }],
  panes: [{ pane_id: "p", terminal_id: "term", workspace_id: "w", tab_id: "t", agent_status: status, title: " compiler " }],
});
const observe = (before: Snapshot | undefined, after: Snapshot, selected: string | null = null, focused = true, lastSent: Record<string, number> = {}, now = 100_000, parked: string[] = []) =>
  observeAttention(before, after, selected, focused, new Set(parked), lastSent, now);

describe("Mac unseen-tab notification contract", () => {
  it.each([
    ["working", "blocked", "blocked", "needs you compiler"],
    ["unknown", "blocked", "blocked", "needs you compiler"],
    ["working", "done", "done", "finished compiler"],
  ])("posts one notification for %s → %s", (before, after, kind, body) => {
    expect(observe(snapshot(before), snapshot(after)).notifications).toEqual([{ tab: "t", kind, title: "Build", body }]);
  });
  it.each([["blocked", "blocked"], ["done", "done"], ["unknown", "done"], ["working", "request"]])("does not notify for %s → %s", (before, after) => {
    expect(observe(snapshot(before), snapshot(after)).notifications).toEqual([]);
  });
  it("does not notify on the first snapshot or for a new pane", () => {
    expect(observe(undefined, snapshot("blocked")).notifications).toEqual([]);
    expect(observe({ panes: [] }, snapshot("blocked")).notifications).toEqual([]);
  });
  it("suppresses the selected tab only while focused", () => {
    expect(observe(snapshot("working"), snapshot("blocked"), "t").notifications).toEqual([]);
    expect(observe(snapshot("working"), snapshot("blocked"), "t", false).notifications).toHaveLength(1);
    expect(observe(snapshot("working"), snapshot("blocked"), "t").attention).toBe(false);
    expect(observe(snapshot("working"), snapshot("blocked"), "t", false).attention).toBe(true);
  });
  it("includes hidden agents but excludes catalog-parked tabs", () => {
    expect(observe(snapshot("working", true), snapshot("blocked", true)).notifications).toHaveLength(1);
    const result = observe(snapshot("working"), snapshot("blocked"), null, true, {}, 100_000, ["t"]);
    expect(result.notifications).toEqual([]);
    expect(result.attention).toBe(false);
  });
  it("does not nag an already-notified tab within a minute", () => {
    expect(observe(snapshot("working"), snapshot("blocked"), null, true, { t: 50_000 }).notifications).toEqual([]);
    expect(observe(snapshot("working"), snapshot("blocked"), null, true, { t: 40_000 }).notifications).toHaveLength(1);
  });
  it("prioritizes blocked and coalesces multiple panes into one tab notification", () => {
    const before = snapshot("working"), after = snapshot("done");
    before.panes!.push({ ...before.panes![0], pane_id: "q" });
    after.panes!.push({ ...after.panes![0], pane_id: "q", agent_status: "blocked" });
    expect(observe(before, after).notifications).toEqual([{ tab: "t", kind: "blocked", title: "Build", body: "needs you compiler" }]);
  });
  it("uses agent fallback status and exact fallback title/body", () => {
    const before = snapshot("working"), after = snapshot("blocked");
    delete after.panes![0].agent_status;
    after.panes![0].title = "";
    delete after.tabs![0].label;
    after.agents = [{ pane_id: "p", terminal_id: "term", tab_id: "t", workspace_id: "w", agent: "claude", agent_status: "blocked" }];
    expect(observe(before, after).notifications).toEqual([{ tab: "t", kind: "blocked", title: "tab 1", body: "needs you" }]);
  });
});
