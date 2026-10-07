// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import { LaneSnapshot, parsePulses } from "./laneFiles";
vi.mock("./bridge", () => ({ bridge: { updateStatus: () => new Promise(() => {}) } }));
const { default: Sidebar } = await import("./Sidebar");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const storage = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) } });
const snapshot: Snapshot = { workspaces: [{ workspace_id: "space", number: 1, label: "Factory" }], tabs: [{ tab_id: "lead", workspace_id: "space", number: 1, label: "Lead", pin_index: 0 }] };
const host = document.createElement("div");
let root: ReturnType<typeof createRoot> | undefined;
afterEach(() => { if (root) act(() => root!.unmount()); root = undefined; host.remove(); storage.clear(); });
// The real sidebar receives the same overlay document as the file poller: a user's
// click on the timing line must select the tab without introducing a status icon.
it.each([false, true])("tab_pulse displays a single plain row and selects its tab (drifting=%s)", drifting => {
  storage.set("herdr-shell.areas.mode", JSON.stringify("spaces"));
  const line = "reply 18s · first act 9s · 140k · inline 0/5 ".repeat(10);
  const catalog = new LaneSnapshot();
  catalog.pulses = parsePulses(JSON.stringify({ tabs: { lead: { pulse: { line, drifting } } } }));
  const select = vi.fn(), noop = () => {};
  document.body.append(host); root = createRoot(host);
  act(() => root!.render(<Sidebar snapshot={snapshot} catalog={catalog} machines={[]} chooseMachine={noop} rows={buildSidebar(snapshot)} selected="lead" revealed={{ last: "lead", pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={select} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
  const pulse = host.querySelector<HTMLButtonElement>(".tab-pulse")!;
  expect(pulse.textContent).toBe(line);
  expect(pulse.classList.contains("tab-pulse-bold")).toBe(drifting);
  expect(pulse.children.length).toBe(0);
  act(() => pulse.click()); expect(select).toHaveBeenCalledWith("lead");
});
it.each([undefined, "", " \t\n"])("tab_pulse absent or blank line displays no timing row (%s)", line => {
  const catalog = new LaneSnapshot();
  if (line !== undefined) catalog.pulses = { lead: { line, drifting: false } };
  const noop = () => {};
  document.body.append(host); root = createRoot(host);
  act(() => root!.render(<Sidebar snapshot={snapshot} catalog={catalog} machines={[]} chooseMachine={noop} rows={buildSidebar(snapshot)} selected="lead" revealed={{ last: "lead", pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
  expect(host.querySelector(".tab-pulse")).toBeNull();
  expect(host.querySelector(".has-pulse")).toBeNull();
  expect(host.querySelector('[data-row="pinned:lead"] .label')?.textContent).toBe("Lead");
});
