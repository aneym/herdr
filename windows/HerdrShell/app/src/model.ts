import type { PendingOrders } from "./pinDrag";
import type { Face } from "./faces";
import { compareHomeTags, homeSection, homeTooltip, representedTabIds } from "./home";
export interface Workspace { workspace_id: string; number: number; sort_rank?: number; parked?: boolean; label?: string; focused?: boolean; active_tab_id?: string; work_status?: string; agent_status?: string; tokens?: { pinned?: string; hidden?: string } }
export interface Tab { tab_id: string; workspace_id: string; number: number; label?: string; custom_label?: boolean; focused?: boolean; pane_count?: number; work_status?: string; agent_status?: string; pin_index?: number; role?: string; sort_rank?: number; hidden?: boolean; home_location?: "cloud" | "local" | "unsynced"; runs_on?: string; doc?: TabDoc; pin_section?: string; agent_card?: AgentCard }
export interface AgentCard { name: string; avatar_url?: string }
export interface TabDoc { url: string; title: string }
export interface AutomationSummary { id: string; name: string; cadence?: string; status?: string; result?: string; last_run?: string; next_run?: string; report?: string; report_url?: string }
export interface Pane { restore_error?: string; pane_id: string; terminal_id: string; workspace_id: string; tab_id: string; focused?: boolean; agent?: string; agent_status?: string; title?: string; terminal_title_stripped?: string; cwd?: string; foreground_cwd?: string; label?: string; tokens?: Record<string, string> }
/** Mac PaneCapName: the TUI border's order (agent title, manual label, agent kind), then the terminal's own title and its working folder, so a plain shell still names itself. */
export function paneName(pane: Pane, agent?: { agent?: string }): string {
  const folder = (pane.foreground_cwd ?? pane.cwd)?.split(/[\\/]/).filter(Boolean).pop();
  return [pane.title, pane.label, agent?.agent ?? pane.agent, pane.terminal_title_stripped, folder].map(text => text?.trim()).find(Boolean) ?? "Terminal";
}
export interface Rect { x: number; y: number; width: number; height: number }
export interface Layout { tab_id: string; area: Rect; panes: { pane_id: string; rect: Rect }[]; splits?: { id: string; direction: string; ratio: number; rect: Rect }[]; zoomed?: boolean; focused_pane_id?: string }
export interface TabPulse { line: string; drifting?: boolean }
export type HomeState = "needs_you" | "working" | "finished";
export interface HomeTag { entity: string; class: "owner" | "lane" | "utility"; state: HomeState; seats: number; stale: boolean; progress_at?: number | null; finished_at: number | null; sub: string; order: number; folded: boolean }
export interface OverlayTabTag { pulse?: TabPulse; home?: HomeTag; parent?: string; badge?: string }
export interface Snapshot { pin_sections?: string[]; automations?: AutomationSummary[]; overlay?: { tabs?: Record<string, OverlayTabTag> }; workspaces?: Workspace[]; tabs?: Tab[]; panes?: Pane[]; agents?: { terminal_id: string; pane_id: string; tab_id: string; workspace_id: string; agent: string; agent_status: string; work_status?: string; group?: { placement?: string; parent?: { pane_id?: string }; orphaned?: boolean; collapsed?: boolean } }[]; layouts?: Layout[] }
export interface SidebarRow { kind: "agent" | "pinned" | "home" | "space" | "tab"; id: string; label: string; status: string; hotkey: number | null; section: string; depth?: number; spaceLabel?: string; spaceId?: string; hidden?: boolean; parked?: boolean; pinned?: boolean; face?: Face; request?: string; home?: Tab["home_location"]; runsOn?: string; machine?: string; machineHealth?: string | null; doc?: TabDoc; homeTag?: HomeTag; represented?: boolean; hostBadge?: string; homeDetail?: string }
export const tabStatus = (snapshot: Snapshot, tab: Tab): string => tab.work_status ?? snapshot.agents?.find(agent => agent.tab_id === tab.tab_id)?.agent_status ?? tab.agent_status ?? "unknown";
export const statusRank = (status: string) => ({ blocked: 3, working: 2, done: 1 }[status] ?? 0);
export function buildSidebar(snapshot: Snapshot, _pending: PendingOrders = {}, now = Date.now()): SidebarRow[] {
  const tabs = snapshot.tabs ?? [];
  const tags = snapshot.overlay?.tabs ?? {};
  const represented = representedTabIds(snapshot);
  const homeIds = new Set(tabs.filter(tab => tags[tab.tab_id]?.home).map(tab => tab.tab_id));
  const byRank = (a: Workspace | Tab, b: Workspace | Tab) => (a.sort_rank ?? 0) - (b.sort_rank ?? 0) || a.number - b.number;
  const spaces = [...snapshot.workspaces ?? []].sort((a, b) => (a.sort_rank ?? 0) - (b.sort_rank ?? 0));
  const rows: SidebarRow[] = [];
  const row = (tab: Tab, kind: "home" | "tab", section: string): SidebarRow => ({ kind, section, id: tab.tab_id, label: tab.label || snapshot.panes?.find(p => p.tab_id === tab.tab_id)?.terminal_title_stripped || `tab ${tab.number}`, status: tabStatus(snapshot, tab), hotkey: null, pinned: tab.pin_index != null, spaceId: tab.workspace_id, spaceLabel: spaces.find(s => s.workspace_id === tab.workspace_id)?.label, ...(tab.doc && /^https?:\/\//i.test(tab.doc.url) ? { doc: tab.doc } : {}) });
  for (const section of ["NEEDS YOU", "WORKING", "FINISHED", "UTILITIES"]) {
    tabs.filter(tab => {
      const home = tags[tab.tab_id]?.home;
      return home && homeSection(home) === section;
    }).sort((a, b) => compareHomeTags(tags[a.tab_id]?.home, tags[b.tab_id]?.home)).forEach(tab => {
      const home = tags[tab.tab_id]!.home!;
      rows.push({ ...row(tab, "home", section), status: home.state, homeTag: home, hostBadge: tags[tab.tab_id]?.badge, homeDetail: homeTooltip(home, now) });
    });
  }
  for (const space of spaces.filter(s => s.tokens?.hidden !== "true").concat(spaces.filter(s => s.tokens?.hidden === "true"))) {
    const children = tabs.filter(t => t.workspace_id === space.workspace_id);
    const status = children.map(t => tabStatus(snapshot, t)).sort((a, b) => statusRank(b) - statusRank(a))[0] ?? "idle";
    rows.push({ kind: "space", id: space.workspace_id, label: space.label || `space ${space.number}`, status, hotkey: null, section: "spaces", hidden: space.tokens?.hidden === "true", ...(space.parked ? { parked: true } : {}) });
    children.filter(t => !homeIds.has(t.tab_id)).sort(byRank).forEach(t => rows.push({ ...row(t, "tab", space.workspace_id), represented: represented.has(t.tab_id), hidden: space.tokens?.hidden === "true" }));
  }
  const numbered = new Map<string, number>();
  for (const item of rows) {
    if (item.kind === "space" || item.represented) continue;
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
  if (rows.some(r => r.kind === "home" && r.id === selected)) return null;
  const tab = rows.find(r => r.kind === "tab" && r.id === selected && !r.represented);
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

export function selectionAfterClose(previous: SidebarRow[], rows: SidebarRow[], selected: string | null, initial?: string): string | null {
  const visible = (items: SidebarRow[]) => tabOrder(items.filter(r => !r.represented && !(r.kind === "agent" && r.hidden)));
  const order = visible(rows);
  if (selected) {
    const home = previous.find(r => r.id === selected && r.kind === "home");
    if (home) {
      const own = previous.filter(r => r.kind === "home" && r.section === home.section).map(r => r.id);
      const other = previous.filter(r => r.kind === "home" && r.section !== home.section).map(r => r.id);
      const index = own.indexOf(selected);
      const next = [...own.slice(index + 1), ...own.slice(0, index).reverse(), ...other].find(id => order.includes(id));
      if (next) return next;
    }
  }
  const index = selected ? visible(previous).indexOf(selected) : -1;
  return index >= 0 ? order[Math.min(index, order.length - 1)] ?? null : initial != null && rows.some(r => r.id === initial) ? initial : order[0] ?? null;
}
