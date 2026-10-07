// @vitest-environment happy-dom
import { StrictMode, act, useState } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
  tabs: [{ tab_id: "a:1", workspace_id: "a", number: 1 }, { tab_id: "b:1", workspace_id: "b", number: 1 }, { tab_id: "r:1", workspace_id: "r", number: 1 }],
};
let drive: { select: (id: string) => void; show: (visible: boolean) => void } = { select: () => {}, show: () => {} };
function Shell() {
  // The parent's side of App's wiring: selection and sidebar visibility live above the sidebar.
  const [selected, select] = useState<string | null>(null);
  const [visible, show] = useState(true);
  const revealed = useSelectionReveal(selected);
  drive = { select, show };
  const noop = () => {};
  return visible ? <Sidebar machines={[]} chooseMachine={noop} rows={buildSidebar(snapshot)} selected={selected} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={null} select={select} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} /> : null;
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
  it("opens a parked space for its selected tab, once", () => {
    expect(open("rails")).toBe("false");
    act(() => drive.select("r:1"));
    expect(open("rails")).toBe("true");
    fold("rails");
    act(() => drive.show(false)); act(() => drive.show(true));
    expect(open("rails")).toBe("false");
  });
});
