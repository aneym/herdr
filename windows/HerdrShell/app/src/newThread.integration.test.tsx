// @vitest-environment happy-dom
// Integration: a new thread stays with the client that made it. Two real PaneTerm clients on
// one terminal; only native IPC is faked, by a stand-in for the herdr server's attach rules
// (one writable owner per terminal, a plain attach refused while one exists, --takeover shuts
// the old owner down with "terminal attach taken over"). Alex, 2026-10-11: "new threads should
// be focused on the client that made them when they're first made."
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import PaneTerm, { type PaneController } from "./PaneTerm";
import { bridge, fromBase64, type AttachEvent } from "./bridge";
import type { Pane, Snapshot } from "./model";
import { newPanes } from "./newPanes";

const pane: Pane = { pane_id: "p", terminal_id: "term", tab_id: "t", workspace_id: "w" };
const before = { panes: [{ ...pane, pane_id: "old" }] } as unknown as Snapshot;
const after = { panes: [{ ...pane, pane_id: "old" }, pane] } as unknown as Snapshot;
type Conn = { client: string; fn: (event: AttachEvent) => void; writable: boolean };
let server: { conns: Map<number, Conn>; owner: number | null; next: number; typed: string[] };
const roots: Root[] = [];
const controllers = new Map<string, PaneController>();

function mount(client: string) {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); roots.push(root);
  const register = (_id: string, value: PaneController | null) => { if (value) controllers.set(client, value); else controllers.delete(client); };
  return act(async () => root.render(<PaneTerm pane={pane} machine={client} focused onFocus={() => {}} shortcut={() => false} register={register} />));
}
const until = async (ok: () => boolean, ms: number) => { const end = Date.now() + ms; while (!ok() && Date.now() < end) await act(async () => { await new Promise(r => setTimeout(r, 10)); }); return ok(); };
const ownerClient = () => server.owner == null ? null : server.conns.get(server.owner)!.client;
const mode = (client: string) => controllers.get(client)?.info().mode;

beforeEach(() => {
  server = { conns: new Map(), owner: null, next: 1, typed: [] };
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }));
  vi.stubGlobal("requestAnimationFrame", () => 0);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.spyOn(bridge, "attach").mockImplementation(async (client, _term, _c, _r, attachMode, fn) => {
    if (attachMode === "attach" && server.owner != null) throw new Error("terminal attach failed: terminal term already has an attached client; retry with --takeover");
    const id = server.next++;
    server.conns.set(id, { client, fn, writable: attachMode !== "observe" });
    if (attachMode !== "observe") {
      const old = server.owner;
      if (old != null) { server.conns.get(old)!.fn({ kind: "closed", reason: "terminal attach taken over" }); server.conns.delete(old); }
      server.owner = id;
    }
    return id;
  });
  vi.spyOn(bridge, "input").mockImplementation(async (handle, b64) => {
    if (handle !== server.owner) throw new Error("not the controller");
    server.typed.push(`${server.conns.get(handle)!.client}:${new TextDecoder().decode(fromBase64(b64))}`);
  });
  vi.spyOn(bridge, "api").mockResolvedValue({ process_info: { human_input_age_ms: null } });
  vi.spyOn(bridge, "takeControl").mockResolvedValue();
  vi.spyOn(bridge, "resize").mockResolvedValue();
  vi.spyOn(bridge, "close").mockImplementation(async handle => { server.conns.delete(handle); if (server.owner === handle) server.owner = null; });
});
afterEach(() => { for (const r of roots.splice(0)) act(() => r.unmount()); document.body.innerHTML = ""; vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("the client that made a new thread controls it; another client that sees it does not take it", async () => {
  // Both clients were running before the thread existed.
  newPanes.observe("Book", before); newPanes.observe("Studio", before);
  newPanes.markMine("Book", "p");
  newPanes.observe("Book", after); newPanes.observe("Studio", after);
  // Studio, idle, attaches the new pane first; Book, its maker, still gets it at once.
  await mount("Studio");
  expect(await until(() => ownerClient() === "Studio", 2000)).toBe(true);
  await mount("Book");
  expect(await until(() => ownerClient() === "Book" && mode("Book") === "attach", 2000)).toBe(true);
  await act(async () => { await controllers.get("Book")!.type("hi"); });
  expect(server.typed).toEqual(["Book:hi"]);
  // Studio sees the pane again (a re-render, a reconnect): it observes and leaves Book in control.
  for (const r of roots.splice(0, 1)) act(() => r.unmount());
  await mount("Studio");
  expect(await until(() => mode("Studio") === "observe", 2000)).toBe(true);
  expect(ownerClient()).toBe("Book");
  expect(bridge.takeControl).not.toHaveBeenCalled();
});

it("a pane that was there when the client started keeps the take-over on open", async () => {
  newPanes.observe("PC", after);
  await mount("Book2");
  expect(await until(() => ownerClient() === "Book2", 2000)).toBe(true);
  await mount("PC");
  expect(await until(() => ownerClient() === "PC" && mode("PC") === "attach", 2000)).toBe(true);
});
