// @vitest-environment happy-dom
// Integration through App: only native IPC is faked. Guards the Mac parity placement of Update:
// a title-bar button whose menu restarts to update, rolls back, or puts the release off.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import App from "./App";
import { bridge, type UpdateStatus } from "./bridge";
import type { Snapshot } from "./model";
let root: Root, host: HTMLDivElement;
const snapshot: Snapshot = { workspaces: [{ workspace_id: "w", number: 1 }], tabs: [{ tab_id: "t", workspace_id: "w", number: 1, focused: true }], panes: [], agents: [] };
const staged = { sha: "abcdef123456", built_at: "2026-10-06T12:00:00Z" };
const previous = { sha: "fedcba123456" };
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) });
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }));
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.spyOn(bridge, "machines").mockResolvedValue([{ name: "studio", state: "up" }]);
  vi.spyOn(bridge, "snapshot").mockResolvedValue(snapshot);
  vi.spyOn(bridge, "machineEvents").mockResolvedValue(() => {});
  vi.spyOn(bridge, "snapshots").mockResolvedValue(() => {});
  vi.spyOn(bridge, "controlEvent").mockResolvedValue(() => {});
  vi.spyOn(bridge, "remoteHome").mockResolvedValue("/home/test");
  vi.spyOn(bridge, "fileList").mockResolvedValue([]);
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const mount = async (status: UpdateStatus) => {
  vi.spyOn(bridge, "updateStatus").mockResolvedValue(status);
  await act(async () => root.render(<App />));
};
const buttons = () => [...host.querySelectorAll("button")];
const press = async (text: string) => { const button = buttons().find(b => b.textContent === text); expect(button, text).toBeTruthy(); await act(async () => button!.click()); };

it("offers Update in the title strip, not the sidebar footer, with restart, roll back and later in its menu", async () => {
  const apply = vi.spyOn(bridge, "updateApply").mockResolvedValue();
  const rollback = vi.spyOn(bridge, "updateRollback").mockResolvedValue();
  await mount({ current: "1234567", available: true, staged, previous });
  expect(host.querySelector(".sidebar footer")?.textContent ?? "").not.toMatch(/Update|Roll back/);
  const update = host.querySelector<HTMLButtonElement>(".layout > .title-strip button[aria-haspopup=menu]");
  expect(update?.textContent).toBe("Update");
  expect(update?.closest(".sidebar")).toBeNull();
  await press("Update");
  const items = [...host.querySelectorAll('.title-strip [role="menuitem"]')].map(item => item.textContent);
  expect(items).toEqual(["Restart to update", "Roll back to fedcba1", "Later"]);
  expect(host.querySelector(".title-strip [role=menu]")?.textContent).toContain("abcdef12");
  await press("Roll back to fedcba1");
  expect(rollback).toHaveBeenCalledTimes(1);
  expect(apply).not.toHaveBeenCalled();
});

it("Later hides the offered release across a remount and leaves roll back as the only action", async () => {
  await mount({ current: "1234567", available: true, staged, previous });
  await press("Update");
  await press("Later");
  act(() => root.unmount()); root = createRoot(host);
  await mount({ current: "1234567", available: true, staged, previous });
  expect(buttons().some(b => b.textContent === "Update")).toBe(false);
  await press("Roll back");
  expect([...host.querySelectorAll('.title-strip [role="menuitem"]')].map(item => item.textContent)).toEqual(["Restart to update", "Roll back to fedcba1"]);
});

it("shows a failed apply in the menu and retries from it", async () => {
  const apply = vi.spyOn(bridge, "updateApply").mockRejectedValueOnce(new Error("bundle missing")).mockResolvedValue();
  await mount({ current: "1234567", available: true, staged, previous: null });
  await press("Update");
  await press("Restart to update");
  expect(host.querySelector('.title-strip [role="alert"]')?.textContent).toContain("bundle missing");
  await press("Retry");
  expect(apply).toHaveBeenCalledTimes(2);
});
