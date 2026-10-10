// @vitest-environment happy-dom
// Mac parity (Sidebar.swift spacesFooter, SpacesTree footerHost): Spaces mode ends with one row per
// host from the overlay, its running count and free memory, "load " and " live" dropped. The usage
// line (footerUsage) is on hold and must not draw.
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildSidebar } from "./model";
import type { Snapshot } from "./model";
import type { OverlayHost } from "./spacesOverlay";
vi.mock("./bridge", () => ({ bridge: { api: async () => ({}), updateStatus: () => new Promise(() => {}), fileList: async () => [], fileRead: async () => ({ data_b64: "" }) }, fromBase64: () => new Uint8Array() }));
const { default: Sidebar, useSelectionReveal } = await import("./Sidebar");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => void stored.set(k, v), clear: () => stored.clear() } });

const snapshot: Snapshot = { workspaces: [{ workspace_id: "w1", number: 1, label: "rails" }], tabs: [{ tab_id: "a", workspace_id: "w1", number: 1, label: "lane" }] };
const hosts: OverlayHost[] = [
  { name: "Studio", summary: "11 running · 0.1G free · load 3.2", attention: "none" },
  { name: "pc", summary: "0 running · stopped", attention: "warn" },
  { name: "ax42", summary: "4 running · 24G free · 3/28 live", attention: "none" },
];

describe("sidebar host rows", () => {
  let host: HTMLDivElement, root: Root;
  function Shell() {
    const revealed = useSelectionReveal(null);
    const noop = () => {};
    return <Sidebar snapshot={snapshot} hosts={hosts} rows={buildSidebar(snapshot)} selected={null} revealed={revealed} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />;
  }
  beforeEach(() => { stored.clear(); host = document.createElement("div"); document.body.append(host); root = createRoot(host); });
  afterEach(() => { act(() => root.unmount()); host.remove(); });

  it("draws one row per host in Spaces mode, below the tree", () => {
    stored.set("herdr-shell.areas.mode", JSON.stringify("spaces"));
    act(() => root.render(<Shell />));
    const rows = [...host.querySelectorAll('[aria-label="Hosts"] [data-row^="host:"]')];
    expect(rows.map(r => [r.querySelector(".host-name")?.textContent, r.querySelector(".host-summary")?.textContent])).toEqual([
      ["Studio", "11 running 0.1G free 3.2"], ["pc", "0 running stopped"], ["ax42", "4 running 24G free 3/28"],
    ]);
    expect(rows[1].querySelector(".host-alert.warn")?.textContent).toBe("!");
    expect(host.querySelector("nav")!.contains(rows[0])).toBe(false);
    expect(host.textContent).not.toMatch(/claude|codex/);
  });

  it("draws no host rows in Areas mode", () => {
    stored.set("herdr-shell.areas.mode", JSON.stringify("areas"));
    act(() => root.render(<Shell />));
    expect(host.querySelector('[aria-label="Hosts"]')).toBeNull();
  });
});
