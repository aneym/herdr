// @vitest-environment happy-dom
// Integration: real sidebar/catalog/panel, native file boundary only. Guards row clicks
// stealing pane focus/selection and Escape leaking into the terminal; no prior panel coverage.
import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import { useLaneFiles } from "./laneFiles";
import Switcher from "./Switcher";
import Sidebar, { useSelectionReveal } from "./Sidebar";
import { installControl } from "./control";
import DetailPanel, { useDetailPanel } from "./DetailPanel";
const read = vi.fn(async (_machine: string, path: string) => ({ data_b64: btoa(path.endsWith("lanes.json") ? JSON.stringify({ lanes: [{ tab: "lane", name: "Lane catalog", kind: "project" }] }) : "{}") }));
vi.mock("./bridge", () => ({ bridge: { fileRead: (...args: [string, string]) => read(...args), fileList: async () => [], controlEvent: async (cmd: string, fn: (payload: unknown) => void) => { controlEvents.set(cmd, fn); return () => { controlEvents.delete(cmd); }; }, controlResult: (cmd: string, result: unknown) => controlResult(cmd, result), updateStatus: () => new Promise(() => {}) }, fromBase64: (s: string) => Uint8Array.from(atob(s), c => c.charCodeAt(0)) }));
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
const controlEvents = new Map<string, (payload: unknown) => void>();
const controlResult = vi.fn(async (_cmd: string, _result: unknown) => {});
const paneEscape = vi.fn();
function Shell({ data = snapshot }: { data?: Snapshot }) {
  const [selected, select] = useState("current");
  const [switcher, setSwitcher] = useState(false);
  const catalog = useLaneFiles("book", true);
  const panel = useDetailPanel(switcher, data);
  useEffect(() => installControl(() => ({ machine: { name: "book", state: "up" }, machines: [], chooseMachine: () => ({ name: "book", state: "up" }), selected, docs: { open: false, items: [], active: null }, rows: buildSidebar(data), panes: [], focused: undefined, open: select, openDetail: id => panel.open(id ?? null), action: async () => {} })), [selected, data, panel.open]);
  const noop = () => {};
  return <><output>{selected}</output><button onClick={() => setSwitcher(true)}>Switch</button><Sidebar snapshot={data} catalog={catalog} machines={[]} chooseMachine={noop} rows={buildSidebar(data)} selected={selected} revealed={useSelectionReveal(selected)} machine={{ name: "book", state: "up" }} notice={null} select={select} openDetail={panel.toggle} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />{panel.rowId && <DetailPanel snapshot={data} rowId={panel.rowId} openFull={id => { panel.close(); select(id); }} />}<textarea className="xterm-helper-textarea" aria-label="Pane" onKeyDown={event => { if (event.key === "Escape") paneEscape(); }} />{switcher && <Switcher rows={buildSidebar(data)} selected={selected} machine="book" open={select} close={() => setSwitcher(false)} />}</>;
}
let cleanup: (() => void) | undefined;
afterEach(() => { cleanup?.(); stored.clear(); paneEscape.mockClear(); read.mockClear(); controlResult.mockClear(); });
async function mount() {
  stored.set("herdr-shell.areas.mode", '"spaces"');
  const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
  cleanup = () => { act(() => root.unmount()); host.remove(); };
  await act(async () => { root.render(<Shell />); });
  return { host, root, pane: host.querySelector("textarea")! };
}
const laneRow = (host: HTMLElement) => [...host.querySelectorAll<HTMLButtonElement>(".select-tab")].find(b => b.querySelector(".label")?.textContent === "Lane")!;
const panelIn = (host: HTMLElement) => host.querySelector('[aria-label="Lane details"]');
function escape(target: Element) { act(() => target.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }))); }
it("Spaces clicks select without opening details", async () => {
  const { host } = await mount(); act(() => laneRow(host).click());
  expect(host.querySelector("output")?.textContent).toBe("lane"); expect(panelIn(host)).toBeNull();
});
it("Switcher Escape takes precedence over an open detail panel", async () => {
  const { host } = await mount();
  act(() => laneRow(host).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })));
  act(() => [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find(b => b.textContent === "Show info")!.click());
  act(() => [...host.querySelectorAll("button")].find(b => b.textContent === "Switch")!.click());
  escape(host.querySelector('[aria-label="Filter tabs"]')!);
  expect(host.querySelector('[aria-label="Switch tab"]')).toBeNull(); expect(panelIn(host)).not.toBeNull();
});
it("Show info preserves selection and routes Escape only while open; row removal closes it", async () => {
  const { host, root, pane } = await mount();
  const show = () => {
    act(() => laneRow(host).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })));
    act(() => [...host.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find(b => b.textContent === "Show info")!.click());
  };
  pane.focus(); show();
  expect(document.activeElement).toBe(pane);
  expect(host.querySelector("output")?.textContent).toBe("current");
  expect(panelIn(host)?.textContent).toContain("Check the diff"); expect(panelIn(host)?.textContent).toContain("wf verify");
  escape(pane); expect(panelIn(host)).toBeNull(); expect(paneEscape).not.toHaveBeenCalled();
  escape(pane); expect(paneEscape).toHaveBeenCalledOnce(); expect(document.activeElement).toBe(pane);
  show(); act(() => root.render(<Shell data={{ ...snapshot, tabs: snapshot.tabs?.filter(t => t.tab_id !== "lane") }} />));
  expect(panelIn(host)).toBeNull();
});

it("control hooks expose a row menu without acting and open detail without selecting", async () => {
  const { host } = await mount();
  await act(async () => {
    controlEvents.get("row_menu")!({ row_id: laneRow(host).closest<HTMLElement>("[data-row]")!.dataset.row });
    await vi.waitFor(() => expect(controlResult).toHaveBeenCalledWith("row_menu", { ok: true }));
  });
  expect(host.querySelector('[role="menu"]')?.textContent).toContain("Show info");
  expect(host.querySelector("output")?.textContent).toBe("current");
  expect(panelIn(host)).toBeNull();
  await act(async () => {
    controlEvents.get("open_detail")!({ row_id: "lane" });
    await vi.waitFor(() => expect(controlResult).toHaveBeenCalledWith("open_detail", { ok: true }));
  });
  expect(panelIn(host)?.textContent).toContain("Check the diff");
  expect(host.querySelector("output")?.textContent).toBe("current");
});
