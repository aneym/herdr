// An AGENTS row's face, as the Mac's AgentFace and Rails PersonFace: the agent's https picture,
// else its initial on one of nine tints (shell/tokens.json `face.tints`) picked from its name.
export interface Face { initial: string; tint: number; avatar?: string }
export type FaceDot = "working" | "ok" | "accent" | "warn";
export const FACE_TINTS = 9;
/** Rails personTint: a hash of the trimmed, lowercased name's code points; the initial is the first code point. */
export function faceFor(name: string, avatar?: string): Face {
  const key = name.trim();
  let hash = 7;
  for (const character of key.toLowerCase()) hash = (hash * 31 + character.codePointAt(0)!) >>> 0;
  const first = [...key][0];
  return { initial: first ? first.toUpperCase() : "?", tint: hash % FACE_TINTS, ...(avatar ? { avatar } : {}) };
}
/** An open request or a blocked agent needs you (the row's one blue dot), working is orange, done is green, idle has none. */
export function faceDot(status: string, request?: string): FaceDot | null {
  if (request) return "accent";
  return ({ working: "working", blocked: "accent", done: "ok" } as Record<string, FaceDot>)[status] ?? null;
}
export function faceHover(status: string, request?: string): string {
  if (request) return `Needs you, request ${request}`;
  return ({ working: "Working", blocked: "Needs you", done: "Done" } as Record<string, string>)[status] ?? "";
}
