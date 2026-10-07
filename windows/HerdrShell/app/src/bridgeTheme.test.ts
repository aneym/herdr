// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";

// Tauri is the external IPC boundary; the real bridge and ThemeStore run together.
const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: ipc.invoke, Channel: class { onmessage = () => {}; } }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); ipc.invoke.mockReset(); });

it("reports the current theme on attach, media changes, and reconnect, but stops after close", async () => {
  const listeners = new Set<() => void>();
  const media = { matches: true, addEventListener: (_: string, fn: () => void) => listeners.add(fn), removeEventListener: (_: string, fn: () => void) => listeners.delete(fn) };
  vi.stubGlobal("matchMedia", () => media);
  let nextHandle = 6;
  ipc.invoke.mockImplementation(async cmd => cmd === "attach_open" ? ++nextHandle : undefined);
  const { bridge } = await import("./bridge");
  const { appTheme } = await import("./theme");
  const handle = await bridge.attach("studio", "term", 80, 24, "attach", () => {});
  expect(ipc.invoke).toHaveBeenLastCalledWith("attach_theme", { handle, dark: true, foreground: [205, 214, 244], background: [30, 30, 46] });
  media.matches = false;
  listeners.forEach(fn => fn());
  expect(ipc.invoke).toHaveBeenLastCalledWith("attach_theme", { handle, dark: false, foreground: [58, 58, 56], background: [255, 255, 255] });
  await bridge.close(handle);
  const observer = await bridge.attach("studio", "term", 80, 24, "observe", () => {});
  expect(ipc.invoke).toHaveBeenLastCalledWith("attach_theme", { handle: observer, dark: false, foreground: [58, 58, 56], background: [255, 255, 255] });
  await bridge.close(observer);
  ipc.invoke.mockClear();
  media.matches = true;
  listeners.forEach(fn => fn());
  expect(ipc.invoke).not.toHaveBeenCalled();
  appTheme().dispose();
});
