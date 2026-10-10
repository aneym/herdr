// @vitest-environment happy-dom
import { StrictMode, act, useState } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
const tokensCSS = readFileSync("src/tokens.css", "utf8");
import { appTheme } from "./theme";
import { LaneSnapshot } from "./laneFiles";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
vi.mock("./bridge", () => ({ bridge: { updateStatus: () => new Promise(() => {}), fileList: async () => [], fileRead: async () => ({ data_b64: "" }) }, fromBase64: () => new Uint8Array() }));
const { default: Sidebar, useSelectionReveal } = await import("./Sidebar");
// Reveal-on-select spans App state, the sidebar's mount and its stored folds: a fold the user
// made must survive hiding the sidebar, and a selection made while hidden must still reveal.
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
// Node 26's own localStorage global is undefined without --localstorage-file and hides happy-dom's.
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => void stored.set(k, v), clear: () => stored.clear() } });
const snapshot: Snapshot = {
  workspaces: [{ workspace_id: "a", number: 1, label: "alpha" }, { workspace_id: "b", number: 2, label: "beta" }, { workspace_id: "r", number: 3, label: "rails", parked: true }],
  tabs: [{ tab_id: "a:1", workspace_id: "a", number: 1 }, { tab_id: "b:1", workspace_id: "b", number: 1 }, { tab_id: "r:1", workspace_id: "r", number: 1 }, { tab_id: "a:2", workspace_id: "a", number: 2, pin_index: 0, label: "pinned chat" }, { tab_id: "b:2", workspace_id: "b", number: 2, pin_index: 1, role: "agent", label: "lead" }],
};
let drive: { select: (id: string) => void; show: (visible: boolean) => void } = { select: () => {}, show: () => {} };
function Shell() {
  // The parent's side of App's wiring: selection and sidebar visibility live above the sidebar.
  const [selected, select] = useState<string | null>(null);
  const [visible, show] = useState(true);
  const revealed = useSelectionReveal(selected);
  drive = { select, show };
  const noop = () => {};
  return visible ? <Sidebar rows={buildSidebar(snapshot)} selected={selected} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={null} select={select} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} /> : null;
}
describe.each([["plain", false], ["StrictMode", true]])("sidebar reveal on select (%s)", (_name, strict) => {
  let host: HTMLDivElement, root: Root;
  beforeEach(() => { localStorage.clear(); host = document.createElement("div"); document.body.append(host); root = createRoot(host); act(() => root.render(strict ? <StrictMode><Shell /></StrictMode> : <Shell />)); });
  afterEach(() => { act(() => root.unmount()); host.remove(); });
  const open = (label: string) => [...host.querySelectorAll<HTMLButtonElement>(".space-row")].find(b => b.textContent?.includes(label))?.getAttribute("aria-expanded");
  const fold = (label: string) => act(() => [...host.querySelectorAll<HTMLButtonElement>(".space-row")].find(b => b.textContent?.includes(label))!.click());
  it("keeps a user fold through hide and show, and reveals a selection made while hidden", () => {
    act(() => drive.select("a:1"));
    expect(open("alpha")).toBe("true");
    fold("alpha");
    expect(open("alpha")).toBe("false");
    // Ctrl+B twice on the same selection: the fold sticks.
    act(() => drive.show(false)); act(() => drive.show(true));
    expect(open("alpha")).toBe("false");
    // Hidden, select b then a again: showing the sidebar reveals a.
    act(() => drive.show(false)); act(() => drive.select("b:1")); act(() => drive.select("a:1")); act(() => drive.show(true));
    expect(open("alpha")).toBe("true");
  });
  it("selects a pinned row without reopening its home, while a non-pinned selection reveals", () => {
    act(() => drive.select("a:1"));
    fold("alpha");
    expect(open("alpha")).toBe("false");
    act(() => host.querySelector<HTMLButtonElement>('[data-row="pinned:a:2"] .select-tab')!.click());
    expect(host.querySelector('[data-row="pinned:a:2"]')?.classList.contains("selected")).toBe(true);
    expect(open("alpha")).toBe("false");
    act(() => drive.select("a:1"));
    expect(open("alpha")).toBe("true");
  });
  it("collapses and expands every space without folding AGENTS or PINNED, and persists", () => {
    act(() => drive.select("a:1"));
    expect(host.querySelector('.areas-mode [aria-label="Collapse all spaces"]')).not.toBeNull();
    expect(host.textContent).not.toContain("goal");
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Collapse all spaces"]')!.click());
    for (const label of ["alpha", "beta", "rails"]) expect(open(label)).toBe("false");
    expect(host.querySelector('[data-row="pinned:a:2"]')).not.toBeNull();
    expect(host.querySelector('[data-row="agent:b:2"]')).not.toBeNull();
    act(() => drive.show(false)); act(() => drive.show(true));
    for (const label of ["alpha", "beta", "rails"]) expect(open(label)).toBe("false");
    act(() => host.querySelector<HTMLButtonElement>('[aria-label="Expand all spaces"]')!.click());
    for (const label of ["alpha", "beta", "rails"]) expect(open(label)).toBe("true");
    expect(host.querySelector('[data-row="pinned:a:2"]')).not.toBeNull();
    expect(host.querySelector('[data-row="agent:b:2"]')).not.toBeNull();
  });
  it("opens a parked space for its selected tab, once", () => {
    expect(open("rails")).toBe("false");
    act(() => drive.select("r:1"));
    expect(open("rails")).toBe("true");
    fold("rails");
    act(() => drive.show(false)); act(() => drive.show(true));
    expect(open("rails")).toBe("false");
  });
});

// Real catalog -> sidebar DOM: preserve the fill, add visibility, and follow live theme changes.
describe("area dot visibility", () => {
  it("rings the factory dot in dark mode and removes the ring in light mode", () => {
    localStorage.clear();
    localStorage.setItem("herdr-shell.areas.mode", JSON.stringify("areas"));
    const style = document.createElement("style"); style.textContent = tokensCSS; document.head.append(style);
    const catalog = new LaneSnapshot();
    catalog.areas = [{ id: "factory", name: "factory", color: "#1F1F23" }];
    catalog.spaces = { a: "factory" };
    const host = document.createElement("div"); document.body.append(host);
    const root = createRoot(host), noop = () => {};
    appTheme().setOverride("dark");
    try {
      act(() => root.render(<Sidebar snapshot={snapshot} catalog={catalog} rows={[]} selected={null} revealed={{ last: null, pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
      const dot = host.querySelector<HTMLElement>(".areas-dot")!;
      expect(dot.classList.contains("areas-dot-ring")).toBe(true);
      expect(dot.style.backgroundColor).toBe("#1F1F23");
      act(() => appTheme().setOverride("light"));
      expect(dot.classList.contains("areas-dot-ring")).toBe(false);
    } finally {
      act(() => root.unmount()); host.remove(); style.remove(); localStorage.clear(); appTheme().setOverride("system");
    }
  });
});
