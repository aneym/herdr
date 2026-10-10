// @vitest-environment happy-dom
// Integration: real PaneSurface and PaneTerm; only native IPC and browser geometry are faked.
// Guards Mac PaneCapBar parity: glyph and name with no agent face, a Terminal | Chat segment,
// and Take control (which the Mac never shows in the header) reached from the more menu.
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import PaneSurface from "./PaneSurface";
import { bridge } from "./bridge";
import type { Pane } from "./model";
let root: Root, host: HTMLDivElement;
const pane: Pane = { pane_id: "p", terminal_id: "term", tab_id: "t", workspace_id: "w", agent: "claude", agent_status: "idle" };
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) });
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }));
  vi.stubGlobal("requestAnimationFrame", () => 0);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  // Another client holds the terminal: attach is refused, observe succeeds.
  vi.spyOn(bridge, "attach").mockImplementation(async (_machine, _terminal, _cols, _rows, mode) => { if (mode === "attach") throw new Error("busy"); return 7; });
  vi.spyOn(bridge, "resize").mockResolvedValue(); vi.spyOn(bridge, "close").mockResolvedValue();
  vi.spyOn(bridge, "api").mockResolvedValue({ type: "agent_info", agent: { agent: "claude", work_status: "idle" } });
  vi.spyOn(bridge, "fileStat").mockResolvedValue({ exists: false, size: 0, mtime_ms: 0, inode: 0 });
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(() => { act(() => root.unmount()); host.remove(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
const press = async (button: Element | null | undefined) => { expect(button).toBeTruthy(); await act(async () => (button as HTMLButtonElement).click()); };
const byText = (scope: ParentNode, text: string) => [...scope.querySelectorAll("button")].find(b => b.textContent === text);

it("matches the Mac pane header: no face, a Terminal | Chat segment, pin, and Take control in the more menu", async () => {
  const take = vi.spyOn(bridge, "takeControl").mockResolvedValue();
  await act(async () => root.render(<PaneSurface agent={{ ...pane, agent: "claude", agent_status: "idle" }} pane={pane} machine="studio" focused hasAgent pinned={false} onPin={() => {}} onFocus={() => {}} shortcut={() => false} register={() => {}} />));
  const cap = host.querySelector(".pane-cap")!;
  expect(cap.querySelector(".label")?.textContent).toBe("claude");
  expect(cap.querySelector('[aria-label="idle"]')).not.toBeNull();
  expect(cap.querySelector(".face")).toBeNull();
  expect(byText(host, "Take control")).toBeUndefined();
  const segment = host.querySelector('.pane-tools [role="group"]')!;
  expect([...segment.querySelectorAll("button")].map(b => [b.textContent, b.getAttribute("aria-pressed")])).toEqual([["Terminal", "true"], ["Chat", "false"]]);
  expect(host.querySelector('.pane-tools button[aria-label="Pin this tab"]')).not.toBeNull();
  await press(host.querySelector('.pane-tools button[aria-label="Pane actions"]'));
  const menu = host.querySelector('[role="menu"]')!;
  expect([...menu.querySelectorAll('[role="menuitem"]')].map(item => item.textContent)).toEqual(["Take control", "Restart agent"]);
  await press(byText(menu, "Take control"));
  expect(take).toHaveBeenCalledWith(7);
  expect(host.querySelector('[role="menu"]')).toBeNull();
  await act(async () => {});
  await press(host.querySelector('.pane-tools button[aria-label="Pane actions"]'));
  expect([...host.querySelectorAll('[role="menu"] [role="menuitem"]')].map(item => item.textContent)).toEqual(["Restart agent"]);
  await press(byText(segment, "Chat"));
  expect([...host.querySelectorAll('.pane-tools [role="group"] button')].map(b => b.getAttribute("aria-pressed"))).toEqual(["false", "true"]);
});
