// Lead ruling 2026-10-07 (agents-hide W2): next/previous tab skips hidden agents, as Cmd+E and the
// digits do, on every client. A wrong step lands on a chat the person chose to hide.
import { describe, expect, it, vi } from "vitest";
import { runAction } from "./actions";
import type { ActionContext } from "./actions";
import { buildSidebar } from "./model";
import type { Snapshot, Tab } from "./model";

function story(hiddenB = true): Snapshot {
  const tab = (tab_id: string, number: number, base: Partial<Tab>): Tab => ({ tab_id, workspace_id: "w1", number, ...base });
  return {
    workspaces: [{ workspace_id: "w1", number: 1, label: "home space" }],
    tabs: [
      tab("A", 1, { label: "alpha", role: "agent", pin_index: 0 }),
      tab("B", 2, { label: "bravo", role: "agent", pin_index: 1, hidden: hiddenB }),
      tab("C", 3, { label: "charlie", role: "agent", pin_index: 2 }),
      tab("P", 4, { label: "plain", pin_index: 3 }),
    ],
    panes: [],
  };
}
async function step(action: "next_tab" | "prev_tab", selected: string, snapshot = story()): Promise<string | undefined> {
  const select = vi.fn();
  const noop = () => {};
  const ctx: ActionContext = { machine: "studio", snapshot, rows: buildSidebar(snapshot), selected, focused: null, api: async () => ({}), select, focus: noop, created: noop, rename: noop, switcher: noop, toggleSidebar: noop, error: e => { throw e; } };
  await runAction(action, ctx);
  return select.mock.calls.at(-1)?.[0];
}

describe("next and previous tab skip hidden agents", () => {
  it("steps over the hidden agent in both directions", async () => {
    expect(await step("next_tab", "A")).toBe("C");
    expect(await step("prev_tab", "C")).toBe("A");
  });
  it("visits the agent again once it is shown", async () => {
    expect(await step("next_tab", "A", story(false))).toBe("B");
    expect(await step("prev_tab", "C", story(false))).toBe("B");
  });
  it("leaves a selected hidden agent for a visible tab, never staying on or returning to it", async () => {
    const next = await step("next_tab", "B");
    const prev = await step("prev_tab", "B");
    for (const id of [next, prev]) expect(["A", "C", "P"]).toContain(id);
  });
});
