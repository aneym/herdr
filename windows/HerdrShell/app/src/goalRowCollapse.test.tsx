// @vitest-environment happy-dom
// Mac parity (Sidebar.swift chrome and goal row): with goals in the overlay, collapse-all sits on the
// goal row and not beside the Areas/Spaces switch; without goals it stays beside the switch.
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
  tabs: [{ tab_id: "a", workspace_id: "w1", number: 1, label: "lane" }, { tab_id: "b", workspace_id: "w2", number: 1, label: "scratch" }],
};
const goals: SpacesOverlay = { tabs: { a: { kind: "lane", mode: "active", goal: "rails", done: false } }, hosts: [], spaceGroups: [] };
const none: SpacesOverlay = { tabs: {}, hosts: [], spaceGroups: [] };

describe("collapse-all placement", () => {
  let host: HTMLDivElement, root: Root;
  function Shell({ overlay }: { overlay: SpacesOverlay }) {
    const revealed = useSelectionReveal(null);
    const noop = () => {};
    return <Sidebar snapshot={snapshot} overlay={overlay} rows={buildSidebar(snapshot)} selected={null} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />;
  }
  beforeEach(() => { stored.clear(); stored.set("herdr-shell.areas.mode", JSON.stringify("spaces")); stored.set("herdr-space-expanded", JSON.stringify({ w1: true, w2: true })); host = document.createElement("div"); document.body.append(host); root = createRoot(host); });
  afterEach(() => { act(() => root.unmount()); host.remove(); });
  const expanded = () => [...host.querySelectorAll(".space-row")].map(e => e.getAttribute("aria-expanded"));

  it("sits on the goal row when the overlay names goals, and folds every space", () => {
    act(() => root.render(<Shell overlay={goals} />));
    expect(host.querySelector(".areas-mode .spaces-fold-all")).toBeNull();
    const button = host.querySelector<HTMLButtonElement>('[data-row="goal"] .spaces-fold-all')!;
    expect(button.getAttribute("aria-label")).toBe("Collapse all spaces");
    act(() => button.click());
    expect(expanded()).toEqual(["false", "false"]);
    expect(host.querySelector('[data-row="goal"] .spaces-fold-all')!.getAttribute("aria-label")).toBe("Expand all spaces");
  });

  it("stays beside the Areas/Spaces switch when there are no goals", () => {
    act(() => root.render(<Shell overlay={none} />));
    expect(host.querySelector('[data-row="goal"]')).toBeNull();
    expect(host.querySelector(".areas-mode .spaces-fold-all")).not.toBeNull();
  });
});
