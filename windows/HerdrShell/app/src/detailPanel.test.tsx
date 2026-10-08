// @vitest-environment happy-dom
// Integration: real sidebar/catalog/panel, native file boundary only. Guards row clicks
// stealing pane focus/selection and Escape leaking into the terminal; no prior panel coverage.
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import { useLaneFiles } from "./laneFiles";
import Sidebar, { useSelectionReveal } from "./Sidebar";
import DetailPanel, { useDetailPanel } from "./DetailPanel";
const read = vi.fn(async (_machine: string, path: string) => ({ data_b64: btoa(path.endsWith("lanes.json") ? JSON.stringify({ lanes: [{ tab: "lane", name: "Lane catalog", kind: "project" }] }) : "{}") }));
vi.mock("./bridge", () => ({ bridge: { fileRead: (...args: [string, string]) => read(...args), fileList: async () => [], updateStatus: () => new Promise(() => {}) }, fromBase64: (s: string) => Uint8Array.from(atob(s), c => c.charCodeAt(0)) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => stored.set(k, v) } });
const snapshot = {
  workspaces: [{ workspace_id: "w", number: 1, active_tab_id: "current", label: "Space" }],
  tabs: [{ tab_id: "orch", workspace_id: "w", number: 1, label: "Orchestrator" }, { tab_id: "lane", workspace_id: "w", number: 2, label: "Lane" }, { tab_id: "wf", workspace_id: "w", number: 3, label: "wf verify" }, { tab_id: "current", workspace_id: "w", number: 4, label: "Current" }],
  panes: [{ pane_id: "owner", terminal_id: "owner", workspace_id: "w", tab_id: "lane" }],
  agents: [
    { tab_id: "orch", agent_status: "working", agent: "claude", tokens: { kind: "orchestrator", routed: "Review assigned@router" } },
    { tab_id: "lane", agent_status: "working", agent: "codex", tokens: { kind: "lane", inbox_items: "Check the diff@lead|Address notes", host: "Book" } },
    { tab_id: "wf", agent_status: "blocked", agent: "codex", tokens: { phase: "review", host: "Studio" }, ownership: { current: { pane_id: "owner" } } },
  ],
} as unknown as Snapshot;
const paneEscape = vi.fn();
function Shell() {
  const [selected, select] = useState("current");
  const catalog = useLaneFiles("book", true);
  const panel = useDetailPanel();
  const noop = () => {};
  return <><output>{selected}</output><Sidebar snapshot={snapshot} catalog={catalog} machines={[]} chooseMachine={noop} rows={buildSidebar(snapshot)} selected={selected} revealed={useSelectionReveal(selected)} machine={{ name: "book", state: "up" }} notice={null} select={select} openDetail={panel.toggle} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />{panel.rowId && <DetailPanel snapshot={snapshot} rowId={panel.rowId} openFull={id => { panel.close(); select(id); }} />}<textarea aria-label="Pane" onKeyDown={event => { if (event.key === "Escape") paneEscape(); }} /></>;
}
let cleanup: (() => void) | undefined;
afterEach(() => { cleanup?.(); stored.clear(); paneEscape.mockClear(); read.mockClear(); });
it("lane detail preserves the current pane and owns Escape only while open", async () => {
  stored.set("herdr-shell.areas.mode", '"spaces"');
  const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
  cleanup = () => { act(() => root.unmount()); host.remove(); };
  await act(async () => { root.render(<Shell />); });
  const pane = host.querySelector("textarea")!; pane.focus(); const before = document.activeElement;
  const row = [...host.querySelectorAll<HTMLButtonElement>(".select-tab")].find(b => b.textContent === "Lane")!;
  expect(row).toBeTruthy();
  await act(async () => { row.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true })); row.click(); });
  const panel = host.querySelector('[aria-label="Lane details"]');
  expect(panel?.textContent).toContain("Check the diff"); expect(panel?.textContent).toContain("lead");
  expect(panel?.textContent).toContain("Address notes"); expect(panel?.textContent).toContain("wf verify wants you");
  expect(panel?.textContent).toContain("wf verify"); expect(panel?.textContent).toContain("review"); expect(panel?.textContent).toContain("Studio");
  expect(host.querySelector("output")?.textContent).toBe("current"); expect(document.activeElement).toBe(before);
  expect(read.mock.calls.every(([machine]) => machine === "book")).toBe(true);
  act(() => pane.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })));
  expect(host.querySelector('[aria-label="Lane details"]')).toBeNull(); expect(paneEscape).not.toHaveBeenCalled();
  act(() => pane.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true })));
  expect(paneEscape).toHaveBeenCalledOnce(); expect(document.activeElement).toBe(before);
});
