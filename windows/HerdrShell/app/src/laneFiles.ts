import { useEffect, useState } from "react";
import { bridge, fromBase64 } from "./bridge";

export interface LaneRecord { tab: string; name: string; label: string; kind?: string; goal?: string; goalArea?: string; section?: string; scopeURL?: string; reviewURL?: string }
export interface ParkRecord { note?: string; at?: number; by?: string }
export interface AreaDef { id: string; name: string; color: string }
export interface TabAssign { area?: string; role?: string; name?: string }
// The Mac catalog's missing-color value is area data, not a shell chrome color.
const defaultAreaColor = `#${(0x999999).toString(16)}`;
type ObjectMap = Record<string, unknown>;
const object = (value: unknown): ObjectMap => value !== null && typeof value === "object" && !Array.isArray(value) ? value as ObjectMap : {};
const str = (value: unknown): string | undefined => typeof value === "string" ? value.trim() || undefined : undefined;
const list = (value: unknown): ObjectMap[] => Array.isArray(value) ? value.map(object) : [];
const json = (value: unknown): ObjectMap => { try { return object(typeof value === "string" ? JSON.parse(value) : value); } catch { return {}; } };
const stringMap = (value: unknown): Record<string, string> => Object.fromEntries(Object.entries(object(value)).flatMap(([k, v]) => str(v) ? [[k, str(v)!]] : []));
export function parseLanes(value: unknown): { lanes: Record<string, LaneRecord>; parked: Record<string, ParkRecord> } {
  const lanes: Record<string, LaneRecord> = {}, parked: Record<string, ParkRecord> = {};
  for (const item of list(json(value).lanes)) {
    const tab = str(item.tab); if (!tab) continue;
    lanes[tab] = { tab, name: str(item.name) ?? str(item.label) ?? tab, label: str(item.label) ?? "", kind: str(item.kind), goal: str(item.goal), goalArea: str(item.goal_area), section: str(item.section), scopeURL: str(item.scope_url), reviewURL: str(item.review_url) };
    if (str(item.mode) === "parked") parked[tab] = {};
  }
  return { lanes, parked };
}
export function parseAreas(value: unknown) {
  const obj = json(value);
  return {
    areas: list(obj.areas).flatMap(a => { const id = str(a.id); return id ? [{ id, name: str(a.name) ?? id, color: str(a.color) ?? defaultAreaColor }] : []; }),
    tabs: Object.fromEntries(Object.entries(object(obj.tabs)).map(([id, v]) => { const d = object(v); return [id, { area: str(d.area), role: str(d.role), name: str(d.name) }]; })) as Record<string, TabAssign>,
    spaces: stringMap(obj.spaces), goalArea: stringMap(obj.goal_area), goal: stringMap(obj.goal),
  };
}
export function parseModes(value: unknown): Record<string, ParkRecord> {
  const out: Record<string, ParkRecord> = {};
  for (const [tab, v] of Object.entries(object(json(value).tabs))) {
    const d = object(v); if (str(d.mode) !== "parked") continue;
    const at = Date.parse(str(d.at) ?? "");
    out[tab] = { note: str(d.note), by: str(d.by), at: Number.isFinite(at) ? at : undefined };
  }
  return out;
}
export class LaneSnapshot {
  areas: AreaDef[] = [];
  lanes: Record<string, LaneRecord> = {};
  tabs: Record<string, TabAssign> = {};
  spaces: Record<string, string> = {};
  goalArea: Record<string, string> = {};
  goal: Record<string, string> = {};
  parked: Record<string, ParkRecord> = {};
  hasFiles = false;
  areaName(id: string) { return this.areas.find(a => a.id === id)?.name ?? id; }
  areaColor(id: string) { return this.areas.find(a => a.id === id)?.color ?? defaultAreaColor; }
  areaId(tab: string, workspace: string, lane?: LaneRecord) { return this.tabs[tab]?.area || this.spaces[workspace] || (lane?.goalArea && this.goalArea[lane.goalArea]) || (lane?.goal && this.goal[lane.goal]) || "unsorted"; }
  role(tab: string, lane?: LaneRecord) { return this.tabs[tab]?.role || (lane?.kind === "orchestrator" ? "orchestrator" : "project"); }
  displayName(tab: string, lane: LaneRecord | undefined, fallback: string) { return this.tabs[tab]?.name || lane?.name.replace(/^\[scoping\] /, "") || fallback; }
  orderedAreaIds(used: Set<string>) {
    const listed = this.areas.map(a => a.id).filter(id => id !== "unsorted");
    for (const id of [...used].sort()) if (id !== "unsorted" && !listed.includes(id)) listed.push(id);
    if (used.has("unsorted") || this.areas.some(a => a.id === "unsorted")) listed.push("unsorted");
    return listed;
  }
}
export function useLaneFiles(machine: string, up: boolean): LaneSnapshot {
  const [snapshot, setSnapshot] = useState(() => new LaneSnapshot());
  useEffect(() => {
    let disposed = false, busy = false, stamp = "";
    if (!up) return;
    const poll = async () => {
      if (disposed || busy || document.hidden) return;
      busy = true;
      try {
        const files = await Promise.all(["lanes", "areas", "modes"].map(name => bridge.fileRead(machine, `~/.agent-rails/herdr/${name}.json`, 0, 1 << 20).then(chunk => new TextDecoder().decode(fromBase64(chunk.data_b64)), () => null)));
        if (disposed) return;
        const nextStamp = JSON.stringify(files);
        if (nextStamp === stamp) return;
        stamp = nextStamp;
        const next = Object.assign(new LaneSnapshot(), parseLanes(files[0]), parseAreas(files[1]));
        // Only a parsed modes document overrides the lanes file's one-tick-behind parking.
        if (files[2] !== null) { try { const obj: unknown = JSON.parse(files[2]); if (obj && typeof obj === "object" && !Array.isArray(obj)) next.parked = parseModes(obj); } catch { /* A half-written document keeps the lanes fallback. */ } }
        next.hasFiles = files[0] !== null || files[1] !== null;
        setSnapshot(next);
      } finally { busy = false; }
    };
    void poll(); const timer = setInterval(() => void poll(), 5000);
    document.addEventListener("visibilitychange", poll);
    return () => { disposed = true; clearInterval(timer); document.removeEventListener("visibilitychange", poll); };
  }, [machine, up]);
  return snapshot;
}
