// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import type { Snapshot } from "./model";
import { attentionTransition, observeActiveAttention, observeAttention } from "./notify";

// Pure transition algorithm: pane identity, focus, parking, priority and time gates
// have distinct edge cases; no existing shell test owns notification decisions.
const snapshot = (status: string, hidden = false): Snapshot => ({
  tabs: [{ tab_id: "t", workspace_id: "w", number: 1, label: "Build", hidden }],
  panes: [{ pane_id: "p", terminal_id: "term", workspace_id: "w", tab_id: "t", agent_status: status, title: " compiler " }],
});
const observe = (before: Snapshot | undefined, after: Snapshot, selected: string | null = null, focused = true, lastSent: Record<string, number> = {}, now = 100_000, parked: string[] = []) =>
  observeAttention(before, after, selected, focused, new Set(parked), lastSent, now);

describe("Mac unseen-tab notification contract", () => {
  it.each([
    ["working", "blocked", "blocked", "needs you compiler"],
    ["unknown", "blocked", "blocked", "needs you compiler"],
    ["working", "done", "done", "finished compiler"],
  ])("posts one notification for %s → %s", (before, after, kind, body) => {
    expect(observe(snapshot(before), snapshot(after)).notifications).toEqual([{ tab: "t", kind, title: "Build", body }]);
  });
  it.each([["blocked", "blocked"], ["done", "done"], ["unknown", "done"], ["working", "request"]])("does not notify for %s → %s", (before, after) => {
    expect(observe(snapshot(before), snapshot(after)).notifications).toEqual([]);
  });
  it("does not notify on the first snapshot or for a new pane", () => {
    expect(observe(undefined, snapshot("blocked")).notifications).toEqual([]);
    expect(observe({ panes: [] }, snapshot("blocked")).notifications).toEqual([]);
  });
  it("suppresses the selected tab only while focused", () => {
    expect(observe(snapshot("working"), snapshot("blocked"), "t").notifications).toEqual([]);
    expect(observe(snapshot("working"), snapshot("blocked"), "t", false).notifications).toHaveLength(1);
    expect(observe(snapshot("working"), snapshot("blocked"), "t").attention).toBe(false);
    expect(observe(snapshot("working"), snapshot("blocked"), "t", false).attention).toBe(true);
  });
  it("includes hidden agents but excludes catalog-parked tabs", () => {
    expect(observe(snapshot("working", true), snapshot("blocked", true)).notifications).toHaveLength(1);
    const result = observe(snapshot("working"), snapshot("blocked"), null, true, {}, 100_000, ["t"]);
    expect(result.notifications).toEqual([]);
    expect(result.attention).toBe(false);
  });
  it("does not nag an already-notified tab within a minute", () => {
    expect(observe(snapshot("working"), snapshot("blocked"), null, true, { t: 50_000 }).notifications).toEqual([]);
    expect(observe(snapshot("working"), snapshot("blocked"), null, true, { t: 40_000 }).notifications).toHaveLength(1);
  });
  it("prioritizes blocked and coalesces multiple panes into one tab notification", () => {
    const before = snapshot("working"), after = snapshot("done");
    before.panes!.push({ ...before.panes![0], pane_id: "q" });
    after.panes!.push({ ...after.panes![0], pane_id: "q", agent_status: "blocked" });
    expect(observe(before, after).notifications).toEqual([{ tab: "t", kind: "blocked", title: "Build", body: "needs you compiler" }]);
  });
  it("uses agent fallback status and exact fallback title/body", () => {
    const before = snapshot("working"), after = snapshot("blocked");
    delete after.panes![0].agent_status;
    after.panes![0].title = "";
    delete after.tabs![0].label;
    after.agents = [{ pane_id: "p", terminal_id: "term", tab_id: "t", workspace_id: "w", agent: "claude", agent_status: "blocked" }];
    expect(observe(before, after).notifications).toEqual([{ tab: "t", kind: "blocked", title: "tab 1", body: "needs you" }]);
  });
});

// Regression: unchanged aggregate attention must not restart Windows' flash cycle.
describe("app-wide taskbar attention transitions", () => {
  it.each([
    [false, false, undefined], [false, true, "request"],
    [true, true, undefined], [true, false, "clear"],
  ])("%s → %s decides %s", (previous, next, expected) => {
    expect(attentionTransition(previous, next)).toBe(expected);
  });
  it("observes only the active machine, not a background blocked transition", () => {
    const snapshots = { studio: snapshot("working"), pc: snapshot("blocked") };
    const result = observeActiveAttention(snapshot("working"), snapshots, "studio", null, true, new Set(), {}, 100_000);
    expect(result.notifications).toEqual([]);
    expect(result.attention).toBe(false);
    expect(observeActiveAttention(snapshot("working"), snapshots, "pc", null, true, new Set(), {}, 100_000).notifications).toHaveLength(1);
  });
});

// Integration wiring: real App, observer, selection restoration and control dispatch;
// only native Tauri IPC/window/notification APIs are replaced at the host boundary.
const native = vi.hoisted(() => ({
  attention: vi.fn(async () => {}), invoke: vi.fn(),
  events: new Map<string, (event: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: native.invoke, Channel: class {} }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async (name: string, callback: (event: { payload: unknown }) => void) => { native.events.set(name, callback); return () => { native.events.delete(name); }; } }));
vi.mock("@tauri-apps/api/window", () => ({ UserAttentionType: { Informational: 2 }, getCurrentWindow: () => ({ requestUserAttention: native.attention, onFocusChanged: async () => () => {}, isFocused: () => new Promise<boolean>(() => {}) }) }));
vi.mock("@tauri-apps/plugin-notification", () => ({ isPermissionGranted: async () => true, requestPermission: async () => "granted", sendNotification: vi.fn() }));

it("never flashes when a focused machine switch restores its selected blocked tab", async () => {
  const { act, createElement } = await import("react");
  const { createRoot } = await import("react-dom/client");
  const { default: App } = await import("./App");
  const focus = vi.spyOn(document, "hasFocus").mockReturnValue(true);
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => stored.set(key, value), clear: () => stored.clear() });
  localStorage.setItem("herdr-shell.machine", "pc");
  let pcStatus = "working";
  const machineSnapshot = (name: string): Snapshot => ({ tabs: [{ tab_id: "t", workspace_id: "w", number: 1, focused: true }], panes: [{ pane_id: "p", terminal_id: "term", tab_id: "t", workspace_id: "w", agent_status: name === "pc" ? pcStatus : "working" }], workspaces: [{ workspace_id: "w", number: 1 }] });
  native.invoke.mockImplementation(async (cmd: string, args?: { machine?: string }) => {
    if (cmd === "machines_list") return [{ name: "pc", state: "up" }, { name: "studio", state: "up" }];
    if (cmd === "snapshot") return machineSnapshot(args!.machine!);
    if (cmd === "file_stat") return { exists: false };
    if (cmd === "file_read") throw new Error("missing catalog");
    if (cmd === "api_request") return [];
    return undefined;
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(createElement(App)));
    await act(async () => { native.events.get("ctl-machine")?.({ payload: { name: "studio" } }); });
    pcStatus = "blocked";
    await act(async () => native.events.get("herdr://snapshot")!({ payload: { machine: "pc", snapshot: machineSnapshot("pc") } }));
    native.attention.mockClear();
    await act(async () => { native.events.get("ctl-machine")?.({ payload: { name: "pc" } }); });
    expect(native.events.has("ctl-machine")).toBe(true);
    expect(native.attention).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount()); host.remove(); focus.mockRestore(); vi.unstubAllGlobals();
  }
});
