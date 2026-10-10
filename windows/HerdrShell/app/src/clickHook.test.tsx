// @vitest-environment happy-dom
// Integration: control delivery to a mounted sidebar, with only the native API edge mocked.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { bridge } from "./bridge";
import { installControl } from "./control";
import Sidebar from "./Sidebar";
import { buildSidebar } from "./model";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => void stored.set(key, value) } });
const events = new Map<string, (payload: unknown) => void>();
const result = vi.fn(async (_cmd: string, _value: unknown) => {});
let cleanup: () => void;
beforeEach(async () => {
  stored.set("herdr-shell.areas.mode", '"spaces"'); events.clear(); result.mockClear();
  vi.spyOn(bridge, "controlEvent").mockImplementation(async (cmd, fn) => { events.set(cmd, fn as (payload: unknown) => void); return () => { events.delete(cmd); }; });
  vi.spyOn(bridge, "controlResult").mockImplementation(result);
  vi.spyOn(bridge, "api").mockResolvedValue({ tab: { tab_id: "new" } });
  vi.spyOn(bridge, "updateStatus").mockImplementation(() => new Promise(() => {}));
  const snapshot = { workspaces: [{ workspace_id: "w", number: 1, label: "Home" }], tabs: [] };
  const host = document.createElement("div"); document.body.append(host);
  const style = document.createElement("style"); style.textContent = readFileSync(resolve("src/styles.css"), "utf8"); document.head.append(style);
  const root = createRoot(host), noop = () => {};
  await act(async () => root.render(<Sidebar snapshot={snapshot} rows={buildSidebar(snapshot)} selected={null} revealed={{ last: undefined, pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
  const stop = installControl(() => ({ machine: { name: "studio", state: "up" }, machines: [], chooseMachine: () => ({ name: "studio", state: "up" }), selected: null, docs: { open: false, items: [], active: null }, rows: [], panes: [], focused: undefined, open: noop, action: async () => {} }));
  await Promise.resolve();
  cleanup = () => { stop(); act(() => root.unmount()); host.remove(); style.remove(); };
});
afterEach(() => { cleanup(); stored.clear(); vi.restoreAllMocks(); });
async function command(cmd: string, payload: unknown) {
  result.mockClear();
  await act(async () => { events.get(cmd)!(payload); await vi.waitFor(() => expect(result).toHaveBeenCalled()); });
  return result.mock.calls[result.mock.calls.length - 1]?.[1];
}
it("reveals the hover-only space plus while clicking and creates a tab", async () => {
  const button = document.querySelector<HTMLButtonElement>('.space-row .sidebar-plus')!;
  expect(getComputedStyle(button).display).toBe("none");
  let display = "", marked = false;
  button.addEventListener("click", () => { display = getComputedStyle(button).display; marked = button.closest(".sidebar-row")!.hasAttribute("data-test-hover"); });
  expect(await command("click", { selector: '.space-row .sidebar-plus', hover: true })).toEqual({ ok: true, matched: 1 });
  expect(display).toBe("block"); expect(marked).toBe(true);
  expect(bridge.api).toHaveBeenCalledWith("studio", "tab.create", { workspace_id: "w", focus: false });
  expect(document.querySelector("[data-test-hover]")).toBeNull();
  expect(getComputedStyle(button).display).toBe("none");
});
it("returns no match, including an out-of-range nth", async () => {
  expect(await command("click", { selector: ".missing" })).toEqual({ ok: false, error: "no match" });
  expect(await command("click", { selector: ".sidebar-plus", nth: 99 })).toEqual({ ok: false, error: "no match" });
  expect(bridge.api).not.toHaveBeenCalled();
});
it("escapes a quoted drag section and tab id rather than failing selector parsing", async () => {
  const row = document.createElement("div"); row.dataset.row = 'quoted"section:tab"id'; document.body.append(row);
  vi.spyOn(document, "elementFromPoint").mockReturnValue(row);
  try {
    expect(await command("drag_pin", { section: 'quoted"section', tab_id: 'tab"id', steps: 1, interval_ms: 0 })).toMatchObject({ ok: true });
  } finally { row.remove(); }
});
it("keeps hover controls visible across commands until explicitly cleared", async () => {
  const selector = ".space-row .sidebar-plus";
  const button = document.querySelector<HTMLButtonElement>(selector)!;
  expect(await command("hover", { selector, on: true })).toEqual({ ok: true });
  expect(getComputedStyle(button).display).toBe("block");
  await command("click", { selector, hover: true });
  expect(button.closest(".sidebar-row")!.hasAttribute("data-test-hover")).toBe(true);
  expect(getComputedStyle(button).display).toBe("block");
  expect(await command("hover", { selector, on: false })).toEqual({ ok: true });
  expect(getComputedStyle(button).display).toBe("none");
  expect(document.querySelector("[data-test-hover]")).toBeNull();
  expect(await command("hover", { selector: ".missing", on: true })).toEqual({ ok: false, error: "no match" });
});
it.each(["click", "hover"])("rejects %s at the real CLI boundary without test-window mode", cmd => {
  const script = resolve("../scripts/pc.py");
  let error: { status?: number; stderr?: Buffer } | undefined;
  try { execFileSync("python3", [script, "ctl", JSON.stringify({ cmd, selector: ".sidebar-plus", hover: true, on: true })], { stdio: "pipe" }); }
  catch (caught) { error = caught as typeof error; }
  expect(error?.status).toBe(2);
  expect(error?.stderr?.toString()).toContain("command requires --test-window");
  expect(document.querySelector("[data-test-hover]")).toBeNull();
  expect(bridge.api).not.toHaveBeenCalled();
});
