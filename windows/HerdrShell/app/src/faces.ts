import type { Snapshot } from "./model";
// An AGENTS row's face, as the Mac's AgentFace and Rails PersonFace: the agent's https picture,
// else its initial on one of nine tints (shell/tokens.json `face.tints`) picked from its name.
export interface AgentCard { name: string; avatar?: string }
export interface Face { initial: string; tint: number; avatar?: string }
export type FaceDot = "ok" | "accent" | "warn";
export const FACE_TINTS = 9;
export const AGENTS_DIR = "~/.agent-rails/agents";
const text = (value: unknown) => typeof value === "string" && value.trim() ? value.trim() : undefined;
/** Pictures load over https only, whatever the scheme's case. */
export function isPicture(url: string): boolean { try { return new URL(url).protocol === "https:"; } catch { return false; } }
/** One `<agents dir>/<name>/agent.json`; a card with no name or no pane (`"none"` included) names no row. */
export function parseCard(json: string): { pane: string; card: AgentCard } | null {
  let obj: Record<string, unknown>;
  try { obj = JSON.parse(json); } catch { return null; }
  if (!obj || typeof obj !== "object") return null;
  const name = text(obj.name), pane = text(obj.pane), avatar = text(obj.avatar_url);
  if (!name || !pane || pane === "none") return null;
  return { pane, card: avatar && isPicture(avatar) ? { name, avatar } : { name } };
}
/** Cards by tab: the first card whose pane the tab holds, in snapshot pane order (Mac AgentCards.attach). */
export function cardsByTab(snapshot: Snapshot, cards: Record<string, AgentCard>): Record<string, AgentCard> {
  const out: Record<string, AgentCard> = {};
  for (const pane of snapshot.panes ?? []) if (!out[pane.tab_id] && cards[pane.pane_id]) out[pane.tab_id] = cards[pane.pane_id];
  return out;
}
/** Rails personTint: a hash of the trimmed, lowercased name's code points; the initial is the first code point. */
export function faceFor(name: string, avatar?: string): Face {
  const key = name.trim();
  let hash = 7;
  for (const character of key.toLowerCase()) hash = (hash * 31 + character.codePointAt(0)!) >>> 0;
  const first = [...key][0];
  return { initial: first ? first.toUpperCase() : "?", tint: hash % FACE_TINTS, ...(avatar ? { avatar } : {}) };
}
/** An open request or a blocked agent needs you (the row's one blue dot), working is green, done is peach, idle has none. */
export function faceDot(status: string, request?: string): FaceDot | null {
  if (request) return "accent";
  return ({ working: "ok", blocked: "accent", done: "warn" } as Record<string, FaceDot>)[status] ?? null;
}
export function faceHover(status: string, request?: string): string {
  if (request) return `Needs you, request ${request}`;
  return ({ working: "Working", blocked: "Needs you", done: "Done" } as Record<string, string>)[status] ?? "";
}
