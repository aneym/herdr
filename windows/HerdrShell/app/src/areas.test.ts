import { describe, expect, it } from "vitest";
import { buildAreas, focusTabs } from "./areas";
import type { AreaOptions } from "./areas";
import { LaneSnapshot, parseAreas, parseLanes, parseModes } from "./laneFiles";
import type { Snapshot } from "./model";
// P15 scenario inputs with stable IDs replacing UUIDs. This pure ordering/filter
// algorithm has interacting edge cases; literal golden lines guard cross-client parity.
const lanesFile = {
  "version": 1,
  "generated_at": "2026-10-01T00:00:00Z",
  "lanes": [
    {
      "tab": "orch",
      "name": "orchestrator",
      "label": "orchestrator",
      "kind": "orchestrator",
      "goal": null,
      "goal_area": null,
      "section": "orchestrator",
      "section_source": "explicit",
      "scope_url": null,
      "review_url": null,
      "mode": null
    },
    {
      "tab": "blocked",
      "name": "blocked build",
      "label": "blocked build",
      "kind": "lane",
      "goal": null,
      "goal_area": "factory infra",
      "section": "implementing",
      "section_source": "project",
      "scope_url": null,
      "review_url": null,
      "mode": null
    },
    {
      "tab": "scope_f",
      "name": "scope factory",
      "label": "scope factory",
      "kind": "lane",
      "goal": null,
      "goal_area": "factory infra",
      "section": "scoping",
      "section_source": "project",
      "scope_url": null,
      "review_url": null,
      "mode": null
    },
    {
      "tab": "scope_r",
      "name": "[scoping] raise brief",
      "label": "raise brief raw",
      "kind": "lane",
      "goal": "raise",
      "goal_area": null,
      "section": "scoping",
      "section_source": "project",
      "scope_url": null,
      "review_url": null,
      "mode": null
    },
    {
      "tab": "review",
      "name": "review packet",
      "label": "review packet",
      "kind": "lane",
      "goal": "raise",
      "goal_area": null,
      "section": "reviewing",
      "section_source": "project",
      "scope_url": null,
      "review_url": null,
      "mode": null
    },
    {
      "tab": "build",
      "name": "raise build",
      "label": "raise build",
      "kind": "lane",
      "goal": "raise",
      "goal_area": null,
      "section": "implementing",
      "section_source": "project",
      "scope_url": null,
      "review_url": null,
      "mode": null
    },
    {
      "tab": "wf",
      "name": "wf raise-1",
      "label": "wf raise-1",
      "kind": "workflow",
      "goal": null,
      "goal_area": null,
      "section": null,
      "section_source": "default",
      "scope_url": null,
      "review_url": null,
      "mode": null
    },
    {
      "tab": "closed",
      "name": "old raise",
      "label": "old raise",
      "kind": "lane",
      "goal": "raise",
      "goal_area": null,
      "section": "closed",
      "section_source": "project",
      "scope_url": null,
      "review_url": null,
      "mode": null
    },
    {
      "tab": "desk",
      "name": "recruiting desk",
      "label": "recruiting desk raw",
      "kind": "lane",
      "goal": null,
      "goal_area": null,
      "section": "implementing",
      "section_source": "default",
      "scope_url": null,
      "review_url": null,
      "mode": null
    },
    {
      "tab": "job1",
      "name": "job outreach",
      "label": "job outreach raw",
      "kind": "lane",
      "goal": null,
      "goal_area": null,
      "section": "implementing",
      "section_source": "default",
      "scope_url": null,
      "review_url": null,
      "mode": null
    },
    {
      "tab": "job2",
      "name": "job followups",
      "label": "job followups",
      "kind": "lane",
      "goal": null,
      "goal_area": null,
      "section": "implementing",
      "section_source": "default",
      "scope_url": null,
      "review_url": null,
      "mode": null
    }
  ]
};
const areasFile = {
  "version": 1,
  "areas": [
    {
      "id": "factory",
      "name": "factory",
      "color": "#5AA9FF"
    },
    {
      "id": "raise",
      "name": "raise",
      "color": "#B5476B"
    },
    {
      "id": "recruiter",
      "name": "recruiter",
      "color": "#4F5BD5"
    },
    {
      "id": "empty",
      "name": "empty",
      "color": "#888888"
    },
    {
      "id": "unsorted",
      "name": "unsorted",
      "color": "#999999"
    }
  ],
  "tabs": {
    "orch": {
      "area": "factory",
      "role": "top",
      "name": "rails orchestrator"
    },
    "desk": {
      "area": "recruiter",
      "role": "desk",
      "name": "recruiting desk"
    },
    "job1": {
      "area": "recruiter",
      "role": "job",
      "name": "job outreach"
    },
    "job2": {
      "area": "recruiter",
      "role": "job"
    }
  },
  "spaces": {
    "fw": "factory",
    "rw": "raise"
  },
  "goal_area": {
    "factory infra": "factory"
  },
  "goal": {
    "raise": "raise"
  }
};
const catalog = Object.assign(new LaneSnapshot(), parseLanes(lanesFile), parseAreas(areasFile), { hasFiles: true });
const fixture = [
  ["orch", "fw", 1, "rails orchestrator", "working", "orchestrator"],
  ["blocked", "fw", 2, "blocked build", "blocked", "lane"],
  ["scope_f", "fw", 3, "scope factory", "working", "lane"],
  ["stray", "fw", 4, "stray tab", "idle", "lane"],
  ["scope_r", "rw", 1, "raise brief raw", "working", "lane"],
  ["review", "rw", 2, "review packet", "working", "lane"],
  ["build", "rw", 3, "raise build", "working", "lane"],
  ["wf", "rw", 4, "wf raise-1", "working", "workflow"],
  ["closed", "rw", 5, "old raise", "idle", "lane"],
  ["desk", "rw", 6, "recruiting desk raw", "idle", "lane"],
  ["job1", "rw", 7, "job outreach raw", "working", "lane"],
  ["job2", "rw", 8, "job followups", "idle", "lane"],
] as const;
const snapshot: Snapshot = {
  workspaces: [{ workspace_id: "fw", number: 1 }, { workspace_id: "rw", number: 2 }],
  tabs: fixture.map(([tab_id, workspace_id, number, label, agent_status]) => ({ tab_id, workspace_id, number, label, agent_status })),
  panes: fixture.map(([tab_id, workspace_id]) => ({ pane_id: `${tab_id}-pane`, terminal_id: tab_id, tab_id, workspace_id })),
  agents: fixture.map(([tab_id, workspace_id, , , agent_status, kind]) => ({ terminal_id: tab_id, pane_id: `${tab_id}-pane`, tab_id, workspace_id, agent: "claude", agent_status, tokens: { kind }, ...(tab_id === "wf" ? { ownership: { current: { pane_id: "build-pane" } } } : {}) })),
};
type Golden = [string, string, string, string, number][];
const collapsedFocus: Golden = [
  ["focus", "focus", "Focus", "4", 0],
  ["focus:blocked", "lane", "blocked build", "next · factory", 1],
];
const factory: Golden = [
  ["area:factory", "area", "factory", "4", 0],
  ["sub:factory:ORCHESTRATOR", "header", "ORCHESTRATOR", "", 0],
  ["tab:orch", "orchestrator", "rails orchestrator", "", 1],
  ["sub:factory:PROJECTS", "header", "PROJECTS", "", 0],
  ["tab:blocked", "lane", "blocked build", "", 1],
  ["tab:scope_f", "lane", "scope factory", "", 1],
  ["tab:stray", "lane", "stray tab", "", 1],
];
const raise: Golden = [
  ["area:raise", "area", "raise", "4", 0],
  ["sub:raise:PROJECTS", "header", "PROJECTS", "", 0],
  ["tab:scope_r", "lane", "raise brief", "", 1],
  ["tab:review", "lane", "review packet", "", 1],
  ["tab:build", "lane", "raise build", "", 1],
  ["tab:closed", "lane", "old raise", "", 1],
];
const recruiter: Golden = [
  ["area:recruiter", "area", "recruiter", "3", 0],
  ["sub:recruiter:USE", "header", "USE", "", 0],
  ["tab:desk", "lane", "recruiting desk", "", 1],
  ["tab:job1", "lane", "job outreach", "", 1],
  ["tab:job2", "lane", "job followups", "", 1],
];
const cases: [string, AreaOptions, Golden][] = [
  ["all / Focus collapsed", {}, [...collapsedFocus, ...factory, ...raise, ...recruiter]],
  ["needs", { chip: "needs" }, [...collapsedFocus,
    ["area:factory", "area", "factory", "2", 0],
    ["sub:factory:PROJECTS", "header", "PROJECTS", "", 0],
    ["tab:blocked", "lane", "blocked build", "", 1],
    ["tab:scope_f", "lane", "scope factory", "", 1],
    ["area:raise", "area", "raise", "2", 0],
    ["sub:raise:PROJECTS", "header", "PROJECTS", "", 0],
    ["tab:scope_r", "lane", "raise brief", "", 1],
    ["tab:review", "lane", "review packet", "", 1],
  ]],
  ["folded factory", { folded: new Set(["factory"]) }, [...collapsedFocus, ["area:factory", "area", "factory", "4", 0], ...raise, ...recruiter]],
  ["Focus expanded", { focusExpanded: true, focusCursor: 3 }, [
    ["focus", "focus", "Focus", "3 of 4", 0],
    ["focus:blocked", "lane", "blocked build", "", 1],
    ["focus:review", "lane", "review packet", "", 1],
    ["focus:scope_f", "lane", "scope factory", "", 1],
    ["focus:scope_r", "lane", "raise brief", "", 1],
    ...factory, ...raise, ...recruiter,
  ]],
  ["empty parked", { chip: "parked" }, []],
];
const lines = (c: LaneSnapshot, opts: AreaOptions) => buildAreas(snapshot, c, opts).map(l => [l.id, l.kind, l.title, l.trailing, l.depth]);
describe("Mac P15 area-line golden contract", () => {
  it.each(cases)("%s", (_name, opts, expected) => { expect(lines(catalog, opts)).toEqual(expected); });
  it("pulls parked workflows out of live owners and orders newest first", () => {
    const c = Object.assign(new LaneSnapshot(), catalog, { parked: parseModes({ tabs: {
      wf: { mode: "parked", at: "2026-10-02T12:00:00Z" },
      closed: { mode: "parked", at: "2026-10-03T12:00:00Z" },
    } }) });
    expect(lines(c, { chip: "parked" })).toEqual([
      ["parked:closed", "lane", "old raise", "", 0],
      ["parked:wf", "workflow", "wf raise-1", "", 0],
    ]);
    expect(focusTabs(snapshot, c)).toEqual(["blocked", "review", "scope_f", "scope_r"]);
    expect(lines(c, { areaOnly: "raise" })).toEqual([...collapsedFocus,
      ["area:raise", "area", "raise", "3", 0],
      ["sub:raise:PROJECTS", "header", "PROJECTS", "", 0],
      ["tab:scope_r", "lane", "raise brief", "", 1],
      ["tab:review", "lane", "review packet", "", 1],
      ["tab:build", "lane", "raise build", "", 1],
      ["parked", "parked", "Parked", "2", 0],
    ]);
  });
});
