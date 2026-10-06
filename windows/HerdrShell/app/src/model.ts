import { pendingOrder } from "./pinDrag";
import type { PendingOrders } from "./pinDrag";
export interface Workspace { workspace_id: string; number: number; label?: string; focused?: boolean; active_tab_id?: string; work_status?: string; agent_status?: string; tokens?: { pinned?: string; hidden?: string } }
export interface Tab { tab_id: string; workspace_id: string; number: number; label?: string; focused?: boolean; pane_count?: number; work_status?: string; agent_status?: string; pin_index?: number; role?: string }
export interface Pane { pane_id: string; terminal_id: string; workspace_id: string; tab_id: string; focused?: boolean; agent?: string; agent_status?: string; title?: string; terminal_title_stripped?: string; cwd?: string }
export interface Rect { x: number; y: number; width: number; height: number }
export interface Layout { tab_id: string; area: Rect; panes: { pane_id: string; rect: Rect }[]; zoomed?: boolean; focused_pane_id?: string }
export interface Snapshot { workspaces?: Workspace[]; tabs?: Tab[]; panes?: Pane[]; agents?: { terminal_id: string; pane_id: string; tab_id: string; workspace_id: string; agent: string; agent_status: string; work_status?: string }[]; layouts?: Layout[] }
export interface SidebarRow { kind: "agent" | "pinned" | "space" | "tab"; id: string; label: string; status: string; hotkey: number | null; section: string; spaceLabel?: string; spaceId?: string; hidden?: boolean; pinned?: boolean }
export const tabStatus = (snapshot: Snapshot, tab: Tab): string => tab.work_status ?? snapshot.agents?.find(agent => agent.tab_id === tab.tab_id)?.agent_status ?? tab.agent_status ?? "unknown";
export const statusRank = (status: string) => ({ blocked: 3, working: 2, done: 1 }[status] ?? 0);
export function buildSidebar(snapshot: Snapshot, pending: PendingOrders = {}, now = Date.now()): SidebarRow[] {
  const tabs = snapshot.tabs ?? [];
  const spaces = [...snapshot.workspaces ?? []].sort((a, b) => Number(b.tokens?.pinned === "true") - Number(a.tokens?.pinned === "true") || a.number - b.number);
  const rows: SidebarRow[] = [];
  const row = (tab: Tab, kind: "agent" | "pinned" | "tab", section: string): SidebarRow => ({ kind, section, id: tab.tab_id, label: tab.label || snapshot.panes?.find(p => p.tab_id === tab.tab_id)?.terminal_title_stripped || `tab ${tab.number}`, status: tabStatus(snapshot, tab), hotkey: null, pinned: tab.pin_index != null, spaceId: tab.workspace_id, spaceLabel: spaces.find(s => s.workspace_id === tab.workspace_id)?.label });
  // As the Mac's SpacesTree.pinTabs: agents and plain pins are separate blocks, each in pin order.
  const byPin = (a: Tab, b: Tab) => (a.pin_index ?? 0) - (b.pin_index ?? 0);
  const pinned = (list: Tab[], section: "agent" | "pinned") => {
    const ids = pendingOrder(list.sort(byPin).map(t => t.tab_id), pending[section], now);
    return ids.map(id => list.find(t => t.tab_id === id)!);
  };
  pinned(tabs.filter(t => t.role === "agent"), "agent").forEach(t => rows.push(row(t, "agent", "AGENTS")));
  pinned(tabs.filter(t => t.role !== "agent" && t.pin_index != null), "pinned").forEach(t => rows.push(row(t, "pinned", "PINNED")));
  for (const space of spaces.filter(s => s.tokens?.hidden !== "true").concat(spaces.filter(s => s.tokens?.hidden === "true"))) {
    const children = tabs.filter(t => t.workspace_id === space.workspace_id);
    const status = children.map(t => tabStatus(snapshot, t)).sort((a, b) => statusRank(b) - statusRank(a))[0] ?? "idle";
    rows.push({ kind: "space", id: space.workspace_id, label: space.label || `space ${space.number}`, status, hotkey: null, section: "spaces", hidden: space.tokens?.hidden === "true" });
    children.filter(t => t.role !== "agent").sort((a, b) => a.number - b.number).forEach(t => rows.push({ ...row(t, "tab", space.workspace_id), hidden: space.tokens?.hidden === "true" }));
  }
  const numbered = new Map<string, number>();
  for (const item of rows) {
    if (item.kind === "space") continue;
    if (!numbered.has(item.id) && numbered.size < 9) numbered.set(item.id, numbered.size + 1);
    item.hotkey = numbered.get(item.id) ?? null;
  }
  return rows;
}
export function tabOrder(rows: SidebarRow[]): string[] { return [...new Set(rows.filter(r => r.kind !== "space").map(r => r.id))]; }
export function scaleRect(rect: Rect, area: Rect, width: number, height: number): Rect {
  return { x: (rect.x - area.x) / Math.max(1, area.width) * width, y: (rect.y - area.y) / Math.max(1, area.height) * height, width: rect.width / Math.max(1, area.width) * width, height: rect.height / Math.max(1, area.height) * height };
}
