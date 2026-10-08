// @vitest-environment happy-dom
import { act, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { actionFor, controlKey } from "./keys";
import { runAction } from "./actions";
import type { ActionContext } from "./actions";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import { LaneSnapshot } from "./laneFiles";
vi.mock("./bridge", () => ({ bridge: { updateStatus: () => new Promise(() => {}), fileList: async () => [], fileRead: async () => ({ data_b64: "" }) }, fromBase64: () => new Uint8Array() }));
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
function context(selected: string | null, select: (id: string) => void): ActionContext {
  return { machine: "studio", snapshot, rows: buildSidebar(snapshot), selected, focused: null, api: async () => ({}), select, focus: noop, created: noop, rename: noop, switcher: noop, toggleSidebar: noop, error: error => { throw error; } };
}
function Shell() {
  const [selected, select] = useState<string | null>("a1");
  const navigation = useSidebarNavigation(snapshot, catalog, selected);
  const revealed = useSelectionReveal(selected);
  useEffect(() => {
    const key = (event: KeyboardEvent) => { const action = actionFor(event); if (action) void runAction(action, { ...context(selected, select), navigation }); };
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
  expect([...host.querySelectorAll("button")].find(b => b.textContent === "Needs You")?.getAttribute("aria-pressed")).toBe("true");
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
