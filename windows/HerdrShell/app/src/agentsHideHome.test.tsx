// @vitest-environment happy-dom
// Slice W scenario (spec agents-hide-and-home-glyph, 2026-10-07): hiding an agent and its home glyph,
// over the row model and the mounted Sidebar, with the native API boundary as the only mock.
// One story: agent pins A, B, C and plain pin P, with B hidden on the server. Digits, the Hidden
// fold, its quiet dot, the row menu and the glyphs are all user-visible; a wrong numbering opens the
// wrong chat on Ctrl+N, and a wrong tab.set_hidden hides the wrong agent on every client.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot, Tab } from "./model";
const api = vi.fn(async (..._args: unknown[]): Promise<unknown> => ({}));
vi.mock("./bridge", () => ({ bridge: { api: (...args: unknown[]) => api(...args), updateStatus: () => new Promise(() => {}), fileList: async () => [], fileRead: async () => ({ data_b64: "" }) }, fromBase64: () => new Uint8Array() }));
const { default: Sidebar, useSelectionReveal } = await import("./Sidebar");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// Node 26's own localStorage global is undefined without --localstorage-file and hides happy-dom's.
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => void stored.set(k, v), clear: () => stored.clear() } });

const CLOUD = "☁︎", LOCAL = "⌂︎", UNSYNCED = "⇡︎";
const TITLES = { cloud: "Memory in Rails cloud", local: "Memory on this machine only", unsynced: "Memory not synced to Rails cloud" } as const;
type Home = keyof typeof TITLES;
/** The story's snapshot; `edit` adjusts one tab, `panes` adds the agents' panes (requests ride on them). */
function story(edit: Partial<Record<"A" | "B" | "C" | "P", Partial<Tab>>> = {}, panes: Snapshot["panes"] = []): Snapshot {
  const tab = (tab_id: "A" | "B" | "C" | "P", number: number, base: Partial<Tab>): Tab => ({ tab_id, workspace_id: "w1", number, ...base, ...edit[tab_id] });
  return {
    workspaces: [{ workspace_id: "w1", number: 1, label: "home space" }],
    tabs: [
      tab("A", 1, { label: "alpha", role: "agent", pin_index: 0, home_location: "cloud" }),
      tab("B", 2, { label: "bravo", role: "agent", pin_index: 1, hidden: true, home_location: "unsynced" }),
      tab("C", 3, { label: "charlie", role: "agent", pin_index: 2, home_location: "local" }),
      tab("P", 4, { label: "plain", pin_index: 3 }),
    ],
    panes,
  };
}

describe("hide agents and home glyph: row model", () => {
  const pins = (snapshot: Snapshot) => buildSidebar(snapshot).filter(r => r.kind === "agent" || r.kind === "pinned").map(r => [r.kind, r.id, r.hotkey, !!r.hidden]);
  it("numbers visible agents then plain pins, skipping the hidden agent, which follows the visible ones", () => {
    expect(pins(story())).toEqual([["agent", "A", 1, false], ["agent", "C", 2, false], ["agent", "B", null, true], ["pinned", "P", 3, false]]);
    const rows = buildSidebar(story());
    // A hidden agent's chat stays out of its space, as a visible agent's does.
    expect(rows.filter(r => r.kind === "tab").map(r => r.id)).toEqual(["P"]);
    expect(rows.find(r => r.id === "B")?.home).toBe("unsynced");
    // Unhidden, B is back in its pin slot between A and C.
    expect(pins(story({ B: { hidden: false } }))).toEqual([["agent", "A", 1, false], ["agent", "B", 2, false], ["agent", "C", 3, false], ["pinned", "P", 4, false]]);
  });
  it("carries the home location on agent rows only, and none when the server sends none", () => {
    const rows = buildSidebar(story({ C: { home_location: undefined }, P: { home_location: "cloud" } }));
    expect(rows.filter(r => r.kind === "agent" || r.kind === "pinned").map(r => [r.id, r.home ?? null])).toEqual([["A", "cloud"], ["C", null], ["B", "unsynced"], ["P", null]]);
  });
});

describe("hide agents and home glyph: rendered sidebar", () => {
  let host: HTMLDivElement, root: Root;
  const select = vi.fn();
  function Shell({ snapshot }: { snapshot: Snapshot }) {
    const revealed = useSelectionReveal(null);
    const noop = () => {};
    return <Sidebar machines={[]} chooseMachine={noop} rows={buildSidebar(snapshot)} selected={null} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={null} select={select} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />;
  }
  const mount = (snapshot: Snapshot = story()) => act(() => root.render(<Shell snapshot={snapshot} />));
  const remount = (snapshot: Snapshot = story()) => { act(() => root.unmount()); root = createRoot(host); mount(snapshot); };
  beforeEach(() => { stored.clear(); api.mockReset(); api.mockResolvedValue({}); select.mockReset(); host = document.createElement("div"); document.body.append(host); root = createRoot(host); });
  afterEach(() => { act(() => root.unmount()); host.remove(); document.body.innerHTML = ""; });
  const row = (id: string) => host.querySelector<HTMLElement>(`[data-row="${id}"]`);
  const header = () => row("hiddenagents");
  const toggleHeader = () => act(() => { header()!.click(); });
  /** Section headings and rows above the spaces, in drawn order; space children are left out. */
  const shape = () => [...host.querySelectorAll<HTMLElement>("nav h2, nav [data-row]")].map(el => el.tagName === "H2" ? el.textContent : el.dataset.row).filter(v => !v?.startsWith("tab:"));
  const menuItems = () => [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
  const openMenu = (id: string) => act(() => { row(id)!.querySelector(".select-tab")!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 40, clientY: 40 })); });
  const choose = async (label: string) => { const item = menuItems().find(b => b.textContent?.trim() === label); expect(item, label).toBeDefined(); await act(async () => { item!.click(); }); };
  const glyph = (id: string) => row(id)?.querySelector<HTMLElement>(".home-glyph") ?? null;

  it("draws visible agents, a shut Hidden header with its count, then PINNED", () => {
    mount();
    expect(shape()).toEqual(["AGENTS", "agent:A", "agent:C", "hiddenagents", "PINNED", "pinned:P"]);
    expect(header()!.getAttribute("aria-expanded")).toBe("false");
    expect(header()!.textContent).toMatch(/Hidden\s*1/);
    expect(row("agent:B")).toBeNull();
  });
  it("opens the fold to show the hidden agent without a drag handle, and keeps the fold across a remount", () => {
    mount();
    toggleHeader();
    expect(header()!.getAttribute("aria-expanded")).toBe("true");
    expect(shape()).toEqual(["AGENTS", "agent:A", "agent:C", "hiddenagents", "agent:B", "PINNED", "pinned:P"]);
    expect(row("agent:B")!.hasAttribute("data-pin-section")).toBe(false);
    expect(row("agent:A")!.getAttribute("data-pin-section")).toBe("agent");
    // Clicking a hidden row still opens its chat.
    act(() => { row("agent:B")!.querySelector<HTMLButtonElement>(".select-tab")!.click(); });
    expect(select).toHaveBeenCalledWith("B");
    // The fold is client presentation, saved through the sidebar's own store.
    expect(stored.get("herdr-shell.areas.hiddenAgents")).toBe("true");
    remount();
    expect(header()!.getAttribute("aria-expanded")).toBe("true");
    toggleHeader();
    expect(stored.get("herdr-shell.areas.hiddenAgents")).toBe("false");
    expect(row("agent:B")).toBeNull();
  });
  it("shows no Hidden header without a hidden agent, and keeps AGENTS when every agent is hidden", () => {
    mount(story({ B: { hidden: false } }));
    expect(header()).toBeNull();
    expect(shape()).toEqual(["AGENTS", "agent:A", "agent:B", "agent:C", "PINNED", "pinned:P"]);
    remount(story({ A: { hidden: true }, C: { hidden: true } }));
    expect(shape()).toEqual(["AGENTS", "hiddenagents", "PINNED", "pinned:P"]);
    expect(header()!.textContent).toMatch(/Hidden\s*3/);
  });
  it.each([
    ["blocked", story({ B: { work_status: "blocked" } }), true],
    ["an open request", story({}, [{ pane_id: "pB", terminal_id: "tB", workspace_id: "w1", tab_id: "B", tokens: { request: "R-7" } }]), true],
    ["working", story({ B: { work_status: "working" } }), false],
    ["done", story({ B: { work_status: "done" } }), false],
  ] as const)("puts one quiet accent dot on the shut header for a hidden agent that is %s: %s", (_name, snapshot, dot) => {
    mount(snapshot);
    const marks = header()!.querySelectorAll('[data-dot="accent"]');
    expect(marks.length).toBe(dot ? 1 : 0);
    // The count stays plain text: the dot carries no number.
    if (dot) expect(marks[0].textContent).toBe("");
    // While open, the hidden row shows its own state; the header carries no dot.
    toggleHeader();
    expect(header()!.querySelectorAll('[data-dot="accent"]').length).toBe(0);
  });
  it("offers Hide on a visible agent and Show in Agents on a hidden one, sending tab.set_hidden", async () => {
    mount();
    openMenu("agent:A");
    expect(menuItems().map(b => b.textContent?.trim())).toContain("Hide");
    expect(menuItems().map(b => b.textContent?.trim())).not.toContain("Show in Agents");
    await choose("Hide");
    expect(api.mock.calls).toEqual([["studio", "tab.set_hidden", { tab_id: "A", hidden: true }]]);
    expect(menuItems()).toEqual([]);
    toggleHeader();
    openMenu("agent:B");
    expect(menuItems().map(b => b.textContent?.trim())).toContain("Show in Agents");
    expect(menuItems().map(b => b.textContent?.trim())).not.toContain("Hide");
    await choose("Show in Agents");
    expect(api.mock.calls[1]).toEqual(["studio", "tab.set_hidden", { tab_id: "B", hidden: false }]);
    // A plain pin is not an agent: no Hide.
    openMenu("pinned:P");
    expect(menuItems().map(b => b.textContent?.trim())).not.toContain("Hide");
  });
  it("ignores an older server's unknown_method answer to tab.set_hidden", async () => {
    api.mockRejectedValueOnce(Object.assign(new Error("unknown method: tab.set_hidden"), { code: "unknown_method" }));
    mount();
    openMenu("agent:A");
    await choose("Hide");
    expect(api).toHaveBeenCalledWith("studio", "tab.set_hidden", { tab_id: "A", hidden: true });
    expect(host.querySelector("footer")!.textContent).toBe("studio · connected");
  });
  it("draws the home glyph with its title on agent rows in place of the space name, which stays on PINNED rows", () => {
    mount();
    toggleHeader();
    const expected: [string, string, Home][] = [["agent:A", CLOUD, "cloud"], ["agent:C", LOCAL, "local"], ["agent:B", UNSYNCED, "unsynced"]];
    for (const [id, text, home] of expected) {
      const mark = glyph(id);
      expect(mark, id).not.toBeNull();
      expect(mark!.textContent).toBe(text);
      expect(mark!.getAttribute("title")).toBe(TITLES[home]);
      expect(mark!.getAttribute("aria-label")).toBe(TITLES[home]);
      // Unsynced reads as a quiet warning (warn token); cloud and local stay muted.
      expect(mark!.classList.contains("warn"), id).toBe(home === "unsynced");
      expect(row(id)!.querySelector(".space-label"), id).toBeNull();
      expect(row(id)!.textContent).not.toContain("home space");
    }
    expect(row("pinned:P")!.querySelector(".space-label")?.textContent).toBe("home space");
    expect(glyph("pinned:P")).toBeNull();
  });
  it("draws neither glyph nor space name on an agent the server gives no home location", () => {
    mount(story({ C: { home_location: undefined } }));
    expect(glyph("agent:C")).toBeNull();
    expect(row("agent:C")!.querySelector(".space-label")).toBeNull();
    expect(row("agent:C")!.textContent).not.toContain("home space");
    expect(glyph("agent:A")!.textContent).toBe(CLOUD);
  });
});
