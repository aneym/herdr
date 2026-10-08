// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import { LaneSnapshot } from "./laneFiles";
vi.mock("./bridge", () => ({ bridge: { updateStatus: () => new Promise(() => {}) } }));
const { default: Sidebar } = await import("./Sidebar");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const storage = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) } });
const snapshot: Snapshot = { workspaces: [{ workspace_id: "space", number: 1, label: "Factory" }], tabs: [{ tab_id: "lead", workspace_id: "space", number: 1, label: "Lead", pin_index: 0 }] };
const host = document.createElement("div");
let root: ReturnType<typeof createRoot> | undefined;
afterEach(() => { if (root) act(() => root!.unmount()); root = undefined; host.remove(); storage.clear(); });
// Alex never asked for the metrics line: an overlay pulse must not add anything to the row.
it.each([false, true])("tab_pulse stays out of the sidebar row (drifting=%s)", drifting => {
  const withPulse: Snapshot = { ...snapshot, overlay: { tabs: { lead: { pulse: { line: "reply 18s · first act 9s · 140k · inline 0/5", drifting } } } } };
  const noop = () => {};
  document.body.append(host); root = createRoot(host);
  act(() => root!.render(<Sidebar snapshot={withPulse} catalog={new LaneSnapshot()} machines={[]} chooseMachine={noop} rows={buildSidebar(withPulse)} selected="lead" revealed={{ last: "lead", pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
  expect(host.textContent).not.toContain("reply 18s");
  expect(host.querySelector('[data-row="pinned:lead"] .label')?.textContent).toBe("Lead");
});
