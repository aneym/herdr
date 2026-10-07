import { describe, expect, it } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import { faceDot, faceFor, parseCard } from "./faces";
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
  it("reads agent.json as the Mac does: https pictures only, a pane-less or broken card names no row", () => {
    expect(parseCard(JSON.stringify({ name: " frank ", pane: "w5H:p137", avatar_url: "HTTPS://example.com/f.png" }))).toEqual({ pane: "w5H:p137", card: { name: "frank", avatar: "HTTPS://example.com/f.png" } });
    expect(parseCard(JSON.stringify({ name: "Recruiter", pane: "w5P:p9", avatar_url: "http://example.com/r.png" }))).toEqual({ pane: "w5P:p9", card: { name: "Recruiter" } });
    expect(parseCard(JSON.stringify({ name: "Recruiter", pane: "w5P:p9", avatar_url: "javascript:alert(1)" }))?.card.avatar).toBeUndefined();
    for (const text of [JSON.stringify({ name: "p7probe", pane: "none" }), JSON.stringify({ pane: "w1:p1" }), JSON.stringify({ name: "x", pane: 3 }), "{not json", "null", ""])
      expect(parseCard(text), text).toBeNull();
  });
  it("puts one dot in the face: an open request is blue ahead of working and done; idle has none", () => {
    expect(["working", "blocked", "done", "idle", "unknown"].map(s => faceDot(s))).toEqual(["ok", "accent", "warn", null, null]);
    expect(["working", "done", "idle"].map(s => faceDot(s, "req-1"))).toEqual(["accent", "accent", "accent"]);
  });
  it("gives agent rows the face of the card whose pane the tab holds, and its open request", () => {
    const snapshot: Snapshot = {
      workspaces: [{ workspace_id: "w1", number: 1, label: "home" }],
      tabs: [
        { tab_id: "w1:t1", workspace_id: "w1", number: 1, label: "Frank tab", role: "agent", pin_index: 0, work_status: "working" },
        { tab_id: "w1:t2", workspace_id: "w1", number: 2, label: "Content", role: "agent", pin_index: 1, work_status: "done" },
        { tab_id: "w1:t3", workspace_id: "w1", number: 3, label: "plain", pin_index: 0 },
      ],
      panes: [
        { pane_id: "p1", terminal_id: "a", workspace_id: "w1", tab_id: "w1:t1", tokens: { request: "ar-1" } },
        { pane_id: "p2", terminal_id: "b", workspace_id: "w1", tab_id: "w1:t2" },
        { pane_id: "p3", terminal_id: "c", workspace_id: "w1", tab_id: "w1:t3", tokens: { request: "ar-2" } },
      ],
    };
    const rows = buildSidebar(snapshot, {}, 0, { p1: { name: "frank", avatar: "https://example.com/f.png" }, p3: { name: "Home" } });
    const byId = (kind: string, id: string) => rows.find(r => r.kind === kind && r.id === id)!;
    expect(byId("agent", "w1:t1")).toMatchObject({ face: { initial: "F", tint: 6, avatar: "https://example.com/f.png" }, request: "ar-1" });
    expect(byId("agent", "w1:t2").face).toEqual({ initial: "C", tint: 2 });
    expect(byId("agent", "w1:t2").request).toBeUndefined();
    // A card on a plain pinned tab and a request there add nothing: only AGENTS rows carry faces.
    expect(byId("pinned", "w1:t3").face).toBeUndefined();
    expect(byId("pinned", "w1:t3").request).toBeUndefined();
  });
});
