// @vitest-environment happy-dom
// Integration: real App and Factory projection, fake native IPC only.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { bridge } from "./bridge";
import App from "./App";
import type { FactoryBundle } from "./factory";
import fixture from "./fixtures/factory.json";
const sample: FactoryBundle = fixture;
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
it("toggles the real pane surface with Ctrl+Shift+F and mirrors Mac bundle rows", async () => {
 vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
 const storage = new Map<string, string>(); vi.stubGlobal("localStorage", { getItem: (k: string) => storage.get(k) ?? null, setItem: (k: string, v: string) => storage.set(k, v) });
 vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
 vi.spyOn(bridge, "machineEvents").mockResolvedValue(() => {}); vi.spyOn(bridge, "snapshots").mockResolvedValue(() => {});
 vi.spyOn(bridge, "machines").mockResolvedValue([{ name: "studio", state: "up" }]); vi.spyOn(bridge, "snapshot").mockResolvedValue({});
 vi.spyOn(bridge, "controlEvent").mockResolvedValue(() => {}); vi.spyOn(bridge, "updateStatus").mockImplementation(() => new Promise(() => {}));
 vi.spyOn(bridge, "fileList").mockResolvedValue([]); vi.spyOn(bridge, "fileRead").mockRejectedValue(new Error("missing")); vi.spyOn(bridge, "fileStat").mockResolvedValue({ exists: false, size: 0, mtime_ms: 0, inode: 0 });
 const native = vi.spyOn(bridge, "factory").mockResolvedValue(sample);
 const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
 dispose = () => { act(() => root.unmount()); host.remove(); };
 await act(async () => root.render(<App />));
 const toggle = async () => act(async () => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "F", ctrlKey: true, shiftKey: true, bubbles: true })); });
 expect(host.querySelector(".factory-view")).toBeNull(); await toggle();
 expect(native).toHaveBeenCalledWith("studio");
 expect([...host.querySelectorAll(".factory-view h2")].map(e => e.textContent)).toEqual(["Machines", "Pools · 0 s ago", "Routing · ladder: interim", "In flight"]);
 for (const text of ["123.4 GiB · ok", "drained (maintenance)", "2/4", "2/3", "75%", "pace 80%", "sol-medium", "Decider: opus (room), decider.json v3", "Factory parity", "headless", "Landed today: 1", "feat: previous slice"]) expect(host.textContent).toContain(text);
 expect(host.querySelector(".factory-pane-host")?.hasAttribute("hidden")).toBe(true);
 await toggle(); expect(host.querySelector(".factory-view")).toBeNull(); expect(host.querySelector(".factory-pane-host")?.hasAttribute("hidden")).toBe(false);
 native.mockResolvedValue({ ...sample, pools: null }); await toggle(); expect(host.textContent).toContain("no pool data");
});

import FactoryPage from "./FactoryView";
it("renders wk, attention and usage tokens, and picks only when expanded", async () => {
 vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
 vi.spyOn(bridge, "factory").mockResolvedValue({ ...sample, overlay: { hosts: [
  { name: "studio", summary: "busy", attention: "act", usage: { state: "overloaded", age_s: 1, cpu_pct: 99 } },
  { name: "pc", summary: "quiet", attention: "warn", usage: { state: "idle", age_s: 1, cpu_pct: 1 } },
  { name: "forge", summary: "running", usage: { state: "busy", age_s: 1, cpu_pct: 50 } }
 ] }, poolState: { drained: ["studio"], down: { pc: "2099-10-08T04:00:00Z" } }, disk: { forge: { held: true } } });
 const pick = vi.fn(async () => "native route result");
 const descriptor = Object.getOwnPropertyDescriptor(bridge, "factoryRoutePick");
 Object.defineProperty(bridge, "factoryRoutePick", { configurable: true, value: pick });
 const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
 dispose = () => { act(() => root.unmount()); host.remove(); if (descriptor) Object.defineProperty(bridge, "factoryRoutePick", descriptor); else delete (bridge as unknown as Record<string, unknown>).factoryRoutePick; };
 await act(async () => root.render(<FactoryPage machine="studio" online />));
 expect(host.querySelector('meter[aria-label="sol wk"]')).not.toBeNull();
 const row = (name: string) => host.querySelector(`[data-machine="${name}"]`)!;
 for (const [name, token] of [["studio", "bad"], ["pc", "warn"]]) {
  expect(row(name).querySelector<HTMLElement>(".factory-machine-name")?.style.color).toBe(`var(--shell-${token})`);
  expect(row(name).querySelector<HTMLElement>(".factory-machine-summary")?.style.color).toBe(`var(--shell-${token})`);
 }
 for (const name of ["studio", "pc", "forge"]) expect(row(name).querySelector<HTMLElement>(".factory-machine-state")?.style.color).toBe("var(--shell-warn)");
 expect(row("pc").textContent).toContain("down until 00:00");
 for (const [name, glyph, token] of [["studio", "blocked", "bad"], ["pc", "idle", "muted"], ["forge", "working", "ok"]]) {
  expect(row(name).querySelector(`[data-usage-glyph="${glyph}"]`)).not.toBeNull();
  expect(row(name).querySelector<HTMLElement>(".factory-usage-line")?.style.color).toBe(`var(--shell-${token})`);
 }
 expect(pick).not.toHaveBeenCalled();
 const route = (name: string) => [...host.querySelectorAll<HTMLButtonElement>(".factory-route")].find(b => b.textContent?.includes(name))!;
 await act(async () => route("implement").click()); expect(pick).toHaveBeenCalledWith("studio", "implement"); expect(host.textContent).toContain("native route result");
 pick.mockRejectedValue(new Error("host unavailable")); await act(async () => route("mechanical").click()); expect(host.textContent).toContain("route pick failed");
});
