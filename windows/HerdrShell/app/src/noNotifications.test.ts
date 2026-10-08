// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { controlKey } from "./keys";
import type { Snapshot } from "./model";

// The source scan is an explicitly requested absence contract: restoring either
// native delivery API or its dependency/permission fails, regardless of wiring.
// Node runs this integration scan; the browser-only project has no Node types.
const fsModule = "node:fs", pathModule = "node:path", urlModule = "node:url";
const { readdirSync, readFileSync } = await import(/* @vite-ignore */ fsModule) as {
  readdirSync(path: string, options: { withFileTypes: true }): { name: string; isDirectory(): boolean }[];
  readFileSync(path: string, encoding: "utf8"): string;
};
const { dirname, join } = await import(/* @vite-ignore */ pathModule) as {
  dirname(path: string): string; join(...paths: string[]): string;
};
const { fileURLToPath } = await import(/* @vite-ignore */ urlModule) as { fileURLToPath(url: string): string };
const app = dirname(dirname(fileURLToPath(import.meta.url)));
function files(path: string): string[] {
  return readdirSync(path, { withFileTypes: true }).flatMap(entry => {
    const child = join(path, entry.name);
    return entry.isDirectory() ? files(child) : [child];
  });
}
it("Windows Shell has no attention notification delivery or permissions", () => {
  const paths = [
    ...files(join(app, "src")).filter(path => !/\.(test|spec)\./.test(path)),
    join(app, "package.json"), join(app, "src-tauri/Cargo.toml"),
    ...files(join(app, "src-tauri/src")), ...files(join(app, "src-tauri/capabilities")),
  ];
  const forbidden = /requestUserAttention|plugin-notification|sendNotification|notification:|tauri_plugin_notification/;
  expect(paths.filter(path => forbidden.test(readFileSync(path, "utf8")))).toEqual([]);
});

// Reuse keyboardParity's controlKey input helper and snapshot-event boundary.
// Unlike its hidden-entry case, this guards the latest visible transition after
// notification removal, without adding a production-only test seam.
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
  getItem: (key: string) => stored.get(key) ?? null,
  setItem: (key: string, value: string) => void stored.set(key, value),
  clear: () => stored.clear(),
} });
let publishSnapshot: ((value: { machine: string; snapshot: Snapshot }) => void) | undefined;
const baseline: Snapshot = {
  workspaces: [{ workspace_id: "a", number: 1 }, { workspace_id: "b", number: 2, active_tab_id: "b2" }],
  tabs: [
    { tab_id: "a1", workspace_id: "a", number: 1, label: "First", role: "agent" },
    { tab_id: "b1", workspace_id: "b", number: 1, label: "Second", role: "agent" },
    { tab_id: "b2", workspace_id: "b", number: 2, label: "Active" },
  ],
};
vi.mock("./bridge", () => ({ bridge: {
  machines: async () => [{ name: "studio", state: "up" }], snapshot: async () => baseline,
  machineEvents: async () => () => {}, snapshots: async (publish: typeof publishSnapshot) => { publishSnapshot = publish; return () => {}; },
  controlEvent: async () => () => {}, api: async () => ({}), updateStatus: () => new Promise(() => {}),
  fileList: async () => [], fileRead: async () => ({ data_b64: "" }),
}, fromBase64: () => new Uint8Array() }));
it("attention_jump selects the latest tab to enter attention through the App keyboard path", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  const { default: App } = await import("./App");
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const press = async (chord: string) => { await act(async () => { window.dispatchEvent(controlKey(chord)); }); };
  const frame = (a: string, b: string): Snapshot => ({ ...baseline, panes: [
    { pane_id: "p1", terminal_id: "t1", workspace_id: "a", tab_id: "a1", agent_status: a },
    { pane_id: "p2", terminal_id: "t2", workspace_id: "b", tab_id: "b1", agent_status: b },
  ] });
  try {
    await act(async () => { root.render(createElement(App)); });
    await press("ctrl+shift+2"); // Keep terminal rendering out of this pane-less selection.
    for (const snapshot of [frame("idle", "idle"), frame("done", "idle"), frame("done", "blocked")]) {
      await act(async () => { publishSnapshot?.({ machine: "studio", snapshot }); });
    }
    await press("ctrl+shift+o");
    expect(host.querySelector('[data-row="agent:b1"]')?.classList.contains("selected")).toBe(true);
    expect(host.querySelector('[data-row="agent:a1"]')?.classList.contains("selected")).toBe(false);
  } finally {
    act(() => root.unmount()); host.remove(); localStorage.clear();
  }
});
