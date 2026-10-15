// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { bridge } from "./bridge";
import Sidebar from "./Sidebar";
import { buildSidebar } from "./model";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (key: string) => stored.get(key) ?? null, setItem: (key: string, value: string) => void stored.set(key, value) } });
let cleanup: () => void;
let opened: string[];
beforeEach(async () => {
  stored.set("herdr-shell.areas.mode", '"spaces"'); stored.set("herdr-space-expanded", JSON.stringify({ w: true }));
  opened = [];
  vi.spyOn(bridge, "updateStatus").mockImplementation(() => new Promise(() => {}));
  vi.spyOn(bridge, "openUrl").mockImplementation(async url => { opened.push(url); });
  const snapshot = { workspaces: [{ workspace_id: "w", number: 1, label: "rails" }], tabs: [
    { tab_id: "p", workspace_id: "w", number: 1, label: "Working in Rails", pin_index: 0, doc: { url: "https://example.com/review", title: "Rails scope" } },
    { tab_id: "u", workspace_id: "w", number: 2, label: "plain doc", doc: { url: "https://example.com/plain", title: "Plain" } },
    { tab_id: "n", workspace_id: "w", number: 3, label: "no doc" },
    { tab_id: "f", workspace_id: "w", number: 4, label: "file doc", doc: { url: "/Users/x/plan.md", title: "plan.md" } },
  ] };
  const host = document.createElement("div"); document.body.append(host);
  const styles = ["src/tokens.css", "src/styles.css"].map(path => { const style = document.createElement("style"); style.textContent = readFileSync(resolve(path), "utf8"); document.head.append(style); return style; });
  const root = createRoot(host), noop = () => {};
  await act(async () => root.render(<Sidebar snapshot={snapshot} rows={buildSidebar(snapshot)} selected={null} revealed={{ last: undefined, pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
  cleanup = () => { act(() => root.unmount()); host.remove(); styles.forEach(style => style.remove()); };
});
afterEach(() => { cleanup(); stored.clear(); vi.restoreAllMocks(); });

const shown = (row: Element | null) => !!row?.querySelector(".doc-link") && getComputedStyle(row!.querySelector(".doc-link")!).display !== "none";

it("shows a pinned tab's doc icon at rest on every row of that tab, and unpinned docs only on hover", () => {
  for (const id of ["pinned:p", "tab:p"]) expect(shown(document.querySelector(`[data-row="${id}"]`)), id).toBe(true);
  const plain = document.querySelector<HTMLElement>('[data-row="tab:u"]')!;
  expect(shown(plain)).toBe(false);
  plain.setAttribute("data-test-hover", "");
  expect(shown(plain)).toBe(true);
  const none = document.querySelector<HTMLElement>('[data-row="tab:n"]')!;
  none.setAttribute("data-test-hover", "");
  expect(none.querySelector(".doc-link")).toBeNull();
  const file = document.querySelector<HTMLElement>('[data-row="tab:f"]')!;
  expect(file.querySelector(".doc-link")).toBeNull();
});

it("opens the doc in the browser and leaves the selection alone", async () => {
  const select = vi.fn();
  const button = document.querySelector<HTMLButtonElement>('[data-row="pinned:p"] .doc-link')!;
  expect(button.getAttribute("aria-label")).toBe("Open review doc Rails scope");
  await act(async () => button.click());
  expect(opened).toEqual(["https://example.com/review"]);
  expect(select).not.toHaveBeenCalled();
});
