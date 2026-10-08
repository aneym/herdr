import type { Snapshot } from "./model";

export interface AttentionNote { tab: string; kind: "blocked" | "done"; title: string; body: string }
function panes(snapshot: Snapshot) {
  const agents = new Map(snapshot.agents?.map(a => [a.pane_id, a.agent_status]));
  const tabs = new Map<string, Map<string, { status: string; title: string }>>();
  for (const pane of snapshot.panes ?? []) {
    const facts = tabs.get(pane.tab_id) ?? new Map();
    // The wire can carry terminal_title even though older shell snapshots omit it.
    const terminalTitle = (pane as typeof pane & { terminal_title?: string }).terminal_title;
    facts.set(pane.pane_id, { status: pane.agent_status ?? agents.get(pane.pane_id) ?? "unknown", title: (terminalTitle ?? pane.title ?? "").trim() });
    tabs.set(pane.tab_id, facts);
  }
  return tabs;
}

/** Mac Notifier.observe/capture: initial and new panes are silent; parked, not hidden, is the exclusion. */
export function observeAttention(previous: Snapshot | undefined, next: Snapshot, selected: string | null, windowFocused: boolean, parked: ReadonlySet<string>, lastSent: Readonly<Record<string, number>>, now: number): { notifications: AttentionNote[]; attention: boolean } {
  const old = previous ? panes(previous) : undefined;
  const current = panes(next);
  const labels = new Map(next.tabs?.map(t => [t.tab_id, t.label ?? `tab ${t.number}`]));
  const notifications: AttentionNote[] = [];
  let attention = false;
  for (const [tab, facts] of current) {
    if (parked.has(tab) || (windowFocused && selected === tab)) continue;
    if ([...facts.values()].some(p => p.status === "blocked" || p.status === "done")) attention = true;
    if (!old) continue;
    const blocked = [...facts].find(([id, fact]) => old.get(tab)?.has(id) && fact.status === "blocked" && old.get(tab)?.get(id)?.status !== "blocked");
    const done = blocked ? undefined : [...facts].find(([id, fact]) => old.get(tab)?.get(id)?.status === "working" && fact.status === "done");
    const transition = blocked ?? done;
    if (!transition || (lastSent[tab] !== undefined && now - lastSent[tab] < 60_000)) continue;
    const kind = blocked ? "blocked" : "done";
    const verb = blocked ? "needs you" : "finished";
    notifications.push({ tab, kind, title: labels.get(tab) ?? tab, body: transition[1].title ? `${verb} ${transition[1].title}` : verb });
  }
  return { notifications, attention };
}
