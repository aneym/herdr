// @vitest-environment happy-dom
// Alex, 2026-10-09: "update windows herdr to get parity, still seeing the status line on there".
// The Mac sidebar draws rows as name, dot and icons, with no connection or status line under them;
// the machine row's dot already says whether a machine is up. A connected machine whose agent
// carries status, work and workflow data must draw no status text anywhere in the sidebar.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
vi.mock("./bridge", () => ({ bridge: { api: async () => ({}), updateStatus: () => new Promise(() => {}), fileList: async () => [], fileRead: async () => ({ data_b64: "" }) }, fromBase64: () => new Uint8Array() }));
const { default: Sidebar, useSelectionReveal } = await import("./Sidebar");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// Node 26's own localStorage global is undefined without --localstorage-file and hides happy-dom's.
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => void stored.set(k, v), clear: () => stored.clear() } });

const snapshot: Snapshot = {
  workspaces: [{ workspace_id: "w1", number: 1, label: "rails" }],
  tabs: [{ tab_id: "A", workspace_id: "w1", number: 1, label: "herdr ui", role: "agent", pin_index: 0, agent_status: "working", work_status: "building", home_location: "cloud" }],
  panes: [{ pane_id: "p1", terminal_id: "t1", workspace_id: "w1", tab_id: "A", agent: "claude", agent_status: "working", title: "Running tests 3/12", tokens: { request: "approve merge", metrics: "42k tok · $0.31" } }],
  agents: [{ terminal_id: "t1", pane_id: "p1", tab_id: "A", workspace_id: "w1", agent: "claude", agent_status: "working", work_status: "building" }],
  overlay: { tabs: { A: { pulse: { line: "workflow 3/5: running checks" } } } },
};

describe("sidebar status line", () => {
  let host: HTMLDivElement, root: Root;
  function Shell({ notice }: { notice: string | null }) {
    const revealed = useSelectionReveal(null);
    const noop = () => {};
    return <Sidebar snapshot={snapshot} machines={[{ name: "studio", state: "up" }]} chooseMachine={noop} rows={buildSidebar(snapshot)} selected={null} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={notice} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />;
  }
  beforeEach(() => { stored.clear(); host = document.createElement("div"); document.body.append(host); root = createRoot(host); });
  afterEach(() => { act(() => root.unmount()); host.remove(); });

  it.each(["spaces", "areas"])("draws no status or subline text for a connected machine in %s mode", mode => {
    stored.set("herdr-shell.areas.mode", JSON.stringify(mode));
    act(() => root.render(<Shell notice={null} />));
    const text = host.textContent ?? "";
    for (const word of ["connected", "connecting", "offline", "Running tests", "42k tok", "workflow 3/5", "building", "approve merge"]) expect(text, word).not.toContain(word);
    expect(host.querySelector('[role="status"]')!.textContent).toBe("");
    if (mode === "spaces") expect(host.querySelector('[data-row="agent:A"]')!.textContent).toMatch(/^\p{Lu}?herdr ui☁︎⚲$/u);
  });

  it("still shows a transient error notice", () => {
    act(() => root.render(<Shell notice="rename failed" />));
    expect(host.querySelector('[role="status"]')!.textContent).toBe("rename failed");
  });
});
