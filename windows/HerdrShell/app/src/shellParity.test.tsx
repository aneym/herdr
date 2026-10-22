// @vitest-environment happy-dom
// Real React sidebar and generated CSS; only native IPC is simulated.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import Sidebar from "./Sidebar";
import { readFileSync } from "node:fs";
const styles = readFileSync("src/styles.css", "utf8");
const tokens = readFileSync("src/tokens.css", "utf8");
const native = vi.hoisted(() => ({ invoke: vi.fn(async (cmd: string) => cmd === "api_request" ? { unchanged: true } : cmd === "open_url" ? undefined : new Promise(() => {})) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, Channel: class {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => stored.set(key, value) } });
const cleanups: (() => void)[] = [];
afterEach(() => { cleanups.splice(0).forEach(fn => fn()); document.body.innerHTML = ""; stored.clear(); vi.clearAllMocks(); });
function mount() {
  for (const css of [tokens, styles]) { const style = document.createElement("style"); style.textContent = css; document.head.append(style); cleanups.push(() => style.remove()); }
  const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
  cleanups.push(() => act(() => root.unmount())); return { host, root };
}
function sidebar() {
  const { host, root } = mount(); const noop = () => {};
  const rows = [
    { kind: "pinned" as const, id: "p1", label: "Pinned", status: "working", hotkey: null, section: "PINNED" },
    { kind: "agent" as const, id: "a1", label: "Hidden agent", status: "idle", hotkey: null, section: "AGENTS", hidden: true },
    { kind: "space" as const, id: "w2", label: "Hidden space", status: "idle", hotkey: null, section: "spaces", hidden: true },
    { kind: "space" as const, id: "w1", label: "recruiting", status: "done", hotkey: null, section: "spaces" },
    { kind: "tab" as const, id: "t1", spaceId: "w1", label: "Lane", status: "working", hotkey: null, section: "w1" },
  ];
  act(() => root.render(<Sidebar rows={rows} selected={null} revealed={{ last: null, pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
  return host;
}
it("paints pinned, group and lane state as token-sized solid elements rather than text bullets", () => {
  const host = sidebar(); const dots = host.querySelectorAll<HTMLElement>("nav .status");
  expect(dots.length).toBeGreaterThanOrEqual(3);
  for (const dot of dots) {
    expect(dot.textContent).toBe(""); expect(dot.style.width).toBe("var(--shell-face-dot)"); expect(dot.style.height).toBe("var(--shell-face-dot)");
    // Done and blocked are squares, as the TUI's state_icons and the Mac's statusGlyph draw them.
    const square = dot.classList.contains("done") || dot.classList.contains("blocked");
    expect(getComputedStyle(dot).borderRadius).toBe(square ? "1.5px" : "50%"); expect(getComputedStyle(dot).backgroundColor).not.toBe("transparent");
  }
});
it("draws every sidebar disclosure as a token-sized SVG and rotates it with the fold state", () => {
  const host = sidebar(); const group = host.querySelector<HTMLButtonElement>('[data-space="w1"]')!;
  expect(group.lastElementChild?.className).toBe("chevron");
  const checkToggle = (button: HTMLButtonElement) => {
    const check = () => {
      const svg = button.querySelector("svg")!;
      expect(svg).not.toBeNull(); expect(svg.getAttribute("aria-hidden")).toBe("true");
      expect(button.textContent).not.toMatch(/[⌄›▾▸]/);
      expect(getComputedStyle(svg).width).toBe("8px"); expect(getComputedStyle(svg).height).toBe("8px");
      expect(getComputedStyle(svg).transform).toBe(button.getAttribute("aria-expanded") === "true" ? "rotate(90deg)" : "rotate(0deg)");
    };
    check(); const before = button.getAttribute("aria-expanded");
    act(() => button.click()); expect(button.getAttribute("aria-expanded")).not.toBe(before); check();
  };
  checkToggle(group);
  checkToggle(host.querySelector<HTMLButtonElement>('[data-row="hiddenagents"]')!);
  checkToggle(host.querySelector<HTMLButtonElement>('.spaces > button[aria-expanded]')!);
  act(() => [...host.querySelectorAll<HTMLButtonElement>(".areas-mode button")].find(button => button.textContent === "Areas")!.click());
  checkToggle(host.querySelector<HTMLButtonElement>('[aria-label="Fold Focus"]')!);
});
it("keeps the Areas/Spaces switch compact and right-aligned beside collapse", () => {
  const host = sidebar(); const mode = host.querySelector<HTMLElement>(".areas-mode")!;
  expect(getComputedStyle(mode).justifyContent).toBe("flex-end");
  for (const button of mode.querySelectorAll("button:not(.spaces-fold-all)")) expect(getComputedStyle(button).flex).toBe("0 0 auto");
  expect(mode.lastElementChild?.classList.contains("spaces-fold-all")).toBe(true);
});
