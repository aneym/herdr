// @vitest-environment happy-dom
// Real React desk/sidebar and generated CSS; only native IPC is simulated.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import DocPanel from "./DocPanel";
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
it("shows the desk path and routes Open, add and close through their native/server boundaries", async () => {
  const { host, root } = mount(); const close = vi.fn();
  const file = { id: "d1", name: "Notes", kind: "file" as const, path: "/Studio/notes.md", mime: "text/markdown" };
  const render = (item: typeof file | { name: string; kind: "web"; url: string }) => act(() => root.render(<DocPanel machine="studio" tab="t1" items={[item]} active={"id" in item ? item.id : item.name} select={() => {}} error="" close={close} />));
  await render(file);
  expect(host.querySelector(".docs-path")?.textContent).toBe("/Studio/notes.md");
  const open = host.querySelector<HTMLButtonElement>('[aria-label="Open document externally"]')!;
  expect(open.textContent).toBe("Open"); expect(open.disabled).toBe(true); expect(open.title).toBe("File is on Studio");
  const add = host.querySelector<HTMLButtonElement>('[aria-label="Add document"]')!;
  expect(add.textContent).toBe("+"); await act(async () => add.click());
  const input = host.querySelector<HTMLInputElement>('[aria-label="Document address"]')!;
  await act(async () => { Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "https://example.com/new"); input.dispatchEvent(new Event("input", { bubbles: true })); });
  await act(async () => host.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })));
  expect(native.invoke).toHaveBeenCalledWith("api_request", { machine: "studio", method: "desk.open", params: { tab_id: "t1", ref: "https://example.com/new", opened_by: "user" } });
  const cross = host.querySelector<HTMLButtonElement>('[aria-label="Close document"]')!;
  expect(cross.textContent).toBe("✕"); await act(async () => cross.click());
  expect(native.invoke).toHaveBeenCalledWith("api_request", { machine: "studio", method: "desk.close", params: { tab_id: "t1", item: "d1" } });
  await render({ name: "Scope", kind: "web", url: "https://example.com/scope" });
  await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Open document externally"]')!.click());
  expect(native.invoke).toHaveBeenCalledWith("open_url", { url: "https://example.com/scope" });
  await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Close document"]')!.click()); expect(close).toHaveBeenCalledOnce();
});
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
    expect(getComputedStyle(dot).borderRadius).toBe("50%"); expect(getComputedStyle(dot).backgroundColor).not.toBe("transparent");
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
