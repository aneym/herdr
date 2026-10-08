// @vitest-environment happy-dom
import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { actionFor, controlKey } from "./keys";
import { runAction, trackAttention, latestAttentionTab } from "./actions";
import type { ActionContext } from "./actions";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import { LaneSnapshot } from "./laneFiles";
let publishSnapshot: ((value: { machine: string; snapshot: Snapshot }) => void) | undefined;
vi.mock("./bridge", () => ({ bridge: {
  machines: async () => [{ name: "studio", state: "up" }], snapshot: async () => snapshot,
  machineEvents: async () => () => {}, snapshots: async (publish: typeof publishSnapshot) => { publishSnapshot = publish; return () => {}; },
  controlEvent: async () => () => {}, api: async () => ({}), updateStatus: () => new Promise(() => {}), fileList: async () => [], fileRead: async () => ({ data_b64: "" }) }, fromBase64: () => new Uint8Array() }));
import Sidebar, { useSelectionReveal, useSidebarNavigation } from "./Sidebar";
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => void stored.set(k, v), clear: () => stored.clear() } });
const chords = [
  ["alt+]", "next_pane"], ["alt+[", "prev_pane"], ["ctrl+alt+a", "toggle_area_mode"],
  ...Array.from({ length: 6 }, (_, i) => [`ctrl+alt+${i + 1}`, `filter_${i + 1}`]),
  ...Array.from({ length: 9 }, (_, i) => [`ctrl+shift+${i + 1}`, `goto_space_${i + 1}`]),
  ["ctrl+alt+j", "agent_list_up"], ["ctrl+alt+k", "agent_list_down"], ["ctrl+shift+o", "attention_jump"], ["ctrl+shift+k", "search"], ["ctrl+shift+g", "goto"],
  ["ctrl+alt+m", "move_pane_mode"], ["ctrl+tab", "next_tab"], ["ctrl+shift+tab", "prev_tab"], ["ctrl+b", "toggle_sidebar"],
  ...Array.from({ length: 9 }, (_, i) => [`ctrl+${i + 1}`, `select_tab_${i + 1}`]),
  ["ctrl+shift+[", "prev_machine"], ["ctrl+shift+]", "next_machine"], ["ctrl+shift+t", "new_tab"], ["ctrl+shift+w", "close_pane"], ["ctrl+shift+z", "zoom_pane"], ["ctrl+shift+p", "switcher"], ["ctrl+shift+a", "next_attention"], ["ctrl+shift+d", "toggle_docs"],
  ["alt+shift+=", "split_right"], ["alt+shift+-", "split_down"], ["alt+arrowleft", "focus_pane_left"], ["alt+arrowright", "focus_pane_right"], ["alt+arrowup", "focus_pane_up"], ["alt+arrowdown", "focus_pane_down"], ["f2", "rename_tab"],
];
// Golden chord table exercises browser and control-pipe input, including shifted glyphs.
it.each(chords)("%s maps to %s", (chord, expected) => {
  const event = controlKey(chord);
  expect(actionFor(event)).toBe(expected);
  const shifted = event.shiftKey && /^Digit/.test(event.code) ? "!@#$%^&*("[Number(event.code.slice(5)) - 1] : event.code === "BracketLeft" && event.shiftKey ? "{" : event.code === "BracketRight" && event.shiftKey ? "}" : event.key;
  expect(actionFor({ key: shifted, code: event.code, ctrlKey: event.ctrlKey, shiftKey: event.shiftKey, altKey: event.altKey, metaKey: false })).toBe(expected);
});
const snapshot: Snapshot = {
  workspaces: [{ workspace_id: "hidden", number: 0, tokens: { hidden: "true" } }, { workspace_id: "a", number: 1, label: "Alpha" }, { workspace_id: "b", number: 2, label: "Beta", active_tab_id: "b2" }],
  tabs: [{ tab_id: "a1", workspace_id: "a", number: 1, label: "First", role: "agent", agent_status: "blocked" }, { tab_id: "b1", workspace_id: "b", number: 1, label: "Second", role: "agent", agent_status: "blocked" }, { tab_id: "b2", workspace_id: "b", number: 2, label: "Active" }],
};
const catalog = new LaneSnapshot();
const noop = () => {};
function context(selected: string | null, select: (id: string, stepping?: boolean) => void): ActionContext {
  return { machine: "studio", snapshot, rows: buildSidebar(snapshot), selected, focused: null, api: async () => ({}), select, focus: noop, created: noop, rename: noop, switcher: noop, toggleSidebar: noop, error: error => { throw error; } };
}
function Shell() {
  const [selected, select] = useState<string | null>("a1");
  const navigation = useSidebarNavigation(snapshot, catalog, selected);
  const revealed = useSelectionReveal(selected);
  useEffect(() => {
    const key = (event: KeyboardEvent) => { const action = actionFor(event); if (action) void runAction(action, { ...context(selected, (id, stepping) => { navigation.noteSelection?.(!!stepping); select(id); }), navigation }); };
    document.addEventListener("keydown", key); return () => document.removeEventListener("keydown", key);
  }, [selected, navigation]);
  return <><output aria-label="Selection">{selected}</output><Sidebar navigation={navigation} snapshot={snapshot} catalog={catalog} machines={[]} chooseMachine={noop} rows={buildSidebar(snapshot)} selected={selected} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={null} select={select} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} /></>;
}
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; stored.clear(); });
it("keyboard switches Areas, filters Needs You, steps Focus and selects the second visible space target", async () => {
  const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
  dispose = () => { act(() => root.unmount()); host.remove(); };
  act(() => root.render(<Shell />));
  const press = async (chord: string) => { await act(async () => { document.dispatchEvent(controlKey(chord)); }); };
  await press("ctrl+alt+a");
  expect([...host.querySelectorAll("button")].find(b => b.textContent === "Areas")?.getAttribute("aria-pressed")).toBe("true");
  expect(stored.get("herdr-shell.areas.mode")).toBe('"areas"');
  await press("ctrl+alt+2");
  expect(stored.get("herdr-shell.areas.chip")).toBe('"needs"');
  expect(host.querySelector('[data-row="focus"]')?.classList.contains("selected")).toBe(true);
  await press("alt+]");
  expect(host.textContent).toContain("2 of 2");
  expect(host.querySelector("output")?.textContent).toBe("b1");
  await press("ctrl+shift+2");
  expect(host.querySelector("output")?.textContent).toBe("b2");
});
// Pure navigation algorithms: wrapping and fallback are lane-required edge cases.
it("agent_list_down wraps through visible agents", async () => {
  const selected: string[] = []; await runAction("agent_list_down", context("b1", id => selected.push(id)));
  expect(selected).toEqual(["a1"]);
});
it("attention_jump falls back to next_attention without a transition", async () => {
  const selected: string[] = []; await runAction("attention_jump", context("a1", id => selected.push(id)));
  expect(selected).toEqual(["b1"]);
});
it("Focus cursor resets on mode, chip and ordinary selection, but survives stepping", async () => {
  const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
  dispose = () => { act(() => root.unmount()); host.remove(); };
  act(() => root.render(<Shell />));
  const press = async (chord: string) => { await act(async () => { document.dispatchEvent(controlKey(chord)); }); };
  await press("ctrl+alt+a"); await press("alt+]");
  expect(host.textContent).toContain("2 of 2");
  await press("ctrl+alt+2"); expect(host.textContent).not.toContain("of 2");
  await press("alt+["); expect(host.textContent).toContain("1 of 2");
  await press("ctrl+alt+a"); await press("ctrl+alt+a"); expect(host.textContent).not.toContain("of 2");
  await press("alt+]"); expect(host.textContent).toContain("2 of 2");
  await press("ctrl+shift+1"); expect(host.textContent).not.toContain("of 2");
  expect([...host.querySelectorAll(".areas-chips button")].map(b => b.textContent)).toEqual(["All", "Scope", "Build", "Review", "Use", "Parked"]);
});
// Golden class transitions cover idle->done, blocked->done, baseline and hidden trail entries.
it("attention trail captures class changes and skips hidden agents at read time", () => {
  const frame = (a: string, b: string): Snapshot => ({ ...snapshot, panes: [{ pane_id: "p1", terminal_id: "t1", workspace_id: "a", tab_id: "a1", agent_status: a }, { pane_id: "p2", terminal_id: "t2", workspace_id: "b", tab_id: "b1", agent_status: b }] });
  let trail = trackAttention(undefined, frame("idle", "idle"), []);
  expect(trail).toEqual([]);
  trail = trackAttention(frame("idle", "idle"), frame("blocked", "idle"), trail);
  trail = trackAttention(frame("blocked", "idle"), frame("blocked", "done"), trail);
  expect(trail).toEqual(["b1", "a1"]);
  expect(latestAttentionTab(trail, { ...snapshot, tabs: snapshot.tabs?.map(t => ({ ...t, hidden: t.tab_id === "b1" })) })).toBe("a1");
  trail = trackAttention(frame("blocked", "done"), frame("done", "done"), trail);
  expect(trail[0]).toBe("a1");
  expect(trackAttention(frame("idle", "idle"), frame("done", "done"), Array(20).fill("old"))).toHaveLength(20);
});
it("search and goto re-present the open app Switcher and select the query only for goto", async () => {
  const { default: App } = await import("./App");
  const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
  dispose = () => { act(() => root.unmount()); host.remove(); };
  await act(async () => { root.render(<App />); });
  const press = async (chord: string) => { await act(async () => { (host.querySelector("input") ?? window).dispatchEvent(controlKey(chord)); }); };
  await press("ctrl+shift+k");
  const input = host.querySelector<HTMLInputElement>('[aria-label="Filter tabs"]')!;
  expect(input).not.toBeNull();
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "First");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  input.setSelectionRange(5, 5);
  await press("ctrl+shift+g");
  expect([input.selectionStart, input.selectionEnd]).toEqual([0, 5]);
  await press("ctrl+shift+k");
  expect([input.selectionStart, input.selectionEnd]).toEqual([5, 5]);
  await press("ctrl+shift+g");
  expect([input.selectionStart, input.selectionEnd]).toEqual([0, 5]);
  expect(host.querySelectorAll('[aria-label="Switch tab"]')).toHaveLength(1);
});
it("App attention jump uses idle-to-done history and skips the newest hidden agent", async () => {
  const { default: App } = await import("./App");
  const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
  dispose = () => { act(() => root.unmount()); host.remove(); };
  await act(async () => { root.render(<App />); });
  const frame = (a: string, b: string, hidden = false): Snapshot => ({ ...snapshot,
    tabs: snapshot.tabs?.map(t => ({ ...t, hidden: hidden && t.tab_id === "b1" })),
    panes: [{ pane_id: "p1", terminal_id: "t1", workspace_id: "a", tab_id: "a1", agent_status: a }, { pane_id: "p2", terminal_id: "t2", workspace_id: "b", tab_id: "b1", agent_status: b }],
  });
  // Keep real terminal rendering out of this path by viewing the pane-less target tab.
  const press = async (chord: string) => { await act(async () => { window.dispatchEvent(controlKey(chord)); }); };
  await press("ctrl+shift+2");
  for (const value of [frame("idle", "idle"), frame("done", "idle"), frame("done", "done")]) {
    await act(async () => { publishSnapshot?.({ machine: "studio", snapshot: value }); });
  }
  await press("ctrl+shift+o");
  expect(host.querySelector('[data-row="agent:b1"]')?.classList.contains("selected")).toBe(true);
  await act(async () => { publishSnapshot?.({ machine: "studio", snapshot: frame("done", "done", true) }); });
  await press("ctrl+shift+o");
  expect(host.querySelector('[data-row="agent:a1"]')?.classList.contains("selected")).toBe(true);
});
