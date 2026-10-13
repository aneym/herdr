// @vitest-environment happy-dom
// Integration: two real PaneTerm clients on one terminal; only native IPC is faked, by a
// stand-in for the herdr server's attach rules (one writable owner per terminal, a plain
// attach refused while one exists, --takeover shuts the old owner down with "terminal attach
// taken over", pane.process_info reports human_input_age_ms across all clients).
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import PaneTerm, { type PaneController } from "./PaneTerm";
import { bridge, fromBase64, type AttachEvent } from "./bridge";
import type { Pane } from "./model";
import { SETTLE_MS } from "./autoControl";

const pane: Pane = { pane_id: "p", terminal_id: "term", tab_id: "t", workspace_id: "w" };
type Conn = { client: string; fn: (event: AttachEvent) => void; writable: boolean };
let server: { conns: Map<number, Conn>; owner: number | null; next: number; typed: { client: string; text: string; at: number }[]; lastInput: number | null };
const roots: Root[] = [];
const controllers = new Map<string, PaneController>();

function mount(client: string) {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); roots.push(root);
  const register = (_id: string, value: PaneController | null) => { if (value) controllers.set(client, value); else controllers.delete(client); };
  return act(async () => root.render(<PaneTerm pane={pane} machine={client} focused onFocus={() => {}} shortcut={() => false} register={register} />));
}
const typed = () => server.typed.map(t => `${t.client}:${t.text}`).join("");
const until = async (ok: () => boolean, ms: number) => { const end = Date.now() + ms; while (!ok() && Date.now() < end) await act(async () => { await new Promise(r => setTimeout(r, 10)); }); return ok(); };
const ownerClient = () => server.owner == null ? null : server.conns.get(server.owner)!.client;

beforeEach(() => {
  server = { conns: new Map(), owner: null, next: 1, typed: [], lastInput: null };
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }));
  vi.stubGlobal("requestAnimationFrame", () => 0);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.spyOn(bridge, "attach").mockImplementation(async (client, _term, _c, _r, mode, fn) => {
    if (mode === "attach" && server.owner != null) throw new Error("terminal attach failed: terminal term already has an attached client; retry with --takeover");
    const id = server.next++;
    server.conns.set(id, { client, fn, writable: mode !== "observe" });
    if (mode !== "observe") {
      const old = server.owner;
      if (old != null) { server.conns.get(old)!.fn({ kind: "closed", reason: "terminal attach taken over" }); server.conns.delete(old); }
      server.owner = id;
    }
    return id;
  });
  vi.spyOn(bridge, "input").mockImplementation(async (handle, b64) => {
    if (handle !== server.owner) throw new Error("not the controller");
    server.typed.push({ client: server.conns.get(handle)!.client, text: new TextDecoder().decode(fromBase64(b64)), at: Date.now() });
    server.lastInput = Date.now();
  });
  vi.spyOn(bridge, "api").mockImplementation(async (_m, method) => {
    expect(method).toBe("pane.process_info");
    return { process_info: { human_input_age_ms: server.lastInput == null ? null : Date.now() - server.lastInput } };
  });
  vi.spyOn(bridge, "takeControl").mockResolvedValue();
  vi.spyOn(bridge, "resize").mockResolvedValue();
  vi.spyOn(bridge, "close").mockImplementation(async handle => { server.conns.delete(handle); if (server.owner === handle) server.owner = null; });
});
afterEach(() => { for (const r of roots.splice(0)) act(() => r.unmount()); document.body.innerHTML = ""; vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("two clients type in turn and each takes the terminal by typing, waiting out a holder that is still typing", async () => {
  await mount("A");
  expect(await until(() => ownerClient() === "A", 2000)).toBe(true);
  await act(async () => { await controllers.get("A")!.type("a1;"); });
  // B opens the pane: the plain attach is refused, so it takes it over, and A is told so.
  await mount("B");
  expect(await until(() => ownerClient() === "B" && controllers.get("A")!.info().mode === "closed", 2000)).toBe(true);
  await act(async () => { await controllers.get("B")!.type("b1;"); });
  const bTyped = Date.now();
  // A types at once, mid B's turn: its keys wait, B keeps the terminal through the settle window.
  void controllers.get("A")!.type("a2;"); void controllers.get("A")!.type("a3;");
  await until(() => false, SETTLE_MS - 400);
  expect(ownerClient()).toBe("B");
  expect(typed()).toBe("A:a1;B:b1;");
  // Once B has been quiet for the settle window A takes it and its queued keys land in order.
  expect(await until(() => typed() === "A:a1;B:b1;A:a2;A:a3;", 2500)).toBe(true);
  expect(server.typed[server.typed.length - 2].at - bTyped).toBeGreaterThanOrEqual(SETTLE_MS);
  expect(ownerClient()).toBe("A");
  expect(controllers.get("B")!.info().mode).toBe("closed");
  // B's turn again, no menu: its first key queues, waits out A, then lands.
  void controllers.get("B")!.type("b2;");
  expect(await until(() => typed().endsWith("B:b2;"), 3000)).toBe(true);
  expect(ownerClient()).toBe("B");
  expect(bridge.takeControl).not.toHaveBeenCalled();
}, 15000);

it("an observing client takes control on input once the holder is quiet", async () => {
  server.lastInput = null;
  vi.mocked(bridge.attach).mockImplementationOnce(async () => { throw new Error("terminal attach failed: terminal term has a read in progress; retry"); });
  await mount("A");
  expect(await until(() => controllers.get("A")?.info().mode === "observe", 2000)).toBe(true);
  const conn = [...server.conns.entries()].find(([, c]) => c.client === "A")!;
  vi.mocked(bridge.takeControl).mockImplementation(async handle => { server.owner = handle; conn[1].writable = true; });
  await act(async () => { await controllers.get("A")!.type("x"); });
  expect(bridge.takeControl).toHaveBeenCalledWith(conn[0]);
  expect(controllers.get("A")!.info().mode).toBe("attach");
  expect(typed()).toBe("A:x");
});

it("a test window observes a terminal another client holds and never takes it, even when typed into", async () => {
  await mount("A");
  expect(await until(() => ownerClient() === "A", 2000)).toBe(true);
  vi.stubGlobal("__HERDR_TEST_WINDOW__", true);
  await mount("T");
  expect(await until(() => controllers.get("T")?.info().mode === "observe", 2000)).toBe(true);
  void controllers.get("T")!.type("t1;");
  await until(() => false, SETTLE_MS + 300);
  expect(ownerClient()).toBe("A");
  expect(controllers.get("A")!.info().mode).toBe("attach");
  expect(controllers.get("T")!.info().mode).toBe("observe");
  expect(typed()).toBe("");
});
