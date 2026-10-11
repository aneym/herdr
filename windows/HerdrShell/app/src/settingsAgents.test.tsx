// @vitest-environment happy-dom
// Settings > Agents (Alex 2026-10-11: "we should be able to show/hide agents from settings"): the row
// contract over the shared fixture the Mac check reads, then the mounted page with the native API as
// the only mock. A wrong row hides the wrong agent on every client; a switch that flips back before
// the server answers reads as a failed toggle.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { parseCard } from "./faces";
import type { AgentCard } from "./faces";
import type { Snapshot } from "./model";
import { settingsAgentRows, settleSettings } from "./settingsAgents";
import { actionFor } from "./keys";
import raw from "../../../../shell/fixtures/settings-agents.json";
const api = vi.fn(async (..._args: unknown[]): Promise<unknown> => ({}));
vi.mock("./bridge", () => ({ bridge: { api: (...args: unknown[]) => api(...args) }, fromBase64: () => new Uint8Array() }));
const { default: Settings } = await import("./Settings");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

interface FixtureTab { id: string; space: string; label: string; pinIndex?: number; role?: string; hidden?: boolean; runsOn?: string }
interface Fixture {
  input: { spaces: { id: string; name: string }[]; tabs: FixtureTab[] };
  cards: { dir: string; agent: Record<string, string> }[]; panes: [string, string][];
  expected: { tab: string; title: string; meta: string; shown: boolean; face: string }[];
}
const fixture = raw as unknown as Fixture;
/** The fixture's SpacesInput as the herdr snapshot and agent cards the Windows client reads. */
function fromFixture(): { snapshot: Snapshot; cards: Record<string, AgentCard> } {
  const snapshot: Snapshot = {
    workspaces: fixture.input.spaces.map((s, i) => ({ workspace_id: s.id, number: i + 1, label: s.name })),
    tabs: fixture.input.tabs.map((t, i) => ({ tab_id: t.id, workspace_id: t.space, number: i + 1, label: t.label, pin_index: t.pinIndex, role: t.role, hidden: t.hidden, runs_on: t.runsOn })),
    panes: fixture.panes.map(([pane_id, tab_id]) => ({ pane_id, tab_id, terminal_id: pane_id, workspace_id: tab_id.split(":")[0] })),
  };
  const cards: Record<string, AgentCard> = {};
  for (const card of fixture.cards) { const parsed = parseCard(JSON.stringify(card.agent)); if (parsed) cards[parsed.pane] = parsed.card; }
  return { snapshot, cards };
}
const withHidden = (snapshot: Snapshot, tab: string, hidden: boolean): Snapshot => ({ ...snapshot, tabs: snapshot.tabs!.map(t => t.tab_id === tab ? { ...t, hidden } : t) });

describe("settings agents: rows", () => {
  it("lists agent pins in pin order with the Mac's titles, meta, faces and switch state", () => {
    const { snapshot, cards } = fromFixture();
    expect(settingsAgentRows(snapshot, cards).map(r => ({ tab: r.tab, title: r.title, meta: r.meta, shown: r.shown, face: `${r.face.initial}:${r.face.tint}` }))).toEqual(fixture.expected);
  });
  it("holds a pending toggle until the server echoes it", () => {
    const { snapshot, cards } = fromFixture();
    const hidden = fixture.expected.find(e => !e.shown)!.tab, shown = fixture.expected.find(e => e.shown)!.tab;
    const pending = { [hidden]: true, [shown]: false };
    const rows = settingsAgentRows(snapshot, cards, pending);
    expect([rows.find(r => r.tab === hidden)!.shown, rows.find(r => r.tab === shown)!.shown]).toEqual([true, false]);
    expect(Object.keys(settleSettings(pending, snapshot)).sort()).toEqual([hidden, shown].sort());
    expect(Object.keys(settleSettings(pending, withHidden(snapshot, hidden, false)))).toEqual([shown]);
  });
  it("opens with Ctrl+, as the Mac's Cmd+,", () => {
    expect(actionFor({ key: ",", ctrlKey: true, shiftKey: false, altKey: false, metaKey: false })).toBe("open_settings");
  });
});

describe("settings agents: mounted page", () => {
  let host: HTMLDivElement, root: Root;
  const close = vi.fn(), onError = vi.fn();
  const render = (snapshot: Snapshot, cards: Record<string, AgentCard>) => act(() => root.render(<Settings machine="studio" snapshot={snapshot} cards={cards} close={close} onError={onError} />));
  const switches = () => [...host.querySelectorAll<HTMLButtonElement>('[role="switch"]')].map(b => [b.closest("li")!.dataset.tab, b.getAttribute("aria-checked")]);
  beforeEach(() => { host = document.createElement("div"); document.body.append(host); root = createRoot(host); api.mockReset(); api.mockResolvedValue({}); close.mockReset(); onError.mockReset(); });
  afterEach(() => { act(() => root.unmount()); host.remove(); });

  it("hides and shows an agent through tab.set_hidden, keeping its row in place", async () => {
    const { snapshot, cards } = fromFixture();
    await render(snapshot, cards);
    expect(host.querySelector("h1")?.textContent).toBe("Agents");
    expect([...host.querySelectorAll(".settings-row-title")].map(e => e.textContent)).toEqual(fixture.expected.map(e => e.title));
    expect(switches()).toEqual(fixture.expected.map(e => [e.tab, String(e.shown)]));
    const target = fixture.expected.find(e => e.shown && e.title === "Factory")!.tab;
    await act(async () => host.querySelector<HTMLButtonElement>(`[data-tab="${target}"] [role="switch"]`)!.click());
    expect(api).toHaveBeenCalledWith("studio", "tab.set_hidden", { tab_id: target, hidden: true });
    // Off at once, and still off when the server's next snapshot agrees.
    expect(switches().find(([tab]) => tab === target)![1]).toBe("false");
    await render(withHidden(snapshot, target, true), cards);
    expect(switches().find(([tab]) => tab === target)![1]).toBe("false");
    expect(switches().map(([tab]) => tab)).toEqual(fixture.expected.map(e => e.tab));
    // A hide from another client or the CLI shows here with no click.
    const coach = fixture.expected.find(e => !e.shown)!.tab;
    await render(withHidden(withHidden(snapshot, target, true), coach, false), cards);
    expect(switches().find(([tab]) => tab === coach)![1]).toBe("true");
  });

  it("puts the switch back and reports when the server refuses", async () => {
    const { snapshot, cards } = fromFixture();
    api.mockRejectedValueOnce(new Error("herdr api error tab_not_agent: tab is not an agent pin"));
    await render(snapshot, cards);
    const target = fixture.expected[0].tab;
    await act(async () => host.querySelector<HTMLButtonElement>(`[data-tab="${target}"] [role="switch"]`)!.click());
    expect(switches().find(([tab]) => tab === target)![1]).toBe("true");
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("closes on Done and on Escape", async () => {
    const { snapshot, cards } = fromFixture();
    await render(snapshot, cards);
    await act(async () => host.querySelector<HTMLButtonElement>(".settings-done")!.click());
    await act(async () => { host.querySelector(".settings")!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
    expect(close).toHaveBeenCalledTimes(2);
  });
});
