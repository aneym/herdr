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
const snapshot: Snapshot = { workspaces: [{ workspace_id: "space", number: 1, label: "Factory" }], tabs: [
  { tab_id: "lead", workspace_id: "space", number: 1, label: "Lead", pin_index: 0 },
  { tab_id: "agent", workspace_id: "space", number: 2, label: "Agent", role: "agent" },
  { tab_id: "ordinary", workspace_id: "space", number: 3, label: "Ordinary" },
] };
const host = document.createElement("div");
let root: ReturnType<typeof createRoot> | undefined;
afterEach(() => { if (root) act(() => root!.unmount()); root = undefined; host.remove(); storage.clear(); });
// Alex never asked for the metrics line: neither pulse source may add anything to any row.
it.each([false, true].flatMap(drifting => (["spaces", "areas"] as const).map(mode => ({ drifting, mode }))))("tab_pulse stays out of $mode rows (drifting=$drifting)", ({ drifting, mode }) => {
  const tabs = Object.fromEntries(snapshot.tabs!.map(tab => [tab.tab_id, { pulse: { line: "reply 18s · first act 9s · 140k · inline 0/5", drifting } }]));
  const withPulse: Snapshot = { ...snapshot, overlay: { tabs } };
  const catalog = new LaneSnapshot(); catalog.pulses = parsePulses(withPulse.overlay);
  storage.set("herdr-shell.areas.mode", JSON.stringify(mode));
  const select = vi.fn(), noop = () => {};
  document.body.append(host); root = createRoot(host);
  act(() => root!.render(<Sidebar snapshot={withPulse} catalog={catalog} machines={[]} chooseMachine={noop} rows={buildSidebar(withPulse)} selected="lead" revealed={{ last: "lead", pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={select} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
  expect(host.textContent).not.toContain("reply 18s");
  const ids = mode === "spaces" ? ["pinned:lead", "agent:agent", "tab:ordinary"] : ["tab:lead", "tab:agent", "tab:ordinary"];
  for (const [index, id] of ids.entries()) {
    const row = host.querySelector(`[data-row="${id}"]`);
    expect(row).not.toBeNull();
    expect(row?.textContent).toContain(["Lead", "Agent", "Ordinary"][index]);
    expect(row?.querySelector(".tab-pulse")).toBeNull();
  }
  expect(host.querySelector(`[data-row="${ids[0]}"]`)?.classList.contains("selected")).toBe(true);
  const ordinary = host.querySelector<HTMLButtonElement>(`[data-row="${ids[2]}"] .select-tab`)!;
  act(() => ordinary.click());
  expect(select).toHaveBeenCalledWith("ordinary");
});

// Golden decoder inputs cover string/object documents, malformed entries and strict drift booleans.
it.each([
  [{ tabs: { lead: { pulse: { line: "reply 18s", drifting: true } }, quiet: { pulse: { line: "", drifting: "true" } }, missing: {}, invalid: { pulse: { line: 18 } } } }, { lead: { line: "reply 18s", drifting: true }, quiet: { line: "", drifting: false } }],
  ['{"tabs":{"agent":{"pulse":{"line":"first act 9s"}}}}', { agent: { line: "first act 9s", drifting: false } }],
  ["{half-written", {}],
  [null, {}],
])("pulse decoder retains its data contract (%j)", (input, expected) => {
  expect(parsePulses(input)).toEqual(expected);
  expect(new LaneSnapshot().pulses).toEqual({});
});
