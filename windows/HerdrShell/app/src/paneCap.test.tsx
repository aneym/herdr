// @vitest-environment happy-dom
// Integration: real pane, cards and transcript; only native IPC/browser geometry supplied.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import PaneSurface from "./PaneSurface";
import App from "./App";
import { AGENTS_DIR } from "./faces";
import type { Snapshot } from "./model";
import { bridge, toBase64 } from "./bridge";
import type { Pane } from "./model";
let root: Root, host: HTMLDivElement;
const pane: Pane = { pane_id: "p", terminal_id: "term", tab_id: "t", workspace_id: "w", agent: "claude", agent_status: "blocked" };
const transcript = JSON.stringify({ type: "assistant", uuid: "a", message: { content: [{ type: "tool_use", id: "edit", name: "Edit", input: { file_path: "/repo/a", old_string: "old", new_string: "new" } }, { type: "tool_use", id: "bash", name: "Bash", input: { command: "pwd" } }] } }) + "\n";
const encode = (text: string) => toBase64(new TextEncoder().encode(text));
const register = () => {};
const render = (agent = true) => root.render(<PaneSurface pane={agent ? pane : { ...pane, agent: undefined, agent_status: undefined }} machine="studio" focused hasAgent={agent} onFocus={() => {}} shortcut={() => false} register={register} />);
const click = async (text: string) => { const button = [...host.querySelectorAll("button")].find(b => b.textContent === text)!; expect(button).toBeTruthy(); await act(async () => button.click()); };
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true); const storage = new Map<string, string>(); vi.stubGlobal("localStorage", { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) });
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }));
  vi.stubGlobal("requestAnimationFrame", () => 0);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.spyOn(bridge, "attach").mockResolvedValue(1); vi.spyOn(bridge, "resize").mockResolvedValue(); vi.spyOn(bridge, "close").mockResolvedValue();
  vi.spyOn(bridge, "fileList").mockResolvedValue(["sol"]);
  vi.spyOn(bridge, "api").mockResolvedValue({ type: "agent_info", agent: { agent: "claude", work_status: "blocked", agent_session: { kind: "path", value: "/chat.jsonl" } } });
  vi.spyOn(bridge, "fileStat").mockResolvedValue({ exists: true, size: transcript.length, mtime_ms: 1, inode: 1 });
  vi.spyOn(bridge, "fileRead").mockImplementation(async (_machine, path) => ({ size: transcript.length, mtime_ms: 1, inode: 1, offset: 0, data_b64: encode(path.endsWith("agent.json") ? JSON.stringify({ name: "Sol", pane: "p" }) : transcript) }));
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
it("names a blocked agent and persists Focus/Full tool disclosure across remount, with Brief for a shell", async () => {
  await act(async () => render());
  expect(host.querySelector(".pane-cap")?.textContent).toContain("claude");
  expect(host.querySelector('.pane-cap [aria-label="blocked"]')).not.toBeNull();
  await click("Chat");
  expect(host.querySelector('.pane-tools button[aria-pressed="true"]')?.textContent).toBe("Focus");
  expect(host.querySelector<HTMLDetailsElement>(".chat-tools")?.open).toBe(false);
  await click("Full");
  expect(host.querySelector<HTMLDetailsElement>(".chat-tools")?.open).toBe(true);
  expect(host.querySelector(".chat-tools details.chat-tool")?.hasAttribute("open")).toBe(true);
  act(() => root.unmount()); root = createRoot(host); await act(async () => render());
  expect(host.querySelector('.pane-tools button[aria-pressed="true"]')?.textContent).toBe("Full");
  expect(host.querySelector<HTMLDetailsElement>(".chat-tools")?.open).toBe(true);
  await click("Focus"); expect(host.querySelector<HTMLDetailsElement>(".chat-tools")?.open).toBe(false);
  act(() => root.unmount()); root = createRoot(host); await act(async () => render(false));
  expect(host.querySelector(".pane-cap")?.textContent).toContain("Brief");
  expect([...host.querySelectorAll("button")].some(b => b.textContent === "Full")).toBe(false);
});

// Integration through App → TabView → PaneSurface, faking only native IPC and time.
async function renderPanes(count: number) {
  const panes = Array.from({ length: count }, (_, i) => ({ ...pane, pane_id: `p${i}`, terminal_id: `term${i}` }));
  const snapshot: Snapshot = {
    workspaces: [{ workspace_id: "w", number: 1 }],
    tabs: [{ tab_id: "t", workspace_id: "w", number: 1, focused: true }],
    panes,
    agents: panes.map(p => ({ ...p, agent: "claude", agent_status: "blocked", work_status: "working" })),
    layouts: [{ tab_id: "t", area: { x: 0, y: 0, width: 900, height: 600 }, panes: panes.map((p, i) => ({ pane_id: p.pane_id, rect: { x: i * 900 / count, y: 0, width: 900 / count, height: 600 } })) }],
  };
  for (const p of panes) localStorage.setItem(`herdr-shell.mode.studio.${p.pane_id}`, "chat");
  vi.spyOn(bridge, "machines").mockResolvedValue([{ name: "studio", state: "up" }]);
  vi.spyOn(bridge, "snapshot").mockResolvedValue(snapshot);
  vi.spyOn(bridge, "machineEvents").mockResolvedValue(() => {});
  vi.spyOn(bridge, "snapshots").mockResolvedValue(() => {});
  vi.spyOn(bridge, "controlEvent").mockResolvedValue(() => {});
  vi.spyOn(bridge, "updateStatus").mockImplementation(() => new Promise(() => {}));
  vi.spyOn(bridge, "remoteHome").mockResolvedValue("/home/test");
  await act(async () => root.render(<App />));
}
it("the visible Chat cap uses work status and agent kind, not the card name", async () => {
  await renderPanes(1);
  const cap = host.querySelector('.pane-surface > .pane-cap');
  expect(cap).not.toBeNull();
  expect(cap?.querySelector('[aria-label="working"]')).not.toBeNull();
  expect(cap?.querySelector('.label')?.textContent).toBe("claude");
  expect(cap?.querySelector('.face')).not.toBeNull();
});
it("N panes share one agents directory read on mount and each poll", async () => {
  vi.useFakeTimers();
  try {
    await renderPanes(3);
    expect(host.querySelectorAll('.pane-surface > .pane-tools')).toHaveLength(3);
    const reads = () => vi.mocked(bridge.fileList).mock.calls.filter(([, path]) => path === AGENTS_DIR).length;
    expect(reads()).toBe(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(10000); });
    expect(reads()).toBe(2);
  } finally { vi.useRealTimers(); }
});
