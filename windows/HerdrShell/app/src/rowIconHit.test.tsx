// @vitest-environment happy-dom
// Mac parity (SpacesRowView.icon, G2): a row's trailing pin, + and chevron each take a hit area
// the row's height tall and 16 px wide, so a click near the glyph lands on it.
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
beforeEach(async () => {
  stored.set("herdr-shell.areas.mode", '"spaces"'); stored.set("herdr-space-expanded", JSON.stringify({ w: true }));
  vi.spyOn(bridge, "updateStatus").mockImplementation(() => new Promise(() => {}));
  const snapshot = { workspaces: [{ workspace_id: "w", number: 1, label: "Home" }], tabs: [{ tab_id: "a", workspace_id: "w", number: 1, label: "lane" }] };
  const host = document.createElement("div"); document.body.append(host);
  const styles = ["src/tokens.css", "src/styles.css"].map(path => { const style = document.createElement("style"); style.textContent = readFileSync(resolve(path), "utf8"); document.head.append(style); return style; });
  const root = createRoot(host), noop = () => {};
  await act(async () => root.render(<Sidebar snapshot={snapshot} rows={buildSidebar(snapshot)} selected={null} revealed={{ last: undefined, pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
  cleanup = () => { act(() => root.unmount()); host.remove(); styles.forEach(style => style.remove()); };
});
afterEach(() => { cleanup(); stored.clear(); vi.restoreAllMocks(); });

it("gives the space row's pin, + and chevron and a tab row's pin a full-height 16 px hit area", () => {
  const space = document.querySelector<HTMLElement>(".space-row")!;
  space.setAttribute("data-test-hover", "");
  const tab = document.querySelector<HTMLElement>(".tab-row")!;
  tab.setAttribute("data-test-hover", "");
  const controls = [space.querySelector(".pin"), space.querySelector(".sidebar-plus"), space.querySelector(".chevron"), tab.querySelector(".pin")];
  for (const control of controls) {
    const style = getComputedStyle(control as HTMLElement);
    expect(style.minWidth).toBe("16px");
    expect(style.alignSelf).toBe("stretch");
  }
});
