import { useEffect, useState } from "react";
import { bridge, fromBase64 } from "./bridge";
import type { SidebarRow } from "./model";

/** The factory overlay's tab tag, normalised as the Mac's Overlay.Tag decoder (SpacesTree.swift). */
export interface OverlayTag { kind: string; section?: string; mode: string; parent?: string; goal?: string; goalArea?: string; done: boolean }
export interface OverlayHost { name: string; summary?: string; attention: string; url?: string }
export interface SpaceGroup { name: string; spaces: string[] }
export interface SpacesOverlay { tabs: Record<string, OverlayTag>; hosts: OverlayHost[]; spaceGroups: SpaceGroup[] }
export const emptyOverlay = (): SpacesOverlay => ({ tabs: {}, hosts: [], spaceGroups: [] });
type Obj = Record<string, unknown>;
const obj = (v: unknown): Obj => v !== null && typeof v === "object" && !Array.isArray(v) ? v as Obj : {};
const list = (v: unknown): unknown[] => Array.isArray(v) ? v : [];
const str = (v: unknown): string | undefined => typeof v === "string" ? v : undefined;
export function parseSpaceGroups(value: unknown): SpaceGroup[] {
  return list(value).map(obj).map(g => ({ name: str(g.name) ?? "", spaces: list(g.spaces).filter((s): s is string => typeof s === "string") }));
}
export function parseOverlay(text: string | null | undefined): SpacesOverlay {
  let root: Obj;
  try { root = obj(text ? JSON.parse(text) : null); } catch { return emptyOverlay(); }
  const tabs: Record<string, OverlayTag> = {};
  for (const [id, value] of Object.entries(obj(root.tabs))) {
    const t = obj(value);
    let kind = str(t.kind) ?? "unknown"; if (!["orchestrator", "lane", "workflow", "advisor"].includes(kind)) kind = "unknown";
    let mode = str(t.mode) ?? "active"; if (!["parked", "auto"].includes(mode)) mode = "active";
    let section = str(t.section);
    if (section === "inflight" || section === "idle") section = "implementing";
    if (section === "waiting" || section === "ready" || section === "ready_for_review") section = "reviewing";
    if (!["orchestrator", "scoping", "implementing", "reviewing", "monitoring", "closed"].includes(section ?? "")) section = undefined;
    tabs[id] = { kind, mode, section, parent: str(t.parent), goal: str(t.goal), goalArea: str(t.goal_area), done: t.done === true };
  }
  const hosts = list(root.hosts).map(obj).map(h => ({ name: str(h.name) ?? "", summary: str(h.summary), attention: str(h.attention) ?? "none", url: str(h.url) }));
  return { tabs, hosts, spaceGroups: parseSpaceGroups(root.space_groups) };
}
/** As the Mac's Overlay.goalChoices: each known goal that some tab carries, then its areas. */
export function goalChoices(overlay: SpacesOverlay): string[] {
  const tags = Object.values(overlay.tabs);
  return ["recruiter", "closer", "rails"].flatMap(goal => {
    const mine = tags.filter(t => t.goal === goal);
    return mine.length ? [goal, ...[...new Set(mine.map(t => t.goalArea).filter((a): a is string => !!a))].sort().map(a => `${goal}:${a}`)] : [];
  });
}
/** As the Mac's Overlay.spaceGroup: the first group naming the space by id, or by label ignoring case. */
export function spaceGroupIndex(groups: SpaceGroup[], id: string, label: string): number {
  const key = label.trim().toLowerCase();
  return groups.findIndex(g => g.spaces.some(m => { const member = m.trim(); return member === id || (!!key && member.toLowerCase() === key); }));
}
/** Spaces in their named groups, in group order and the group's member order; the rest after. */
export function groupSpaces(spaces: SidebarRow[], groups: SpaceGroup[]): { group: string | null; spaces: SidebarRow[] }[] {
  const named = groups.filter(g => g.name.trim());
  const members = named.map(() => [] as SidebarRow[]), rest: SidebarRow[] = [];
  for (const space of spaces) { const i = spaceGroupIndex(named, space.id, space.label); if (i >= 0) members[i].push(space); else rest.push(space); }
  const out = named.flatMap((g, i) => {
    if (!members[i].length) return [];
    const at = (s: SidebarRow) => { const p = g.spaces.findIndex(m => m === s.id || m.trim().toLowerCase() === s.label.trim().toLowerCase()); return p < 0 ? Number.MAX_SAFE_INTEGER : p; };
    return [{ group: g.name, spaces: members[i].map((s, n) => ({ s, n })).sort((a, b) => at(a.s) - at(b.s) || a.n - b.n).map(x => x.s) }];
  });
  return rest.length ? [...out, { group: null, spaces: rest }] : out;
}
export interface SpaceSection { key: string; title: string; kind: "section" | "group"; foldable: boolean; trailing: string; members: SidebarRow[] }
/** One space's sections, as the Mac's SpacesTree.appendSpace over SpaceScope: ORCHESTRATOR, then
 *  the four stage sections when any tab carries one (else LANES), then the folded services, parked,
 *  closed and background groups. The goal filter applies only to a sectioned top-level space.
 *  `children` maps a header to the workflows nested under it (folded by default on the Mac). */
export function spaceSections(spaceId: string, tabs: SidebarRow[], overlay: SpacesOverlay, filter: string | null, topLevel: boolean): { sections: SpaceSection[]; children: Record<string, SidebarRow[]> } {
  const none: OverlayTag = { kind: "unknown", mode: "active", done: false };
  const tag = (row: SidebarRow) => overlay.tabs[row.id] ?? none;
  const leader = tabs.find(t => tag(t).kind === "orchestrator" && !tag(t).done)?.id;
  const sectioned = tabs.some(t => overlay.tabs[t.id]?.section != null);
  const shown = tabs.filter(t => {
    if (!sectioned || !topLevel || !filter) return true;
    const tg = overlay.tabs[t.id]; if (!tg) return false;
    const [goal, area] = filter.split(/:(.*)/s);
    return tg.kind === "orchestrator" || tg.mode === "auto" || (tg.goal === goal && (!area || tg.goalArea === area));
  });
  const foreground = shown.filter(t => !tag(t).done && tag(t).kind !== "advisor");
  const background = shown.filter(t => { const g = tag(t); return g.kind === "advisor" || (g.done && g.kind !== "workflow" && !(g.kind === "lane" && g.mode !== "active")); });
  const orch = foreground.filter(t => tag(t).kind === "orchestrator");
  const lanes = shown.filter(t => tag(t).kind === "lane" && (!tag(t).done || tag(t).mode !== "active"));
  const workflows = foreground.filter(t => tag(t).kind === "workflow");
  const ordinary = foreground.filter(t => !["orchestrator", "lane", "workflow"].includes(tag(t).kind));
  const parent = (t: SidebarRow) => { const p = tag(t).parent; return lanes.some(l => l.id === p) ? p : leader; };
  const children: Record<string, SidebarRow[]> = {};
  for (const w of workflows) { const p = parent(w); if (p && p !== w.id) (children[p] ??= []).push(w); }
  const topWorkflows = workflows.filter(w => !parent(w) || parent(w) === w.id);
  const sections: SpaceSection[] = [];
  const section = (title: string, members: SidebarRow[], count = false) => {
    if (!members.length) return;
    sections.push({ key: `section:${spaceId}:${title}`, title, kind: "section", foldable: sectioned, trailing: count ? String(members.length) : "", members });
  };
  const active = (t: SidebarRow) => tag(t).mode === "active";
  section("ORCHESTRATOR", [...orch, ...lanes.filter(t => tag(t).section === "orchestrator" && active(t))]);
  if (sectioned) {
    for (const [value, title] of [["reviewing", "READY FOR REVIEW"], ["scoping", "SCOPING"], ["implementing", "IMPLEMENTING"], ["monitoring", "MONITORING"]] as const) {
      section(title, [...lanes.filter(t => active(t) && (tag(t).section ?? "implementing") === value), ...(value === "implementing" ? ordinary : [])], value === "reviewing");
    }
  } else section("LANES", [...lanes.filter(active), ...topWorkflows, ...ordinary]);
  for (const [name, members] of [["services", [...lanes.filter(t => tag(t).mode === "auto"), ...(sectioned ? topWorkflows : [])]], ["parked", lanes.filter(t => tag(t).mode === "parked")], ["closed", sectioned ? lanes.filter(t => active(t) && tag(t).section === "closed") : []], ["background", background]] as const) {
    if (members.length) sections.push({ key: `group:${spaceId}:${name}`, title: `${name} ${members.length}`, kind: "group", foldable: true, trailing: "", members: [...members] });
  }
  return { sections, children };
}
/** The overlay the Mac reads (`~/.agent-rails/herdr/overlay.json`, FactorySources), from a machine. */
export function useSpacesOverlay(machine: string, up: boolean): SpacesOverlay {
  const [overlay, setOverlay] = useState(emptyOverlay);
  useEffect(() => {
    let disposed = false, busy = false, stamp = "";
    if (!up) return;
    const poll = async () => {
      if (disposed || busy || document.hidden) return;
      busy = true;
      try {
        const path = "~/.agent-rails/herdr/overlay.json";
        const text = await bridge.fileRead(machine, path, 0, 8 << 20).then(chunk => new TextDecoder().decode(fromBase64(chunk.data_b64)), () => bridge.fileStat(machine, path).then(info => info.exists ? undefined : null, () => undefined));
        // Any failed read but a missing file keeps the last good overlay.
        if (disposed || text === undefined || text === stamp) return;
        stamp = text ?? "";
        setOverlay(parseOverlay(text));
      } finally { busy = false; }
    };
    void poll(); const timer = setInterval(() => void poll(), 2000);
    document.addEventListener("visibilitychange", poll);
    return () => { disposed = true; clearInterval(timer); document.removeEventListener("visibilitychange", poll); };
  }, [machine, up]);
  return overlay;
}
