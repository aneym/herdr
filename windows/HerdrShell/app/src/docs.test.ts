import { describe, expect, it } from "vitest";
import { docItems, laneFor, parseCatalog, projectFolder } from "./docs";
import type { Snapshot } from "./model";
// Pure catalog parser/path resolution has missing, duplicate, renamed and URL
// edge cases. These synthetic tables own Mac parity; no existing coverage does.
// They detect label/workspace matching, lost override precedence and wrong paths,
// without adding any test-only production seam or copying private lane content.
const fixture = JSON.stringify({ version: 1, generated_at: "2026-10-06T00:00:00Z", lanes: [
  { tab: "tab-a", name: "[scoping] Example Project", label: "old label", kind: "project", goal: null, goal_area: null, mode: null, pin_stale: false, review_url: "https://review.example/", scope_url: "https://scope.example/?route=scoping%2Fexample", section: "demo", section_source: "fixture" },
  { tab: "tab-b", name: "Other Project", label: "same label", scope_url: null, review_url: null },
] });
const snapshot: Snapshot = { tabs: [
  { tab_id: "tab-a", workspace_id: "space-x", number: 1, label: "renamed" },
  { tab_id: "tab-b", workspace_id: "space-y", number: 1, label: "same label" },
  { tab_id: "tab-c", workspace_id: "space-x", number: 2, label: "old label" },
] };
describe("docs lane and folder parity", () => {
  it("joins only on tab ID and honors areas display names before lane names and labels", () => {
    const catalog = parseCatalog(fixture, JSON.stringify({ tabs: { "tab-b": { name: "Override Name", area: "area" } }, spaces: { "space-x": "area" } }));
    expect(laneFor("tab-a", snapshot, catalog)).toMatchObject({ name: "[scoping] Example Project", displayName: "Example Project", scopeURL: "https://scope.example/?route=scoping%2Fexample" });
    expect(projectFolder(laneFor("tab-b", snapshot, catalog))).toBe("~/.agent-rails/lanes/Override-Name");
    expect(laneFor("tab-c", snapshot, catalog)?.displayName).toBe("old label");
    expect(laneFor("tab-c", snapshot, catalog)?.scopeURL).toBeUndefined();
    expect(laneFor(null, snapshot, catalog)).toBeNull();
  });
  it("uses the last duplicate tab record and trims optional catalog strings", () => {
    const catalog = parseCatalog(JSON.stringify({ lanes: [null, { tab: " " }, { tab: "t", name: "first" }, { tab: " t ", label: " fallback ", scope_url: " " }] }));
    expect(laneFor("t", {}, catalog)).toMatchObject({ name: "fallback", displayName: "fallback", scopeURL: undefined });
    expect(parseCatalog("{}", "{}")).toEqual({ lanes: {}, names: {} });
    expect(() => parseCatalog("{bad")).toThrow();
  });
  it.each([
    ["https://scope.example/?route=scoping%2Fslug%2Fextra", "~/.agent-rails/scoping/slug"],
    ["https://scope.example/?route=scoping//slug", "~/.agent-rails/scoping/slug"],
    ["https://scope.example/?route=other/slug", "~/.agent-rails/lanes/Example-Project"],
    ["https://scope.example/?route=scoping/", "~/.agent-rails/lanes/Example-Project"],
    [undefined, "~/.agent-rails/lanes/Example-Project"],
  ])("resolves scope %s to %s", (scopeURL, expected) => {
    const lane = laneFor("tab-a", snapshot, parseCatalog(fixture))!;
    expect(projectFolder({ ...lane, scopeURL })).toBe(expected);
  });
  it("includes web links first and only markdown files that exist in the resolved folder", () => {
    const lane = laneFor("tab-a", snapshot, parseCatalog(fixture));
    expect(docItems(lane, new Set(["~/.agent-rails/scoping/example/RESUME.md", "~/.agent-rails/scoping/example/DECISIONS.md", "~/.agent-rails/lanes/Example-Project/BRIEF.md"]))).toEqual([
      { name: "Scope", kind: "web", url: "https://scope.example/?route=scoping%2Fexample" },
      { name: "Review", kind: "web", url: "https://review.example/" },
      { name: "RESUME", kind: "markdown", path: "~/.agent-rails/scoping/example/RESUME.md" },
      { name: "DECISIONS", kind: "markdown", path: "~/.agent-rails/scoping/example/DECISIONS.md" },
    ]);
    expect(projectFolder(laneFor("missing", {}, parseCatalog("{}")))).toBeNull();
    expect(docItems(null, new Set())).toEqual([]);
  });
});
