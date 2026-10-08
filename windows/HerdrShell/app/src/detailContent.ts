import type { Snapshot } from "./model";
import { tabStatus } from "./model";
export type DetailKind = "orchestrator" | "lane" | "workflow";
export interface DetailItem { text: string; source: string }
export interface DetailWorkflow { id: string; label: string; phase: string; status: string; host: string }
export interface DetailRow { id: string; label: string; kind: DetailKind; status: string; host: string; agent?: string; children: DetailWorkflow[] }
export interface DetailContent extends DetailRow { inbox: DetailItem[]; routed: DetailItem[]; groups: { lane?: string; workflows: DetailWorkflow[] }[] }
type Agent = NonNullable<Snapshot["agents"]>[number] & { tokens?: Record<string, string>; ownership?: { current?: { pane_id: string } } };
function token(snapshot: Snapshot, tab: string, key: string): string | undefined {
  return (snapshot.agents as Agent[] | undefined)?.filter(a => a.tab_id === tab).map(a => a.tokens?.[key]?.trim()).find(Boolean);
}
/** Same last-@ source grammar as HerdrDetailProvider.items; inbox counts aren't content. */
export function detailItems(raw: string | undefined, source: string): DetailItem[] {
  return (raw ?? "").split("|").map(s => s.trim()).filter(Boolean).map(text => {
    const at = text.lastIndexOf("@"), suffix = text.slice(at + 1).trim();
    return at > 0 && suffix && !suffix.includes(" ") ? { text: text.slice(0, at).trim(), source: suffix } : { text, source };
  });
}
/** Snapshot classification/ownership mirrors TabClassifier and HerdrModel.apply. */
export function detailRows(snapshot: Snapshot): DetailRow[] {
  const tabs = [...snapshot.tabs ?? []].sort((a, b) => a.workspace_id < b.workspace_id ? -1 : a.workspace_id > b.workspace_id ? 1 : a.number - b.number);
  const first = new Map<string, string>(), owned = new Map<string, DetailWorkflow[]>();
  const rows: DetailRow[] = [];
  for (const tab of tabs) {
    if (!first.has(tab.workspace_id)) first.set(tab.workspace_id, tab.tab_id);
    const agents = (snapshot.agents as Agent[] | undefined)?.filter(a => a.tab_id === tab.tab_id) ?? [];
    const tagged = token(snapshot, tab.tab_id, "kind")?.toLowerCase();
    const kind: DetailKind = tagged === "orchestrator" || tagged === "lane" || tagged === "workflow" ? tagged : tab.label?.startsWith("wf ") || agents.some(a => a.ownership?.current) ? "workflow" : first.get(tab.workspace_id) === tab.tab_id && agents.length ? "orchestrator" : "lane";
    const row: DetailRow = { id: tab.tab_id, label: tab.label ?? `tab ${tab.number}`, kind, status: tabStatus(snapshot, tab), host: token(snapshot, tab.tab_id, "host") ?? "Studio", agent: agents[0]?.agent, children: [] };
    const owner = agents.map(a => snapshot.panes?.find(p => p.pane_id === a.ownership?.current?.pane_id)?.tab_id).find(id => id && id !== tab.tab_id);
    if (kind === "workflow" && owner) owned.set(owner, [...owned.get(owner) ?? [], { id: row.id, label: row.label, status: row.status, host: row.host, phase: token(snapshot, tab.tab_id, "phase") ?? row.status }]);
    else rows.push(row);
  }
  for (const row of rows) if (row.kind !== "workflow") row.children = owned.get(row.id) ?? [];
  return rows;
}
export function buildDetailContent(snapshot: Snapshot, rowId: string): DetailContent | null {
  const rows = detailRows(snapshot), row = rows.find(r => r.id === rowId), tab = snapshot.tabs?.find(t => t.tab_id === rowId);
  if (!row || !tab || row.kind === "workflow") return null;
  const groups: DetailContent["groups"] = row.children.length ? [{ workflows: row.children }] : [];
  const asks = (workflows: DetailWorkflow[]) => workflows.filter(w => w.status === "blocked").map(w => ({ text: `${w.label} wants you`, source: "blocked" }));
  const inbox = [...detailItems(token(snapshot, rowId, "inbox_items"), "inbox"), ...asks(row.children)];
  if (row.kind === "orchestrator") for (const lane of rows.filter(r => r.kind === "lane" && snapshot.tabs?.find(t => t.tab_id === r.id)?.workspace_id === tab.workspace_id)) {
    if (lane.status === "blocked") inbox.push({ text: `${lane.label} wants you`, source: "blocked" });
    if (lane.children.length) groups.push({ lane: lane.label, workflows: lane.children });
    inbox.push(...asks(lane.children));
  }
  return { ...row, inbox, routed: row.kind === "orchestrator" ? detailItems(token(snapshot, rowId, "routed"), "routed") : [], groups };
}
