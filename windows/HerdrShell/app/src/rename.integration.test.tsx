// @vitest-environment happy-dom
// Integration: R273 (Alex, 2026-10-07: "renaming this thread didnt work? report to herdr ui").
// The PC side of the Mac's scripts/check_rename.py: the real sidebar, rename input, runAction and
// bridge; only native IPC is faked, by a stand-in herdr server that applies tab.rename to its tab
// list and answers session.snapshot. A spaces row draws the live label (model.ts), never the
// overlay's 30 s copy of it, so this passes on origin/main too; it guards that path.
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
const server = vi.hoisted(() => ({
  tabs: [{ tab_id: "w1:t1", workspace_id: "w1", number: 1, label: "recruiter" }],
  title: "claude",
  renames: [] as unknown[],
}));
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  invoke: vi.fn(async (cmd: string, args: { method?: string; params?: { tab_id: string; label: string } }) => {
    if (cmd !== "api_request") return new Promise(() => {});
    if (args.method === "tab.rename") {
      server.renames.push(args.params);
      const tab = server.tabs.find(t => t.tab_id === args.params!.tab_id)!;
      tab.label = args.params!.label;
      return { tab };
    }
    if (args.method === "session.snapshot") return { snapshot: snap() };
    throw new Error(`unexpected ${args.method}`);
  }),
}));
const snap = () => ({ workspaces: [{ workspace_id: "w1", number: 1, label: "recruiting" }], tabs: server.tabs.map(t => ({ ...t })),
  panes: [{ pane_id: "w1:p1", tab_id: "w1:t1", workspace_id: "w1", terminal_id: "term", terminal_title_stripped: server.title }] });
import Sidebar from "./Sidebar";
import { runAction } from "./actions";
import { bridge } from "./bridge";
import { buildSidebar, type Snapshot } from "./model";
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => void stored.set(k, v), clear: () => stored.clear() } });
let refresh: () => Promise<void> = async () => {};

function Shell() {
  // App's side of the wiring: the snapshot it re-reads on tab.renamed, rename state and the action.
  const [snapshot, setSnapshot] = useState<Snapshot>(snap() as Snapshot);
  const [renaming, setRenaming] = useState<string | null>(null);
  refresh = async () => { const r = await bridge.api("studio", "session.snapshot", {}) as { snapshot: Snapshot }; setSnapshot(r.snapshot); };
  const noop = () => {};
  const commitRename = async (id: string, label: string) => {
    await runAction("rename_tab", { machine: "studio", snapshot, rows: buildSidebar(snapshot), selected: id, focused: null, api: bridge.api,
      select: noop, focus: noop, created: noop, rename: setRenaming, switcher: noop, toggleSidebar: noop, error: e => { throw e; }, label, tabId: id });
    setRenaming(null);
    await refresh();
  };
  return <Sidebar snapshot={snapshot} rows={buildSidebar(snapshot)} selected={null} revealed={{ last: null, pending: null }} machine={{ name: "studio", state: "up" }}
    notice={null} select={noop} pin={noop} movePin={noop} renaming={renaming} startRename={setRenaming} cancelRename={() => setRenaming(null)} commitRename={commitRename} />;
}

const roots: Root[] = [];
function mount() {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); roots.push(root);
  act(() => root.render(<Shell />));
  const space = host.querySelector<HTMLButtonElement>(".space-row")!;
  if (space.getAttribute("aria-expanded") !== "true") act(() => space.click());
  return host;
}
// Tab rows only: the space row above them ("recruiting") shares the class.
const labels = (host: HTMLElement) => [...host.querySelectorAll(".select-tab .label")].map(e => e.textContent ?? "").filter(text => text !== "recruiting");
afterEach(() => { for (const r of roots.splice(0)) act(() => r.unmount()); document.body.innerHTML = ""; stored.clear(); });

it("a tab renamed in the sidebar shows its new name, through an agent title change and a reload", async () => {
  const host = mount();
  expect(labels(host)).toEqual(["recruiter"]);
  act(() => [...host.querySelectorAll(".select-tab")].find(b => b.textContent?.includes("recruiter"))!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })));
  const input = host.querySelector<HTMLInputElement>("input.rename-input")!;
  act(() => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "chauhan recap"); input.dispatchEvent(new Event("input", { bubbles: true })); });
  await act(async () => { input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); });
  await act(async () => { await new Promise(r => setTimeout(r, 0)); });
  expect(server.renames).toEqual([{ tab_id: "w1:t1", label: "chauhan recap" }]);
  expect(labels(host)).toEqual(["chauhan recap"]);
  // The agent sets a new terminal title; the snapshot that follows keeps the renamed label.
  server.title = "claude: another title";
  await act(async () => { await refresh(); });
  expect(labels(host)).toEqual(["chauhan recap"]);
  // A reload: a fresh sidebar reads the server again.
  for (const r of roots.splice(0)) act(() => r.unmount());
  document.body.innerHTML = "";
  expect(labels(mount())).toEqual(["chauhan recap"]);
});
