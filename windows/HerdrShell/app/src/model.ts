import { pendingOrder } from "./pinDrag";
import type { PendingOrders } from "./pinDrag";
import { cardsByTab, faceFor } from "./faces";
import type { AgentCard, Face } from "./faces";
export interface Workspace { workspace_id: string; number: number; sort_rank?: number; parked?: boolean; label?: string; focused?: boolean; active_tab_id?: string; work_status?: string; agent_status?: string; tokens?: { pinned?: string; hidden?: string } }
export interface Tab { tab_id: string; workspace_id: string; number: number; label?: string; focused?: boolean; pane_count?: number; work_status?: string; agent_status?: string; pin_index?: number; role?: string; sort_rank?: number; hidden?: boolean; home_location?: "cloud" | "local" | "unsynced" }
export interface Pane { restore_error?: string; pane_id: string; terminal_id: string; workspace_id: string; tab_id: string; focused?: boolean; agent?: string; agent_status?: string; title?: string; terminal_title_stripped?: string; cwd?: string; tokens?: Record<string, string> }
export interface Rect { x: number; y: number; width: number; height: number }
export interface Layout { tab_id: string; area: Rect; panes: { pane_id: string; rect: Rect }[]; splits?: { id: string; direction: string; ratio: number; rect: Rect }[]; zoomed?: boolean; focused_pane_id?: string }
export interface Snapshot { workspaces?: Workspace[]; tabs?: Tab[]; panes?: Pane[]; agents?: { terminal_id: string; pane_id: string; tab_id: string; workspace_id: string; agent: string; agent_status: string; work_status?: string }[]; layouts?: Layout[] }
export interface SidebarRow { kind: "agent" | "pinned" | "space" | "tab"; id: string; label: string; status: string; hotkey: number | null; section: string; spaceLabel?: string; spaceId?: string; hidden?: boolean; parked?: boolean; pinned?: boolean; face?: Face; request?: string; home?: Tab["home_location"] }
export const tabStatus = (snapshot: Snapshot, tab: Tab): string => tab.work_status ?? snapshot.agents?.find(agent => agent.tab_id === tab.tab_id)?.agent_status ?? tab.agent_status ?? "unknown";
export const statusRank = (status: string) => ({ blocked: 3, working: 2, done: 1 }[status] ?? 0);
export function buildSidebar(snapshot: Snapshot, pending: PendingOrders = {}, now = Date.now(), cards: Record<string, AgentCard> = {}): SidebarRow[] {
  const tabs = snapshot.tabs ?? [];
  // As the Mac's SpacesTree: the pin partition first, then the server's priority rank within each.
  const byRank = (a: Workspace | Tab, b: Workspace | Tab) => (a.sort_rank ?? 0) - (b.sort_rank ?? 0) || a.number - b.number;
  const spaces = [...snapshot.workspaces ?? []].sort((a, b) => Number(b.tokens?.pinned === "true") - Number(a.tokens?.pinned === "true") || byRank(a, b));
  const rows: SidebarRow[] = [];
  const row = (tab: Tab, kind: "agent" | "pinned" | "tab", section: string): SidebarRow => ({ kind, section, id: tab.tab_id, label: tab.label || snapshot.panes?.find(p => p.tab_id === tab.tab_id)?.terminal_title_stripped || `tab ${tab.number}`, status: tabStatus(snapshot, tab), hotkey: null, pinned: tab.pin_index != null, spaceId: tab.workspace_id, spaceLabel: spaces.find(s => s.workspace_id === tab.workspace_id)?.label });
  // As the Mac's SpacesTree.pinTabs: agents and plain pins are separate blocks, each in pin order.
  const byPin = (a: Tab, b: Tab) => (a.pin_index ?? 0) - (b.pin_index ?? 0);
  const pinned = (list: Tab[], section: "agent" | "pinned") => {
    const ids = pendingOrder(list.sort(byPin).map(t => t.tab_id), pending[section], now);
    return ids.map(id => list.find(t => t.tab_id === id)!);
  };
  // An agent row carries its face and open request; the request folds into the face's dot.
  const byTab = cardsByTab(snapshot, cards);
  const agents = tabs.filter(t => t.role === "agent");
  // Pending drags name only visible rows; hidden pins retain their server slot.
  pinned(agents.filter(t => !t.hidden), "agent").concat(agents.filter(t => t.hidden).sort(byPin)).forEach(t => {
    const item = row(t, "agent", "AGENTS"), card = byTab[t.tab_id];
    const request = snapshot.panes?.filter(p => p.tab_id === t.tab_id).map(p => p.tokens?.request).find(r => r != null);
    rows.push({ ...item, hidden: !!t.hidden, home: t.home_location, face: faceFor(card?.name ?? item.label, card?.avatar), ...(request != null ? { request } : {}) });
  });
  pinned(tabs.filter(t => t.role !== "agent" && t.pin_index != null), "pinned").forEach(t => rows.push(row(t, "pinned", "PINNED")));
  for (const space of spaces.filter(s => s.tokens?.hidden !== "true").concat(spaces.filter(s => s.tokens?.hidden === "true"))) {
    const children = tabs.filter(t => t.workspace_id === space.workspace_id);
    const status = children.map(t => tabStatus(snapshot, t)).sort((a, b) => statusRank(b) - statusRank(a))[0] ?? "idle";
    rows.push({ kind: "space", id: space.workspace_id, label: space.label || `space ${space.number}`, status, hotkey: null, section: "spaces", hidden: space.tokens?.hidden === "true", ...(space.parked ? { parked: true } : {}) });
    children.filter(t => t.role !== "agent").sort(byRank).forEach(t => rows.push({ ...row(t, "tab", space.workspace_id), hidden: space.tokens?.hidden === "true" }));
  }
  const numbered = new Map<string, number>();
  for (const item of rows) {
    if (item.kind === "space" || item.kind === "agent" && item.hidden) continue;
    if (!numbered.has(item.id) && numbered.size < 9) numbered.set(item.id, numbered.size + 1);
    item.hotkey = numbered.get(item.id) ?? null;
  }
  return rows;
}
/** A space row's fold key: a parked space keeps its own, so unparking does not inherit its fold. */
export const foldKey = (space: SidebarRow) => space.parked ? `parked:${space.id}` : space.id;
/** Whether a space row shows its tabs: the user's fold wins; a parked space starts folded, any other
 *  opens while it holds the selected tab or live work. */
export function spaceOpen(space: SidebarRow, rows: SidebarRow[], selected: string | null, expanded: Record<string, boolean>): boolean {
  const stored = expanded[foldKey(space)];
  if (stored !== undefined) return stored;
  if (space.parked) return false;
  return rows.some(r => r.id === selected && r.spaceId === space.id) || space.status === "working" || space.status === "blocked" || space.status === "done";
}
/** Bulk folds share the manual space keys; tab/section state is not rewritten. */
export function foldAllSpaces(rows: SidebarRow[], expanded: Record<string, boolean>, open: boolean): Record<string, boolean> {
  const next = { ...expanded };
  for (const space of rows.filter(row => row.kind === "space")) next[foldKey(space)] = open;
  return next;
}
/** Reveal on select, as the Mac's SpacesChrome.reveal: the fold key to open so the selected tab's
 *  space shows its tabs, or null when it already does. Hidden spaces stay in the Hidden group. */
export function revealFold(rows: SidebarRow[], selected: string | null, expanded: Record<string, boolean>): string | null {
  if (rows.some(r => r.kind === "pinned" && r.id === selected)) return null;
  const tab = rows.find(r => r.kind === "tab" && r.id === selected);
  const space = tab && rows.find(r => r.kind === "space" && r.id === tab.spaceId);
  return space && !spaceOpen(space, rows, selected, expanded) ? foldKey(space) : null;
}
/** A selection change not yet revealed. App notes every change, sidebar shown or not; the sidebar
 *  takes it once, so remounting it (or StrictMode's second effect run) reopens nothing the user
 *  folded since, and a change made while it was hidden still reveals when it shows. */
export interface RevealMemo { last: string | null | undefined; pending: string | null }
export function noteSelection(memo: RevealMemo, selected: string | null): void {
  if (memo.last !== selected) { memo.last = selected; memo.pending = selected; }
}
export function revealOnSelect(memo: RevealMemo, rows: SidebarRow[], selected: string | null, expanded: Record<string, boolean>): string | null {
  if (memo.pending === null || memo.pending !== selected) return null;
  memo.pending = null;
  return revealFold(rows, selected, expanded);
}
/** Pins on the machine in a `tab.list` answer. */
export function pinCount(answer: unknown): number {
  const tabs = (answer as { tabs?: { pin_index?: number | null }[] } | null)?.tabs;
  return Array.isArray(tabs) ? tabs.filter(t => t?.pin_index != null).length : 0;
}
export function tabOrder(rows: SidebarRow[]): string[] { return [...new Set(rows.filter(r => r.kind !== "space").map(r => r.id))]; }
export function scaleRect(rect: Rect, area: Rect, width: number, height: number): Rect {
  return { x: (rect.x - area.x) / Math.max(1, area.width) * width, y: (rect.y - area.y) / Math.max(1, area.height) * height, width: rect.width / Math.max(1, area.width) * width, height: rect.height / Math.max(1, area.height) * height };
}

// A removed selection follows its previous drawn pin block; unpinned selection
// retains the old index-based fallback, including the server's initial focus.
export function selectionAfterClose(previous: SidebarRow[], rows: SidebarRow[], selected: string | null, initial?: string): string | null {
  const order = tabOrder(rows);
  if (selected) {
    const pin = previous.find(r => r.id === selected && (r.kind === "agent" || r.kind === "pinned"));
    if (pin) {
      const own = previous.filter(r => r.kind === pin.kind).map(r => r.id);
      const other = previous.filter(r => r.kind === (pin.kind === "agent" ? "pinned" : "agent")).map(r => r.id);
      const index = own.indexOf(selected);
      const next = [...own.slice(index + 1), ...own.slice(0, index).reverse(), ...other].find(id => order.includes(id));
      if (next) return next;
    }
  }
  const index = selected ? tabOrder(previous).indexOf(selected) : -1;
  return index >= 0 ? order[Math.min(index, order.length - 1)] ?? null : initial ?? order[0] ?? null;
}
