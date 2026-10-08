// @vitest-environment happy-dom
// Integration regression: real TabView, Sidebar, PaneTerm and control handlers, with
// only Tauri IPC and browser geometry/clock supplied. Existing drag coverage does
// not traverse held control delivery into the rendered lift/chip/hover and cancel.
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { bridge } from "./bridge";
import type { Layout, Snapshot } from "./model";
import type { PaneDrag, PaneDragState } from "./paneDrag";
import { installControl } from "./control";
import TabView from "./TabView";
import Sidebar from "./Sidebar";

let root: Root, host: HTMLDivElement, stop: () => void;
let drag: PaneDrag | null = null;
const registerDrag = (value: PaneDrag | null) => { drag = value; };
const events = new Map<string, (payload: unknown) => void>();
const result = vi.fn();
const machine = { name: "studio", state: "up" as const };
const layout: Layout = { tab_id: "t", area: { x: 0, y: 0, width: 100, height: 40 }, panes: [
  { pane_id: "a", rect: { x: 0, y: 0, width: 50, height: 40 } },
  { pane_id: "b", rect: { x: 50, y: 0, width: 50, height: 40 } },
] };
const snapshot: Snapshot = { layouts: [layout], tabs: [{ tab_id: "target", workspace_id: "w", number: 2, label: "Notes" }], panes: layout.panes.map(pane => ({ pane_id: pane.pane_id, terminal_id: pane.pane_id, tab_id: "t", workspace_id: "w", title: "Brief", agent_status: "blocked" })) };
const rect = (left: number, top: number, width: number, height: number): DOMRect => ({ left, top, width, height, right: left + width, bottom: top + height, x: left, y: top, toJSON: () => ({}) });
function Surface() {
  const [hover, setHover] = useState(null as string | null);
  const changed = (state: PaneDragState) => setHover(state.zone?.kind === "into_tab" ? `tab:${state.zone.tab_id}` : null);
  return <><Sidebar machines={[machine]} chooseMachine={() => {}} rows={[{ kind: "pinned", id: "target", label: "Notes", status: "done", hotkey: null, section: "PINNED" }]} selected="t" revealed={{ last: "t", pending: null }} machine={machine} notice={null} select={() => {}} pin={() => {}} movePin={() => {}} renaming={null} startRename={() => {}} cancelRename={() => {}} commitRename={async () => {}} paneDropRow={hover} /><TabView snapshot={snapshot} selected="t" machine="studio" focused="a" onFocus={() => {}} shortcut={() => false} register={() => {}} pin={() => {}} registerDrag={registerDrag} onDragChange={changed} /></>;
}
beforeEach(async () => {
  vi.useFakeTimers(); events.clear(); result.mockReset(); drag = null;
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("localStorage", { getItem: (key: string) => key === "herdr-shell.areas.mode" ? '"spaces"' : null, setItem: () => {} });
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {}, addListener: () => {}, removeListener: () => {} }));
  vi.stubGlobal("requestAnimationFrame", () => 0); // happy-dom has no terminal renderer
  vi.stubGlobal("cancelAnimationFrame", (id: number) => clearTimeout(id));
  vi.stubGlobal("ResizeObserver", class { constructor(private fn: (entries: unknown[]) => void) {} observe() { this.fn([{ contentRect: rect(0, 0, 1000, 800) }]); } disconnect() {} });
  vi.spyOn(HTMLElement.prototype, "offsetWidth", "get").mockImplementation(function (this: HTMLElement) { return this.classList.contains("xterm-char-measure-element") ? 320 : 500; });
  vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (this: HTMLElement) { return this.classList.contains("xterm-char-measure-element") ? 20 : 800; });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) { return this.matches('[data-tab="target"]') ? rect(-200, 100, 180, 23) : this.classList.contains("pane-cap") ? rect(0, 0, 500, 36) : rect(0, 0, 1000, 800); });
  if (!Element.prototype.animate) Element.prototype.animate = () => ({}) as Animation;
  vi.spyOn(bridge, "api").mockResolvedValue({ place: { changed: true } });
  vi.spyOn(bridge, "controlEvent").mockImplementation(async (cmd, fn) => { events.set(cmd, fn as (payload: unknown) => void); return () => { events.delete(cmd); }; });
  vi.spyOn(bridge, "controlResult").mockImplementation(async (...args) => { result(...args); });
  vi.spyOn(bridge, "attach").mockResolvedValue(1);
  vi.spyOn(bridge, "resize").mockResolvedValue();
  vi.spyOn(bridge, "close").mockResolvedValue();
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  act(() => root.render(<Surface />));
  stop = installControl(() => ({ paneDrag: drag, machine, machines: [machine], chooseMachine: () => machine, selected: "t", docs: { open: false, items: [], active: null }, rows: [], panes: [], focused: undefined, open: () => {}, action: async () => {} }));
  await act(async () => { await vi.advanceTimersByTimeAsync(0); });
  document.elementFromPoint = (x) => x < 0 ? host.querySelector('[data-tab="target"]') : host.querySelector('[data-pane="a"] .pane-cap');
});
afterEach(() => { stop(); act(() => root.unmount()); host.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers(); });

it("renders the lifted stroke class, glyph before cap name and filled tab hover, then cancels a held control drag", async () => {
  await act(async () => { events.get("drag_pane")!({ pane_id: "a", to: { tab_id: "target" }, hold: true, steps: 1, interval_ms: 0 }); await vi.advanceTimersByTimeAsync(2); });
  expect(host.querySelector('[data-tab="target"]')).not.toBeNull();
  expect(drag?.state.phase).toBe("dragging");
  expect(host.querySelector('[data-pane="a"]')?.classList.contains("lifted")).toBe(true);
  const chip = host.querySelector(".pane-drag-chip")!;
  expect(chip.firstElementChild?.getAttribute("aria-label")).toBe("blocked");
  expect(chip.textContent).toBe("■Brief");
  const row = host.querySelector('[data-tab="target"]')!;
  expect(row.classList.contains("pane-drop-fill")).toBe(true);
  expect(row.classList.contains("drop-above")).toBe(false);
  // Native enqueue owns the hold acknowledgement; a later frontend result must
  // not satisfy another outstanding drag_pane request of the same command name.
  expect(result).not.toHaveBeenCalled();
  await act(async () => { events.get("drag_pane")!({ cancel: true }); await vi.advanceTimersByTimeAsync(200); });
  expect(drag?.state.phase).toBe("idle");
  expect(host.querySelector('[data-pane="a"]')?.classList.contains("lifted")).toBe(false);
  expect(result).toHaveBeenCalledWith("drag_pane", expect.objectContaining({ ok: true }));
  expect(vi.mocked(bridge.api).mock.calls.every(([, method, params]) => method !== "pane.place" || (params as { dry_run?: boolean })?.dry_run)).toBe(true);
});

it("does not send a late frontend completion for an enqueue-acknowledged motion freeze", async () => {
  const pause = vi.fn(), play = vi.fn();
  const animation = { pause, play, currentTime: 0 };
  document.getAnimations = () => [animation as unknown as Animation];
  await act(async () => { events.get("motion")!({ freeze_ms: 50 }); await vi.advanceTimersByTimeAsync(0); });
  expect(pause).toHaveBeenCalledOnce(); expect(animation.currentTime).toBe(50);
  expect(result).not.toHaveBeenCalled();
});
