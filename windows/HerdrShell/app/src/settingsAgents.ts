import type { Snapshot } from "./model";
import { faceFor } from "./faces";
import type { Face } from "./faces";
// Settings > Agents: every agent pin with its face and whether it shows in the Agents list. Hidden is
// herdr's shared pin flag (tab.set_hidden), so a switch here reaches every client. The Mac builds the
// same rows in SettingsAgents.swift; shell/fixtures/settings-agents.json holds both to one answer.
export interface SettingsAgentRow { tab: string; title: string; meta: string; face: Face; shown: boolean }
/** Agent pins in the shared pin order; a hidden one keeps its slot. `pending` maps tab to shown until the server echoes it. */
export function settingsAgentRows(snapshot: Snapshot, pending: Record<string, boolean> = {}): SettingsAgentRow[] {
  const spaces = new Map((snapshot.workspaces ?? []).map(w => [w.workspace_id, w.label ?? w.workspace_id]));
  return (snapshot.tabs ?? [])
    .filter(tab => tab.role === "agent" && tab.pin_index != null)
    .sort((a, b) => a.pin_index! - b.pin_index! || (a.tab_id < b.tab_id ? -1 : a.tab_id > b.tab_id ? 1 : 0))
    .map(tab => {
      const card = tab.agent_card;
      const title = card?.name ?? tab.label ?? tab.tab_id;
      const meta = [spaces.get(tab.workspace_id), tab.runs_on].filter((part): part is string => !!part).join(" · ");
      return { tab: tab.tab_id, title, meta, face: faceFor(title, card?.avatar_url), shown: pending[tab.tab_id] ?? !tab.hidden };
    });
}
/** Drop pending toggles the server now agrees with. */
export function settleSettings(pending: Record<string, boolean>, snapshot: Snapshot): Record<string, boolean> {
  const hidden = new Map((snapshot.tabs ?? []).map(tab => [tab.tab_id, !!tab.hidden]));
  return Object.fromEntries(Object.entries(pending).filter(([tab, shown]) => hidden.has(tab) && hidden.get(tab) === shown));
}
