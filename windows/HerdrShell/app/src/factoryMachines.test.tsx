// @vitest-environment happy-dom
// R242 (verify-native 2026-10-13): the Factory view showed a load on 1 of 7 boxes.json boxes, and the overlay's
// "PC", "forge" and "Mac Studio" hosts as extra rows. Integration: the real Factory page over Studio's recorded
// factory files (fixtures/factory-boxes.json), fake native IPC only.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { bridge } from "./bridge";
import FactoryPage from "./FactoryView";
import type { FactoryBundle } from "./factory";
import recorded from "./fixtures/factory-boxes.json";
import base from "./fixtures/factory.json";

let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("draws one row per real machine, and every boxes.json box under a machine with its load", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const bundle: FactoryBundle = { ...base, overlay: recorded.overlay, boxes: recorded.boxes, machines: recorded.machines, disk: recorded.disk, poolState: {} };
  vi.spyOn(bridge, "factory").mockResolvedValue(bundle);
  const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
  dispose = () => { act(() => root.unmount()); host.remove(); };
  await act(async () => root.render(<FactoryPage machine="studio" online />));

  const rows = [...host.querySelectorAll<HTMLElement>("[data-machine]")];
  expect(rows.map(r => r.dataset.machine)).toEqual(["Mac Studio", "PC", "ax42", "forge"]);
  for (const box of recorded.boxes.boxes.map(b => b.name)) {
    const owners = rows.filter(r => r.dataset.machine === box || r.querySelector(`[data-box="${box}"]`) || r.querySelector(".factory-machine-box")?.textContent === box);
    expect(owners, box).toHaveLength(1);
    expect(owners[0].querySelector(".factory-usage-line")?.textContent, box).toMatch(/\d\.\d\d\/core/);
  }
  const forge = rows.find(r => r.dataset.machine === "forge")!;
  expect([...forge.querySelectorAll<HTMLElement>("[data-box]")].map(b => b.dataset.box)).toEqual(["forge-1", "forge-2", "forge-3", "forge-4", "forge-lanes"]);
  expect(forge.querySelector('[data-box="forge-lanes"]')?.textContent).toContain("held");
  expect(rows.find(r => r.dataset.machine === "PC")?.querySelector(".factory-machine-box")?.textContent).toBe("pc-wsl");
  // Disk watch files Studio as "studio"; the overlay calls it "Mac Studio".
  expect(rows.find(r => r.dataset.machine === "Mac Studio")?.querySelector(".factory-detail")?.lastElementChild?.textContent).toBe("ok");

  const stale = { ...bundle, overlay: { ...recorded.overlay, hosts: recorded.overlay.hosts.map(h => ({ ...h, usage: { ...h.usage, age_s: 600 } })) } };
  vi.spyOn(bridge, "factory").mockResolvedValue(stale);
  await act(async () => { await new Promise(r => setTimeout(r, 1100)); });
  for (const row of host.querySelectorAll<HTMLElement>("[data-machine]")) expect(row.querySelector(".factory-usage-line")?.textContent).toBe("no load reported");
});
