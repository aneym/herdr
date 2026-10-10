// @vitest-environment happy-dom
// Integration scenario: selection reveals a hidden agent in the mounted Sidebar,
// persists the fold, and respects a user's later collapse across snapshot polls.
// Only the native bridge boundary is mocked; selection uses the parent's real memo.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
vi.mock("./bridge", () => ({ bridge: { api: async () => ({}), updateStatus: () => new Promise(() => {}), fileList: async () => [], fileRead: async () => ({ data_b64: "" }) }, fromBase64: () => new Uint8Array() }));
const { default: Sidebar, useSelectionReveal } = await import("./Sidebar");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// Node's own localStorage can shadow happy-dom's without --localstorage-file.
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => void stored.set(key, value), clear: () => stored.clear() } });
function story(): Snapshot {
  return {
    workspaces: [{ workspace_id: "w1", number: 1, label: "home space" }],
    tabs: [
      { tab_id: "A", workspace_id: "w1", number: 1, label: "alpha", role: "agent", pin_index: 0 },
      { tab_id: "B", workspace_id: "w1", number: 2, label: "bravo", role: "agent", pin_index: 1, hidden: true },
      { tab_id: "C", workspace_id: "w1", number: 3, label: "charlie", role: "agent", pin_index: 2 },
    ],
  };
}
describe("hidden agent selection reveal", () => {
  let host: HTMLDivElement, root: Root;
  function Shell({ selected, snapshot, shown = true }: { selected: string | null; snapshot: Snapshot; shown?: boolean }) {
    const revealed = useSelectionReveal(selected);
    const noop = () => {};
    return shown ? <Sidebar rows={buildSidebar(snapshot)} selected={selected} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} /> : null;
  }
  const mount = (selected: string | null, shown = true) => act(() => root.render(<Shell selected={selected} snapshot={story()} shown={shown} />));
  const header = () => host.querySelector<HTMLButtonElement>('[data-row="hiddenagents"]')!;
  const hiddenRow = () => host.querySelector('[data-row="agent:B"]');
  beforeEach(() => { localStorage.clear(); host = document.createElement("div"); document.body.append(host); root = createRoot(host); });
  afterEach(() => { act(() => root.unmount()); host.remove(); localStorage.clear(); });
  it("opens and saves Hidden when selection changes to a hidden agent", () => {
    mount("A");
    expect(header().getAttribute("aria-expanded")).toBe("false");
    expect(hiddenRow()).toBeNull();
    mount("B");
    expect(hiddenRow()).not.toBeNull();
    expect(header().getAttribute("aria-expanded")).toBe("true");
    expect(localStorage.getItem("herdr-shell.areas.hiddenAgents")).toBe("true");
  });
  it("keeps the user's collapse with the same selection across new rows and sidebar hide/show", () => {
    mount("A");
    mount("B");
    expect(header().getAttribute("aria-expanded")).toBe("true");
    act(() => header().click());
    mount("B");
    expect(header().getAttribute("aria-expanded")).toBe("false");
    expect(hiddenRow()).toBeNull();
    expect(localStorage.getItem("herdr-shell.areas.hiddenAgents")).toBe("false");
    mount("B", false);
    mount("B");
    expect(header().getAttribute("aria-expanded")).toBe("false");
    expect(hiddenRow()).toBeNull();
  });
  it("does not open Hidden when selection changes to a visible agent", () => {
    mount(null);
    mount("C");
    expect(header().getAttribute("aria-expanded")).toBe("false");
    expect(hiddenRow()).toBeNull();
    expect(localStorage.getItem("herdr-shell.areas.hiddenAgents")).not.toBe("true");
  });
});
