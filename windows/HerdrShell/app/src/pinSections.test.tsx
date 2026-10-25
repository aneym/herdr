// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { bridge } from "./bridge";
import Sidebar from "./Sidebar";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import { pinMovePlan } from "./pinDrag";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => void stored.set(key, value) } });
const snapshot: Snapshot = { pin_sections: ["Needs you", "Northstar: building", "Later"], workspaces: [{ workspace_id: "w", number: 1, label: "rails" }], tabs: [
  { tab_id: "a", workspace_id: "w", number: 1, label: "orchestrator", pin_index: 0 },
  { tab_id: "b", workspace_id: "w", number: 2, label: "scope review", pin_index: 1, pin_section: "Needs you" },
  { tab_id: "c", workspace_id: "w", number: 3, label: "deploy watch", pin_index: 2, pin_section: "Needs you" },
  { tab_id: "d", workspace_id: "w", number: 4, label: "runtime chat", pin_index: 3, pin_section: "Northstar: building" },
  { tab_id: "e", workspace_id: "w", number: 5, label: "old notes", pin_index: 4, pin_section: "Gone" },
] } as Snapshot;
let cleanup: () => void;
const render = async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host), noop = () => {};
  await act(async () => root.render(<Sidebar snapshot={snapshot} rows={buildSidebar(snapshot)} selected={null} revealed={{ last: undefined, pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
  cleanup = () => { act(() => root.unmount()); host.remove(); };
};
beforeEach(async () => {
  stored.set("herdr-shell.areas.mode", '"spaces"');
  vi.spyOn(bridge, "updateStatus").mockImplementation(() => new Promise(() => {}));
  await render();
});
afterEach(() => { cleanup(); stored.clear(); vi.restoreAllMocks(); });

const pinnedRows = () => [...document.querySelectorAll<HTMLElement>("section [data-row]")].map(el => el.dataset.row!).filter(id => id.startsWith("pinned:") || id.startsWith("pins:"));

it("draws unsectioned pins first, then each non-empty named section with its pins", () => {
  expect(pinnedRows()).toEqual(["pinned:a", "pinned:e", "pins:Needs you", "pinned:b", "pinned:c", "pins:Northstar: building", "pinned:d"]);
  expect(document.querySelector('[data-row="pins:Needs you"] .label')?.textContent).toBe("NEEDS YOU");
});

it("folds a section to a count and remembers the fold", async () => {
  const header = document.querySelector<HTMLElement>('[data-row="pins:Needs you"]')!;
  await act(async () => header.click());
  expect(pinnedRows()).toEqual(["pinned:a", "pinned:e", "pins:Needs you", "pins:Northstar: building", "pinned:d"]);
  expect(document.querySelector('[data-row="pins:Needs you"] .space-section-count')?.textContent).toBe("2");
  cleanup();
  await render();
  expect(pinnedRows()).not.toContain("pinned:b");
});

it("a pin drag stays inside its section", () => {
  expect(pinMovePlan(snapshot, ["b", "c"], 1, 0)).toEqual({ tab: "c", pinIndex: 1 });
  expect(pinMovePlan(snapshot, ["a", "b"], 1, 0)).toBeNull();
  expect(document.querySelector('[data-row="pinned:b"]')?.getAttribute("data-pin-section")).toBe("pinned|Needs you");
});
