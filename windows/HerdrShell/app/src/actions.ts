import type { Action } from "./keys";
import type { Layout, Snapshot, SidebarRow } from "./model";
import { tabOrder } from "./model";
export type Direction = "left" | "right" | "up" | "down";
// Match Mac Shell: nearest facing edge, then largest perpendicular overlap.
export function neighbor(layout: Layout, paneId: string, dir: Direction): string | null {
  if (layout.zoomed) return null;
  const cur = layout.panes.find(p => p.pane_id === paneId)?.rect;
  if (!cur) return null;
  let best: { id: string; gap: number; overlap: number } | undefined;
  for (const { pane_id: id, rect: r } of layout.panes) {
    if (id === paneId) continue;
    const horizontal = dir === "left" || dir === "right";
    const gap = dir === "left" ? cur.x - (r.x + r.width) : dir === "right" ? r.x - (cur.x + cur.width) : dir === "up" ? cur.y - (r.y + r.height) : r.y - (cur.y + cur.height);
    const overlap = horizontal ? Math.min(cur.y + cur.height, r.y + r.height) - Math.max(cur.y, r.y) : Math.min(cur.x + cur.width, r.x + r.width) - Math.max(cur.x, r.x);
    if (gap < -0.5 || overlap <= 0) continue;
    if (!best || gap < best.gap || (gap === best.gap && overlap > best.overlap)) best = { id, gap, overlap };
  }
  return best?.id ?? null;
}
export interface ActionContext {
  machine: string; snapshot: Snapshot; rows: SidebarRow[]; selected: string | null; focused: string | null;
  api: (machine: string, method: string, params: unknown) => Promise<unknown>;
  select: (id: string) => void; focus: (id: string) => void;
  created: (tabId: string, paneId: string) => void;
  rename: (id: string) => void; switcher: () => void; toggleSidebar: () => void;
  error: (error: unknown) => void;
  label?: string; tabId?: string;
}
function responseId(value: unknown, field: string, id: string): string {
  const result = value as Record<string, Record<string, unknown>> | null;
  const found = result?.[field]?.[id];
  if (typeof found !== "string" || !found) throw new Error(`API response missing ${field}.${id}`);
  return found;
}
export async function runAction(action: Action, ctx: ActionContext): Promise<void> {
  try {
    const order = tabOrder(ctx.rows);
    const step = (ids: string[], current: string | null, amount: number) => {
      if (!ids.length) return undefined;
      const index = ids.indexOf(current ?? "");
      return ids[index < 0 ? (amount > 0 ? 0 : ids.length - 1) : (index + amount + ids.length) % ids.length];
    };
    const tab = ctx.snapshot.tabs?.find(t => t.tab_id === ctx.selected);
    const pane = () => { if (!ctx.focused) throw new Error("No focused pane"); return ctx.focused; };
    const api = (method: string, params: unknown) => ctx.api(ctx.machine, method, params);
    if (/^select_tab_[1-9]$/.test(action)) {
      const id = ctx.rows.find(r => r.kind !== "space" && r.hotkey === Number(action.slice(11)))?.id;
      if (id) ctx.select(id);
    } else if (action === "next_tab" || action === "prev_tab") {
      const id = step(order, ctx.selected, action === "next_tab" ? 1 : -1); if (id) ctx.select(id);
    } else if (action === "next_attention") {
      const attention = ["blocked", "done"].flatMap(status => order.filter(id => ctx.rows.some(r => r.kind !== "space" && r.id === id && r.status === status)));
      const id = step(attention, ctx.selected, 1); if (id) ctx.select(id);
    } else if (action === "toggle_sidebar") ctx.toggleSidebar();
    else if (action === "switcher") ctx.switcher();
    else if (action === "rename_tab") {
      const id = ctx.tabId ?? ctx.selected;
      if (!id) throw new Error("No selected tab");
      if (ctx.label === undefined) ctx.rename(id);
      else await api("tab.rename", { tab_id: id, label: ctx.label });
    } else if (action === "new_tab") {
      if (!tab) throw new Error("No selected tab workspace");
      const result = await api("tab.create", { workspace_id: tab.workspace_id, focus: false });
      ctx.created(responseId(result, "tab", "tab_id"), responseId(result, "root_pane", "pane_id"));
    } else if (action === "split_right" || action === "split_down") {
      if (!tab) throw new Error("No selected tab");
      const result = await api("pane.split", { target_pane_id: pane(), direction: action === "split_right" ? "right" : "down", focus: false });
      ctx.created(tab.tab_id, responseId(result, "pane", "pane_id"));
    } else if (action === "close_pane") await api("pane.close", { pane_id: pane() });
    else if (action === "zoom_pane") await api("pane.zoom", { pane_id: pane(), mode: "toggle" });
    else if (action.startsWith("focus_pane_") || action === "next_pane" || action === "prev_pane") {
      const layout = ctx.snapshot.layouts?.find(l => l.tab_id === ctx.selected);
      const id = action.startsWith("focus_pane_") ? layout && neighbor(layout, pane(), action.slice(11) as Direction)
        : step((ctx.snapshot.panes ?? []).filter(p => p.tab_id === ctx.selected && (!layout?.zoomed || p.pane_id === layout.focused_pane_id)).map(p => p.pane_id), ctx.focused, action === "next_pane" ? 1 : -1);
      if (id) ctx.focus(id);
    } else throw new Error(`Unknown action: ${action}`);
  } catch (error) { ctx.error(error); throw error; }
}
