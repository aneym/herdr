// @vitest-environment happy-dom
// Mounted sidebar scenario: header actions cross only the native bridge boundary,
// and creation waits for the server snapshot before selecting an unknown tab.
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
const api = vi.hoisted(() => vi.fn());
vi.mock("./bridge", () => ({ bridge: { api, updateStatus: () => new Promise(() => {}), fileList: async () => [], fileRead: async () => ({ data_b64: "" }) }, fromBase64: () => new Uint8Array() }));
const { default: Sidebar, useSelectionReveal } = await import("./Sidebar");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => void stored.set(key, value), clear: () => stored.clear() } });
const story = (): Snapshot => ({
  workspaces: [{ workspace_id: "w1", number: 1, label: "Home" }, { workspace_id: "w2", number: 2, label: "Work", tokens: { pinned: "true" } }],
  tabs: [{ tab_id: "A", workspace_id: "w2", number: 1, label: "alpha", pin_index: 0 }],
});
describe("Spaces sidebar header actions", () => {
  let host: HTMLDivElement, root: Root;
  function Shell({ snapshot, initial = "A" }: { snapshot: Snapshot; initial?: string | null }) {
    const [selected, select] = useState(initial);
    const revealed = useSelectionReveal(selected);
    const noop = () => {};
    return <><output>{selected}</output><Sidebar snapshot={snapshot} machines={[]} chooseMachine={noop} rows={buildSidebar(snapshot)} selected={selected} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={null} select={id => { if (!snapshot.tabs?.some(tab => tab.tab_id === id)) throw new Error("Unknown tab"); select(id); }} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} /></>;
  }
  const mount = (snapshot: Snapshot, initial?: string | null) => act(() => root.render(<Shell snapshot={snapshot} initial={initial} />));
  const click = async (label: string) => { await act(async () => { const button = host.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`); expect(button).not.toBeNull(); button!.click(); }); };
  beforeEach(() => {
    stored.clear(); api.mockReset();
    api.mockImplementation(async (_machine: string, method: string) => method === "tab.create" ? { tab: { tab_id: "NEW" } } : method === "tab.list" ? { tabs: [{ pin_index: 0 }, { pin_index: 1 }, { pin_index: 2 }, {}] } : {});
    host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); stored.clear(); });
  it("creates in the selected workspace, pins at the end, then selects when the tab appears", async () => {
    const snapshot = story(); mount(snapshot);
    await click("New pinned tab");
    expect(api.mock.calls).toEqual([
      ["studio", "tab.create", { workspace_id: "w2", focus: false }],
      ["studio", "tab.set_pinned", { tab_id: "NEW", pinned: true }],
      ["studio", "tab.list", {}],
      ["studio", "tab.pin_move", { tab_id: "NEW", pin_index: 2 }],
    ]);
    expect(host.querySelector("output")?.textContent).toBe("A");
    mount({ ...snapshot, tabs: [...snapshot.tabs!, { tab_id: "NEW", workspace_id: "w2", number: 2, pin_index: 2 }] });
    expect(host.querySelector("output")?.textContent).toBe("NEW");
    expect(host.querySelector('[data-row="pinned:NEW"]')?.classList.contains("selected")).toBe(true);
  });
  it("offers PINNED creation with no pins and falls back to the first workspace", async () => {
    mount({ ...story(), tabs: [] }, null);
    await click("New pinned tab");
    expect(api.mock.calls[0]).toEqual(["studio", "tab.create", { workspace_id: "w1", focus: false }]);
  });
  it("creates in the space without pinning or folding it, then selects the new tab", async () => {
    const snapshot = story(); mount(snapshot);
    const fold = host.querySelector('[aria-label="Fold Home"]')!;
    expect(fold.getAttribute("aria-expanded")).toBe("false");
    await click("New tab in Home");
    expect(api.mock.calls).toEqual([["studio", "tab.create", { workspace_id: "w1", focus: false }]]);
    expect(fold.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector("output")?.textContent).toBe("A");
    mount({ ...snapshot, tabs: [...snapshot.tabs!, { tab_id: "NEW", workspace_id: "w1", number: 2 }] });
    expect(host.querySelector("output")?.textContent).toBe("NEW");
    expect(host.querySelector('[aria-label="Fold Home"]')?.getAttribute("aria-expanded")).toBe("true");
  });
  it("pins and unpins the owning space without toggling its fold", async () => {
    mount(story());
    await click("Pin space"); await click("Unpin space");
    expect(api.mock.calls).toEqual([
      ["studio", "workspace.set_pinned", { workspace_id: "w1", pinned: true }],
      ["studio", "workspace.set_pinned", { workspace_id: "w2", pinned: false }],
    ]);
    expect(host.querySelector('[aria-label="Fold Home"]')?.getAttribute("aria-expanded")).toBe("false");
  });
  it("reports a failed pin without selecting the created tab", async () => {
    api.mockImplementation(async (_machine: string, method: string) => { if (method === "tab.set_pinned") throw new Error("pin failed"); return { tab: { tab_id: "NEW" } }; });
    const snapshot = story(); mount(snapshot); await click("New pinned tab");
    mount({ ...snapshot, tabs: [...snapshot.tabs!, { tab_id: "NEW", workspace_id: "w2", number: 2 }] });
    expect(host.querySelector("output")?.textContent).toBe("A");
    expect(host.querySelector('footer[role="status"]')?.textContent).toContain("pin failed");
    expect(api).toHaveBeenCalledTimes(2);
  });
});
