import { useEffect, useRef, useState } from "react";
import { bridge } from "./bridge";
import type { Snapshot } from "./model";
export interface Lane { tab: string; name: string; label: string; displayName: string; scopeURL?: string; reviewURL?: string }
export interface LaneCatalog { lanes: Record<string, Lane>; names: Record<string, string> }
export interface DocItem { name: string; kind: "web" | "markdown" | "file"; url?: string; path?: string; id?: string; mime?: string }
export interface DocsState { open: boolean; items: string[]; active: string | null }
const object = (value: unknown): Record<string, unknown> => value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
const str = (value: unknown) => typeof value === "string" ? value.trim() || undefined : undefined;
export function parseCatalog(lanesJSON: string, areasJSON = "{}"): LaneCatalog {
  const catalog: LaneCatalog = { lanes: Object.create(null), names: Object.create(null) };
  const raw = object(JSON.parse(lanesJSON)).lanes;
  if (Array.isArray(raw)) for (const value of raw) {
    const item = object(value), tab = str(item.tab);
    if (!tab) continue;
    const name = str(item.name) ?? str(item.label) ?? tab;
    catalog.lanes[tab] = { tab, name, label: str(item.label) ?? "", displayName: name, scopeURL: str(item.scope_url), reviewURL: str(item.review_url) };
  }
  for (const [tab, value] of Object.entries(object(object(JSON.parse(areasJSON)).tabs))) {
    const name = str(object(value).name);
    if (name) catalog.names[tab] = name;
  }
  return catalog;
}
// DocPanel.swift items: lookup by tab ID only; areas name, lane name, then tab label.
export function laneFor(tab: string | null, snapshot: Snapshot, catalog: LaneCatalog): Lane | null {
  if (!tab) return null;
  const lane = Object.prototype.hasOwnProperty.call(catalog.lanes, tab) ? catalog.lanes[tab] : undefined;
  const name = lane?.name;
  const displayName = (Object.prototype.hasOwnProperty.call(catalog.names, tab) ? catalog.names[tab] : undefined)
    ?? (name?.startsWith("[scoping] ") ? name.slice(10) : name)
    ?? snapshot.tabs?.find(t => t.tab_id === tab)?.label ?? "";
  return { ...(lane ?? { tab, name: "", label: "" }), displayName };
}
export function projectFolder(lane: Lane | null): string | null {
  if (!lane) return null;
  if (lane.scopeURL) {
    try {
      const route = new URL(lane.scopeURL, "http://shell.invalid").searchParams.get("route");
      const bits = route?.split("/").filter(Boolean);
      if (bits && bits.length >= 2 && bits[0] === "scoping") return `~/.agent-rails/scoping/${bits[1]}`;
    } catch { /* Non-URL scopes use the display-name folder. */ }
  }
  const folder = lane.displayName.replaceAll(" ", "-");
  return folder ? `~/.agent-rails/lanes/${folder}` : null;
}
export function docItems(lane: Lane | null, existing: ReadonlySet<string>): DocItem[] {
  if (!lane) return [];
  const items: DocItem[] = [];
  if (lane.scopeURL) items.push({ name: "Scope", kind: "web", url: lane.scopeURL });
  if (lane.reviewURL) items.push({ name: "Review", kind: "web", url: lane.reviewURL });
  const folder = projectFolder(lane);
  if (folder) for (const name of ["RESUME", "BRIEF", "DECISIONS"]) {
    const path = `${folder}/${name}.md`;
    if (existing.has(path)) items.push({ name, kind: "markdown", path });
  }
  return items;
}

export interface DeskItem { id: string; kind: string; ref: string; title: string; mime: string; opened_by: string; opened_at_ms: number }
export interface DeskInfo { items: DeskItem[]; front: string | null }
export const emptyDesk: DeskInfo = { items: [], front: null };
export function deskFor(snapshot: Snapshot, tab: string | null): DeskInfo {
  return (snapshot.tabs?.find(t => t.tab_id === tab) as ({ desk?: DeskInfo } | undefined))?.desk ?? emptyDesk;
}
const docKey = (item: DocItem) => item.id ?? item.name;
// Server snapshots carry tab.desk; desk.changed invalidates that snapshot, exactly as on Mac.
export function useDesk(machine: string, tab: string | null, snapshot: Snapshot, catalog: DocItem[], landed: (tab: string) => void) {
  const [activeKeys, setActiveKeys] = useState<Record<string, string>>({});
  const fronts = useRef<Record<string, string | null>>({});
  const seen = useRef<{ machine: string; ids: Record<string, Set<string>> } | null>(null);
  const onLanded = useRef(landed); onLanded.current = landed;
  const desk = deskFor(snapshot, tab);
  const items: DocItem[] = [...catalog, ...desk.items.map(item => ({ id: item.id, name: item.title, kind: item.kind === "url" ? "web" as const : "file" as const, url: item.kind === "url" ? item.ref : undefined, path: item.kind === "file" ? item.ref : undefined, mime: item.mime }))];
  const key = `${machine}.${tab}`;
  const old = activeKeys[key];
  const valid = items.some(item => docKey(item) === old);
  const front = desk.items.some(item => item.id === desk.front) ? desk.front : desk.items[0]?.id;
  const active = front && (fronts.current[key] !== desk.front || !valid) ? front : valid ? old : items[0] ? docKey(items[0]) : null;
  useEffect(() => {
    if (!snapshot.tabs?.length) return;
    const ids: Record<string, Set<string>> = {};
    for (const t of snapshot.tabs) {
      const current = deskFor(snapshot, t.tab_id);
      ids[t.tab_id] = new Set(current.items.map(item => item.id));
      if (seen.current?.machine === machine && current.items.some(item => !seen.current!.ids[t.tab_id]?.has(item.id))) {
        onLanded.current(t.tab_id);
        // MainWindow.refreshDocs selects the server front even for a background arrival.
        if (current.front) setActiveKeys(value => ({ ...value, [`${machine}.${t.tab_id}`]: current.front! }));
      }
    }
    seen.current = { machine, ids };
  }, [machine, snapshot]);
  useEffect(() => {
    fronts.current[key] = desk.front;
    if (active !== null && active !== old) setActiveKeys(value => ({ ...value, [key]: active }));
  }, [key, desk.front, active, old]);
  const select = (id: string) => {
    setActiveKeys(value => ({ ...value, [key]: id }));
    if (desk.items.some(item => item.id === id)) void bridge.api(machine, "desk.focus", { tab_id: tab, item: id }).catch(console.error);
  };
  return { items, active, select };
}
export { docKey };
