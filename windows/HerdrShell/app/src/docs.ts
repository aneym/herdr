import type { Snapshot } from "./model";
export interface Lane { tab: string; name: string; label: string; displayName: string; scopeURL?: string; reviewURL?: string }
export interface LaneCatalog { lanes: Record<string, Lane>; names: Record<string, string> }
export interface DocItem { name: string; kind: "web" | "markdown"; url?: string; path?: string }
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
