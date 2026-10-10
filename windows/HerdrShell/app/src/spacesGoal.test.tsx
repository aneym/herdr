// @vitest-environment happy-dom
// Mac parity (SpacesTree.swift, SpacesRowView.swift): Spaces mode draws a "goal All" filter row when
// the factory overlay names goals, space-group titles above their spaces, and sections inside each
// space: ORCHESTRATOR, then the stage sections when the space is sectioned, else LANES.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import type { SpacesOverlay } from "./spacesOverlay";
vi.mock("./bridge", () => ({ bridge: { api: async () => ({}), updateStatus: () => new Promise(() => {}), fileList: async () => [], fileRead: async () => ({ data_b64: "" }) }, fromBase64: () => new Uint8Array() }));
const { default: Sidebar, useSelectionReveal } = await import("./Sidebar");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => void stored.set(k, v), clear: () => stored.clear() } });

const snapshot: Snapshot = {
  workspaces: [{ workspace_id: "w1", number: 1, label: "rails" }, { workspace_id: "w2", number: 2, label: "notes" }],
  tabs: [
    { tab_id: "o", workspace_id: "w1", number: 1, label: "orchestrator" },
    { tab_id: "r", workspace_id: "w1", number: 2, label: "review lane" },
    { tab_id: "b", workspace_id: "w1", number: 3, label: "build lane" },
    { tab_id: "c", workspace_id: "w1", number: 4, label: "closer lane" },
    { tab_id: "f", workspace_id: "w1", number: 5, label: "checks flow" },
    { tab_id: "n", workspace_id: "w2", number: 1, label: "scratch" },
  ],
};
// The overlay as the shell holds it once parsed (spacesOverlay.parseOverlay normalises stage names).
const overlay: SpacesOverlay = {
  tabs: {
    o: { kind: "orchestrator", mode: "active", done: false },
    r: { kind: "lane", mode: "active", section: "reviewing", goal: "rails", goalArea: "workspace ui", done: false },
    b: { kind: "lane", mode: "active", section: "implementing", goal: "rails", done: false },
    c: { kind: "lane", mode: "active", section: "implementing", goal: "closer", done: false },
    f: { kind: "workflow", mode: "active", parent: "b", done: false },
  },
  hosts: [],
  spaceGroups: [{ name: "Rails", spaces: ["rails"] }],
};

describe("spaces mode goal row and sections", () => {
  let host: HTMLDivElement, root: Root;
  function Shell() {
    const revealed = useSelectionReveal(null);
    const noop = () => {};
    return <Sidebar snapshot={snapshot} overlay={overlay} rows={buildSidebar(snapshot)} selected={null} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />;
  }
  const order = () => [...host.querySelectorAll("[data-row], [data-space]")].map(e => e.getAttribute("data-row") ?? `space:${e.getAttribute("data-space")}`);
  beforeEach(() => {
    stored.clear(); stored.set("herdr-shell.areas.mode", JSON.stringify("spaces")); stored.set("herdr-space-expanded", JSON.stringify({ w1: true, w2: true }));
    host = document.createElement("div"); document.body.append(host); root = createRoot(host); act(() => root.render(<Shell />));
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); });

  it("draws the goal row, the space group title and each space's sections in Mac order", () => {
    expect(host.querySelector('[data-row="goal"]')?.textContent).toBe("goalAll");
    expect(order()).toEqual([
      "goal", "spacegroup:Rails", "space:w1",
      "section:w1:ORCHESTRATOR", "tab:o",
      "section:w1:READY FOR REVIEW", "tab:r",
      "section:w1:IMPLEMENTING", "tab:b", "tab:c",
      "space:w2", "section:w2:LANES", "tab:n",
    ]);
    expect(host.querySelector('[data-row="section:w1:READY FOR REVIEW"]')?.textContent).toBe("READY FOR REVIEW1");
    // The workflow nests under its lane, folded, and the lane says how many it holds.
    expect(host.querySelector('[data-row="tab:b"]')?.textContent).toContain("1 workflow");
    act(() => host.querySelector<HTMLButtonElement>('[data-row="tab:b"] [aria-label="Fold build lane"]')!.click());
    expect(order()).toContain("tab:f");
  });

  it("filters a sectioned space by the chosen goal, keeping the orchestrator, and folds a section", () => {
    act(() => host.querySelector<HTMLButtonElement>(".goal-pick")!.click());
    const items = [...host.querySelectorAll('.goal-menu [role="menuitemradio"]')].map(e => e.textContent);
    expect(items).toEqual(["All", "closer", "rails", "rails · workspace ui"]);
    act(() => [...host.querySelectorAll<HTMLButtonElement>('.goal-menu [role="menuitemradio"]')].find(e => e.textContent === "closer")!.click());
    expect(host.querySelector('[data-row="goal"]')?.textContent).toContain("closer");
    expect(order().filter(id => id.startsWith("tab:"))).toEqual(["tab:o", "tab:c", "tab:n"]);
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Fold IMPLEMENTING"]')!.click());
    expect(order()).not.toContain("tab:c");
    expect(host.querySelector('[data-row="section:w1:IMPLEMENTING"]')?.textContent).toBe("IMPLEMENTING1");
  });
});
