export function cycleMachine(names: readonly string[], current: string, direction: 1 | -1): string | null {
  if (!names.length) return null;
  const index = names.indexOf(current);
  if (index < 0) return direction === 1 ? names[0] : names[names.length - 1];
  return names[(index + direction + names.length) % names.length];
}
import { buildSidebar, statusRank } from "./model";
import { compareHomeTags } from "./home";
import type { SidebarRow, Snapshot } from "./model";
import type { MachineStatus } from "./bridge";

/** Another machine's last snapshot and health, as the Mac's MachineMerge.Machine: health is null
 *  while it is up, otherwise "unreachable". A machine with no snapshot yet adds nothing. */
export interface RemoteMachine { name: string; health: string | null; snapshot: Snapshot }
export function remoteMachines(machines: MachineStatus[], snapshots: Record<string, Snapshot>, active: string): RemoteMachine[] {
  return machines.filter(m => m.name !== active && snapshots[m.name]?.tabs).map(m => ({ name: m.name, health: m.state === "up" ? null : "unreachable", snapshot: snapshots[m.name] }));
}
/** `pc/w1:t2` -> ["pc", "w1:t2"]; a local id -> null. Local ids never contain `/`. */
export function splitRemote(id: string): [string, string] | null {
  const slash = id.indexOf("/");
  return slash > 0 ? [id.slice(0, slash), id.slice(slash + 1)] : null;
}
const labelKey = (label: string) => label.trim().toLowerCase();
export function mergeMachineRows(local: SidebarRow[], remotes: RemoteMachine[]): SidebarRow[] {
  if (!remotes.length) return local;
  const home = local.filter(r => r.kind === "home");
  const spaces: SidebarRow[][] = [];
  for (const row of local) {
    if (row.kind === "space") spaces.push([row]);
    else if (row.kind === "tab") spaces[spaces.length - 1]?.push(row);
  }
  const byLabel = new Map<string, SidebarRow[]>();
  for (const block of spaces) if (!byLabel.has(labelKey(block[0].label))) byLabel.set(labelKey(block[0].label), block);
  for (const remote of remotes) {
    const s = remote.snapshot;
    const shownTab = new Set((s.tabs ?? []).filter(t => t.pin_index != null || t.role === "agent" || s.agents?.some(a => a.tab_id === t.tab_id)).map(t => t.tab_id));
    const tag = (row: SidebarRow): SidebarRow => ({ ...row, id: `${remote.name}/${row.id}`, hotkey: null, machine: remote.name, machineHealth: remote.health, ...(row.spaceId ? { spaceId: `${remote.name}/${row.spaceId}` } : {}) });
    const rows = buildSidebar(s).filter(r => r.kind === "space" || r.kind === "home" || r.represented || shownTab.has(r.id));
    home.push(...rows.filter(r => r.kind === "home").map(tag));
    for (const space of rows.filter(r => r.kind === "space")) {
      const tabs = rows.filter(r => r.kind === "tab" && r.section === space.id);
      if (!tabs.length) continue;
      let block = byLabel.get(labelKey(space.label));
      if (!block) {
        block = [{ ...tag(space), section: "spaces", status: tabs.map(t => t.status).sort((a, b) => statusRank(b) - statusRank(a))[0] ?? "idle" }];
        byLabel.set(labelKey(space.label), block);
        spaces.push(block);
      }
      const homeSpace = block[0];
      block.push(...tabs.map(t => ({ ...tag(t), section: homeSpace.id, spaceId: homeSpace.id, spaceLabel: homeSpace.label, hidden: homeSpace.hidden })));
    }
  }
  const sectionRank = (row: SidebarRow) => ["NEEDS YOU", "WORKING", "FINISHED", "UTILITIES"].indexOf(row.section);
  return [...home.sort((a, b) => sectionRank(a) - sectionRank(b) || compareHomeTags(a.homeTag, b.homeTag)), ...spaces.flat()];
}
/** One name per machine, as the Mac's MachineRows.renameHosts: a host footer row that names a
 *  machine in another case ("PC" for "pc") takes the machine's name, only when it is the one machine
 *  and the one row of that spelling, so `pc` and `PC` stay two. */
export function renameHosts<T extends { name: string }>(hosts: T[], machines: string[]): T[] {
  const folds = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
  const names = hosts.map(h => h.name);
  return hosts.map(host => {
    if (machines.includes(host.name)) return host;
    const candidates = machines.filter(m => folds(m, host.name));
    if (candidates.length !== 1) return host;
    const name = candidates[0];
    return names.includes(name) || names.filter(n => folds(n, name)).length !== 1 ? host : { ...host, name };
  });
}
