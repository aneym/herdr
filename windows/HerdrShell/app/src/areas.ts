import type { Snapshot } from "./model";
import { tabStatus } from "./model";
import type { LaneSnapshot, ParkRecord } from "./laneFiles";
export type AreaChip = "all" | "needs" | "scoping" | "building" | "review" | "use" | "parked";
type RowKind = "orchestrator" | "lane" | "workflow";
interface Agent { tab_id: string; agent_status: string; tokens?: Record<string, string>; ownership?: { current?: { pane_id: string } } }
interface Facts { attention: boolean; failed: boolean; finished: boolean; devLoop: boolean }
export interface AreaItem { id: string; kind: RowKind; status: string; area: string; role: string; stage?: string; name: string; color: string; number: number; park?: ParkRecord; children: AreaItem[]; facts: Facts }
export interface AreaLine { id: string; kind: RowKind | "header" | "area" | "focus" | "parked"; title: string; trailing: string; depth: number; tab?: string; toggle?: string; chevron?: boolean; selected?: boolean; dim?: boolean; area?: string; color?: string; badge?: string; status?: string; role?: string; stage?: string; glyph?: string; glyphTone?: string; parked?: boolean; parkNote?: string }
export interface AreaOptions { chip?: AreaChip; areaOnly?: string | null; folded?: Set<string>; focusExpanded?: boolean; focusCursor?: number | null; selectedTab?: string | null; manualOpen?: Record<string, boolean> }
/** Swift's String `<` on these ASCII ids: code-point order, never locale collation. */
export function codePointOrder(a: string, b: string): number { return a < b ? -1 : a > b ? 1 : 0; }
/** The Mac sidebar's short stage words (Sidebar.swift stageWord). */
export function stageWord(badge: string): string {
  return ({ Scoping: "Scope", Building: "Build", "Ready for review": "Review", Monitoring: "Live", "In use": "desk" } as Record<string, string>)[badge] ?? badge;
}
/** A badge the active chip already says (Sidebar.swift stageImplied). */
export function stageImplied(chip: AreaChip, line: { stage?: string; role?: string }): boolean {
  const use = line.role === "desk" || line.role === "job";
  return chip === "scoping" ? line.stage === "scoping" : chip === "building" ? line.stage === "implementing" || use : chip === "review" ? line.stage === "reviewing" : chip === "use" ? use : false;
}
function areaItems(s: Snapshot, catalog: LaneSnapshot): AreaItem[] {
  const tabs = [...s.tabs ?? []].sort((a, b) => codePointOrder(a.workspace_id, b.workspace_id) || a.number - b.number);
  const agents = (s.agents ?? []) as Agent[];
  const byTab = new Map<string, Agent[]>();
  for (const agent of agents) byTab.set(agent.tab_id, [...byTab.get(agent.tab_id) ?? [], agent]);
  const tabByPane = new Map((s.panes ?? []).map(p => [p.pane_id, p.tab_id]));
  const firstTab = new Map<string, string>();
  for (const tab of tabs) if (!firstTab.has(tab.workspace_id)) firstTab.set(tab.workspace_id, tab.tab_id);
  const owned = new Map<string, AreaItem[]>(), groups: Record<RowKind, AreaItem[]> = { orchestrator: [], lane: [], workflow: [] };
  for (const t of tabs) {
    const aa = byTab.get(t.tab_id) ?? [];
    const token = (key: string) => aa.map(a => a.tokens?.[key]?.trim()).find(Boolean);
    const tagged = token("kind")?.toLowerCase();
    const kind: RowKind = tagged === "orchestrator" || tagged === "lane" || tagged === "workflow" ? tagged : t.label?.startsWith("wf ") || aa.some(a => a.ownership?.current) ? "workflow" : firstTab.get(t.workspace_id) === t.tab_id && aa.length > 0 ? "orchestrator" : "lane";
    const lane = catalog.lanes[t.tab_id], area = catalog.areaId(t.tab_id, t.workspace_id, lane), status = tabStatus(s, t);
    const failed = ["failed", "error", "quarantined", "stalled"].includes(token("state")?.toLowerCase() ?? "");
    const attention = token("attention")?.toLowerCase() ?? "";
    const item: AreaItem = { id: t.tab_id, kind, status, area, role: catalog.role(t.tab_id, lane), stage: lane?.section, name: catalog.displayName(t.tab_id, lane, t.label ?? `tab ${t.number}`), color: catalog.areaColor(area), number: t.number, park: catalog.parked[t.tab_id], children: [], facts: { failed, attention: status === "blocked" || failed || !["", "none", "false", "0", "no"].includes(attention), finished: status === "done" || ["done", "finished"].includes(token("phase")?.toLowerCase() ?? ""), devLoop: !!token("dev_loop") } };
    const owner = aa.map(a => tabByPane.get(a.ownership?.current?.pane_id ?? "")).find(id => id && id !== t.tab_id);
    if (kind === "workflow" && owner) owned.set(owner, [...owned.get(owner) ?? [], item]); else groups[kind].push(item);
  }
  for (const item of [...groups.orchestrator, ...groups.lane]) item.children = owned.get(item.id) ?? [];
  return [...groups.orchestrator, ...groups.lane, ...groups.workflow];
}
const flat = (i: AreaItem): AreaItem[] => [i, ...i.children.flatMap(flat)];
function liveItems(items: AreaItem[]): AreaItem[] {
  return items.flatMap(i => { const children = liveItems(i.children); return i.park ? children.map(c => c.area === "unsorted" ? { ...c, area: i.area } : c) : [{ ...i, children }]; });
}
export function parkedItems(items: AreaItem[]): AreaItem[] { return items.flatMap(flat).filter(i => i.park).sort((a, b) => (b.park?.at ?? -Infinity) - (a.park?.at ?? -Infinity) || a.number - b.number); }
export function focusTabs(s: Snapshot, catalog: LaneSnapshot): string[] {
  return focusForItems(areaItems(s, catalog), catalog);
}
function focusForItems(roots: AreaItem[], catalog: LaneSnapshot): string[] {
  const items = roots.flatMap(flat).filter(i => !i.park);
  const order = catalog.orderedAreaIds(new Set(items.map(i => i.area)));
  const bucket = (i: AreaItem) => i.stage === "closed" ? null : i.status === "blocked" ? 0 : i.stage === "reviewing" ? 1 : i.stage === "scoping" ? 2 : null;
  return items.filter(i => bucket(i) !== null).sort((a, b) => bucket(a)! - bucket(b)! || order.indexOf(a.area) - order.indexOf(b.area) || a.number - b.number || codePointOrder(a.id, b.id)).map(i => i.id);
}
export function passes(i: AreaItem, chip: AreaChip): boolean {
  if (chip === "parked") return !!i.park;
  if (i.park) return false;
  switch (chip) {
    case "all": return true;
    case "needs": return i.stage !== "closed" && (i.stage === "reviewing" || i.stage === "scoping" || i.status === "blocked");
    case "scoping": return i.stage === "scoping";
    case "building": return i.stage === "implementing";
    case "review": return i.stage === "reviewing";
    case "use": return i.stage !== "closed" && (i.role === "desk" || i.role === "job");
  }
}
function badge(i: AreaItem) { return i.role === "desk" || i.role === "job" ? "In use" : ({ scoping: "Scoping", implementing: "Building", reviewing: "Ready for review", monitoring: "Monitoring" } as Record<string, string>)[i.stage ?? ""] ?? ""; }
function parkWhen(at?: number): string {
  if (at === undefined) return "";
  const date = new Date(at), now = new Date();
  if (date.toDateString() === now.toDateString()) return `today ${date.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" })}`;
  return date.toLocaleDateString("en-US", { month: "short", day: "numeric", ...(date.getFullYear() !== now.getFullYear() ? { year: "numeric" } : {}) }).replace(",", "");
}
function itemLine(i: AreaItem, depth: number, selected: string | null | undefined, prefix: string): AreaLine {
  const status = i.facts.finished ? "done" : i.status;
  const glyph = i.facts.failed ? "✕" : ({ working: "●", blocked: "◐", idle: "○", done: "✓" } as Record<string, string>)[status] ?? "·";
  const glyphTone = i.facts.failed || status === "blocked" ? "warn" : status === "working" ? "ok" : "mute";
  return { role: i.role, stage: i.stage, glyph, glyphTone, id: prefix + i.id, kind: i.kind, depth, title: i.name + (i.facts.devLoop ? " ⟳" : ""), trailing: "", tab: i.id, selected: i.id === selected, dim: i.stage === "closed", area: i.area, color: i.color, badge: badge(i), status: i.facts.failed ? "blocked" : i.facts.finished ? "done" : i.status };
}
export function buildAreas(s: Snapshot | null, catalog: LaneSnapshot, opts: AreaOptions = {}): AreaLine[] {
  if (!s) return [];
  const { chip = "all", areaOnly, folded = new Set(), focusExpanded = false, focusCursor, selectedTab, manualOpen = {} } = opts;
  const items = areaItems(s, catalog), shown = chip === "parked" ? [] : liveItems(items).filter(i => passes(i, chip) && (!areaOnly || i.area === areaOnly));
  const focus = focusForItems(items, catalog), byId = new Map(items.flatMap(flat).map(i => [i.id, i]));
  const out: AreaLine[] = [{ id: "focus", kind: "focus", depth: 0, title: "Focus", trailing: focusCursor && focusCursor >= 1 && focusCursor <= focus.length ? `${focusCursor} of ${focus.length}` : `${focus.length}`, toggle: "focus", chevron: focusExpanded }];
  for (const id of focusExpanded ? focus : focus.slice(0, 1)) { const i = byId.get(id)!; const line = itemLine(i, 1, selectedTab, "focus:"); if (!focusExpanded) line.trailing = `next · ${catalog.areaName(i.area)}`; out.push(line); }
  for (const area of catalog.orderedAreaIds(new Set(shown.map(i => i.area)))) {
    const rows = shown.filter(i => i.area === area); if (!rows.length) continue;
    out.push({ id: `area:${area}`, kind: "area", depth: 0, title: catalog.areaName(area), trailing: `${rows.length}`, toggle: `area:${area}`, chevron: !folded.has(area), area, color: catalog.areaColor(area) });
    if (folded.has(area)) continue;
    for (const [title, roles] of [["ORCHESTRATOR", ["top", "orchestrator"]], ["PROJECTS", ["project"]], ["USE", ["desk", "job"]]] as const) {
      const group = rows.filter(i => (roles as readonly string[]).includes(i.role)).sort((a, b) => (title === "USE" ? Number(a.role !== "desk") - Number(b.role !== "desk") : 0) || Number(a.stage === "closed") - Number(b.stage === "closed") || a.number - b.number || codePointOrder(a.id, b.id));
      if (!group.length) continue;
      out.push({ id: `sub:${area}:${title}`, kind: "header", depth: 0, title, trailing: "", area });
      for (const i of group) {
        const line = itemLine(i, 1, selectedTab, "tab:");
        const children = i.children.filter(c => !c.park);
        const open = manualOpen[`tab:${i.id}`] ?? children.some(c => c.facts.attention);
        if (children.length) { line.chevron = open; line.toggle = `tab:${i.id}`; }
        out.push(line);
        if (open) for (const child of children) { const l = itemLine(child, 2, selectedTab, "tab:"); l.dim = l.dim || i.stage === "closed"; out.push(l); }
      }
    }
  }
  const parked = parkedItems(items).filter(i => !areaOnly || i.area === areaOnly);
  const parkLine = (i: AreaItem, depth: number) => { const line = itemLine(i, depth, selectedTab, "parked:"); line.badge = ""; line.dim = true; line.parked = true; line.parkNote = [parkWhen(i.park?.at), i.park?.note?.replace(/^Parked .*?: /, "") ?? ""].filter(Boolean).join(" · "); return line; };
  if (chip === "parked") return parked.map(i => parkLine(i, 0));
  if (parked.length) { out.push({ id: "parked", kind: "parked", depth: 0, title: "Parked", trailing: `${parked.length}`, toggle: "parked", chevron: manualOpen.parked ?? false, dim: true }); if (manualOpen.parked) out.push(...parked.map(i => parkLine(i, 1))); }
  return out;
}
