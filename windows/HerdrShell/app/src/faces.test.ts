import { describe, expect, it } from "vitest";
import { faceDot, faceFor } from "./faces";
// Faces must match the Mac and Rails for the same agent: the tint hash, JS trim and the first code
// point have Unicode edge cases a golden table cannot enumerate. Expected tints come from the
// independent Python port of Rails personTint in macos/HerdrShell/scripts/check_agents_section.py.
describe("agent faces", () => {
  it("picks the Rails tint and the first code point, after JS trim", () => {
    const cases: [string, string, number][] = [
      ["frank", "F", 6], ["Recruiter", "R", 6], ["Content", "C", 2], ["remote chat", "R", 0],
      ["\ufeffe\u0301mile\u3000", "E", 2], ["\u200bAlpha", "\u200b", 2], ["ßeta", "SS", 7], ["🦊 fox", "🦊", 1], ["  ", "?", 7],
    ];
    for (const [name, initial, tint] of cases) expect(faceFor(name), JSON.stringify(name)).toEqual({ initial, tint });
  });
  it("puts one dot in the face: an open request is blue ahead of working and done; idle has none", () => {
    expect(["working", "blocked", "done", "idle", "unknown"].map(s => faceDot(s))).toEqual(["working", "accent", "ok", null, null]);
    expect(["working", "done", "idle"].map(s => faceDot(s, "req-1"))).toEqual(["accent", "accent", "accent"]);
  });
});
