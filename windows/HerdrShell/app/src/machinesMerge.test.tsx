// @vitest-environment happy-dom
// Mac parity (Machines.swift, MachineMerge.swift): other machines' chats join the one tree, each with
// a quiet machine badge, instead of pc/studio chips at the top. An unreachable machine keeps its last
// rows, with the badge dimmed and saying so; clicking a remote chat opens it on its own machine.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import { remoteMachines } from "./machines";
vi.mock("./bridge", () => ({ bridge: { api: async () => ({}), updateStatus: () => new Promise(() => {}), fileList: async () => [], fileRead: async () => ({ data_b64: "" }) }, fromBase64: () => new Uint8Array() }));
const { default: Sidebar, useSelectionReveal } = await import("./Sidebar");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => void stored.set(k, v), clear: () => stored.clear() } });

const local: Snapshot = {
  workspaces: [{ workspace_id: "w1", number: 1, label: "rails" }],
  tabs: [{ tab_id: "w1:t1", workspace_id: "w1", number: 1, label: "orchestrator" }],
};
const pc: Snapshot = {
  workspaces: [{ workspace_id: "w1", number: 1, label: " Rails " }, { workspace_id: "w2", number: 2, label: "games" }],
  tabs: [
    { tab_id: "w1:t1", workspace_id: "w1", number: 1, label: "pc build" },
    { tab_id: "w1:t2", workspace_id: "w1", number: 2, label: "pc shell" },
    { tab_id: "w2:t1", workspace_id: "w2", number: 1, label: "league" },
    { tab_id: "w2:t2", workspace_id: "w2", number: 2, label: "Coach", role: "agent", pin_index: 0 },
  ],
  agents: [
    { terminal_id: "a", pane_id: "p1", tab_id: "w1:t1", workspace_id: "w1", agent: "claude", agent_status: "working" },
    { terminal_id: "b", pane_id: "p2", tab_id: "w2:t1", workspace_id: "w2", agent: "codex", agent_status: "idle" },
  ],
};

describe("machines merged into the sidebar tree", () => {
  let host: HTMLDivElement, root: Root;
  const selectRemote = vi.fn();
  function Shell({ pcState }: { pcState: "up" | "down" }) {
    const revealed = useSelectionReveal(null);
    const noop = () => {};
    const remotes = remoteMachines([{ name: "studio", state: "up" }, { name: "pc", state: pcState }], { studio: local, pc }, "studio");
    return <Sidebar snapshot={local} remotes={remotes} selectRemote={selectRemote} rows={buildSidebar(local)} selected={null} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />;
  }
  beforeEach(() => { stored.clear(); stored.set("herdr-shell.areas.mode", JSON.stringify("spaces")); stored.set("herdr-space-expanded", JSON.stringify({ w1: true, "pc/w2": true })); selectRemote.mockClear(); host = document.createElement("div"); document.body.append(host); root = createRoot(host); });
  afterEach(() => { act(() => root.unmount()); host.remove(); });

  it("draws no machine chips and merges remote chats under their space with a badge", () => {
    act(() => root.render(<Shell pcState="up" />));
    expect(host.querySelector('[aria-label="Machines"]')).toBeNull();
    // A remote agent joins AGENTS; a remote chat joins the local space with the same label.
    expect(host.querySelector('[data-row="agent:pc/w2:t2"] .machine-badge')?.textContent).toBe("pc");
    const rails = [...host.querySelectorAll("[data-row]")].map(e => e.getAttribute("data-row"));
    expect(rails.indexOf("tab:pc/w1:t1")).toBe(rails.indexOf("tab:w1:t1") + 1);
    // Only chats travel: a remote tab with no agent and no pin adds nothing.
    expect(host.querySelector('[data-row="tab:pc/w1:t2"]')).toBeNull();
    // A remote space with no local twin is its own space after the local ones.
    expect(host.querySelector('[data-space="pc/w2"]')?.textContent).toContain("games");
    expect(host.querySelector('[data-row="tab:pc/w2:t1"] .machine-badge')?.getAttribute("title")).toBe("Running on pc");
    act(() => host.querySelector<HTMLButtonElement>('[data-row="tab:pc/w1:t1"] .select-tab')!.click());
    expect(selectRemote).toHaveBeenCalledWith("pc", "w1:t1");
  });

  it("keeps an unreachable machine's rows visible with a dimmed badge that says so", () => {
    act(() => root.render(<Shell pcState="down" />));
    const badge = host.querySelector('[data-row="tab:pc/w1:t1"] .machine-badge')!;
    expect(badge.getAttribute("title")).toBe("pc: unreachable");
    expect(badge.classList.contains("is-unhealthy")).toBe(true);
    expect(host.querySelector('[data-row="tab:w1:t1"] .machine-badge')).toBeNull();
  });
});
