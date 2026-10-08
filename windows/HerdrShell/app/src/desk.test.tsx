// @vitest-environment happy-dom
// Integration scenario: real React desk, xterm OSC 8 links and server snapshot data.
// Only Tauri's invoke boundary is faked; no desk or link collaborator is mocked.
import React, { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { Terminal } from "@xterm/xterm";
import { afterEach, expect, it, vi } from "vitest";
import { bridge } from "./bridge";
import DocPanel from "./DocPanel";
import { useDesk, docItems, parseCatalog } from "./docs";
import { installLinks } from "./links";
import type { Snapshot } from "./model";
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, Channel: class {} }));
let snapshot: Snapshot;
const desk = (id: string, title: string) => ({ items: [{ id, kind: "url", ref: `https://example.com/${id}`, title, mime: "text/html", opened_by: "agent", opened_at_ms: 1 }], front: id });
const tab = (id: string, value: ReturnType<typeof desk>) => ({ tab_id: id, workspace_id: "w1", number: 1, desk: value });
let refresh: () => Promise<void>;
function Scenario() {
  const [value, setValue] = useState<Snapshot>({ tabs: [] });
  const [selected, select] = useState("t1");
  const [shown, setShown] = useState(false);
  refresh = async () => setValue(await bridge.snapshot("pc"));
  const docs = useDesk("pc", selected, value, [], () => setShown(true));
  return <><button onClick={() => select("t1")}>Tab one</button><button onClick={() => select("t2")}>Tab two</button><output>{String(shown)}</output><DocPanel machine="pc" tab={selected} items={docs.items} active={docs.active} select={docs.select} error="" /></>;
}
const cleanup: (() => void)[] = [];
afterEach(() => { cleanup.splice(0).forEach(fn => fn()); document.body.innerHTML = ""; vi.clearAllMocks(); });
it("keeps each tab's server-owned desk and fronts a new arrival", async () => {
  native.invoke.mockImplementation(async (cmd: string) => { if (cmd === "snapshot") return snapshot; throw new Error(`Unexpected native command ${cmd}`); });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); cleanup.push(() => { act(() => root.unmount()); });
  await act(async () => root.render(<Scenario />));
  snapshot = { tabs: [tab("t1", desk("d1", "One")), tab("t2", desk("d2", "Two"))] };
  await act(async () => refresh());
  expect(host.querySelector('[aria-selected="true"]')?.textContent).toBe("One");
  await act(async () => (Array.from(host.querySelectorAll("button")).find(b => b.textContent === "Tab two")!).click());
  expect(host.querySelector('[aria-selected="true"]')?.textContent).toBe("Two");
  await act(async () => (Array.from(host.querySelectorAll("button")).find(b => b.textContent === "Tab one")!).click());
  expect(host.querySelector('[aria-selected="true"]')?.textContent).toBe("One");
  const next = desk("d3", "New arrival"); next.items.unshift(...desk("d1", "One").items);
  snapshot = { tabs: [tab("t1", next), tab("t2", desk("d2", "Two"))] };
  await act(async () => refresh());
  expect(host.querySelector('[aria-selected="true"]')?.textContent).toBe("New arrival");
  expect(host.querySelector("output")?.textContent).toBe("true");
});
it("routes terminal Ctrl links to desk.open, Shift externally, and mailto externally", async () => {
  native.invoke.mockResolvedValue({ handled: false });
  const host = document.createElement("main"); host.dataset.deskMachine = "pc"; host.dataset.deskTab = "t1";
  const pane = document.createElement("div"); pane.dataset.pane = "p1"; host.append(pane); document.body.append(host);
  const term = new Terminal({ cols: 40, rows: 4, allowProposedApi: true }); term.open(pane);
  const gate = installLinks(term, { resolve: async () => null, activate: async () => ({ handled: false }) }, url => { void bridge.openUrl(url); });
  cleanup.push(() => { gate.dispose(); term.dispose(); });
  const screen = term.element!.querySelector(".xterm-screen")!;
  Object.defineProperty(screen, "getBoundingClientRect", { value: () => ({ left: 0, top: 0, width: 400, height: 80 }) });
  for (const [url, shift] of [["https://example.com/desk", false], ["https://example.com/out", true], ["mailto:test@example.com", false]] as const) {
    await new Promise<void>(done => term.write(`\x1b[1;1H\x1b]8;;${url}\x1b\\link\x1b]8;;\x1b\\`, done));
    const event = (type: string) => new MouseEvent(type, { ctrlKey: true, shiftKey: shift, button: 0, clientX: 15, clientY: 10, bubbles: true });
    screen.dispatchEvent(event("mousedown"));
    const up = event("mouseup"); term.options.linkHandler!.activate(up, url, { start: { x: 1, y: 1 }, end: { x: 4, y: 1 } }); screen.dispatchEvent(up);
    await new Promise(done => setTimeout(done, 0));
  }
  expect(native.invoke).toHaveBeenCalledWith("api_request", { machine: "pc", method: "desk.open", params: { pane_id: "p1", ref: "https://example.com/desk", opened_by: "user" } });
  expect(native.invoke).toHaveBeenCalledWith("open_url", { url: "https://example.com/out" });
  expect(native.invoke).toHaveBeenCalledWith("open_url", { url: "mailto:test@example.com" });
  expect(native.invoke.mock.calls.filter(([cmd]) => cmd === "api_request")).toHaveLength(1);
});

// Catalog input is an untrusted machine boundary, not an internal collaborator.
for (const [url, allowed] of [["javascript:window.__unsafe=1", false], ["https://example.com/safe", true]] as const) {
  it(`frames only web catalog scope URLs: ${url}`, async () => {
    const catalog = parseCatalog(JSON.stringify({ lanes: [{ tab: "t1", scope_url: url }] }));
    const items = docItems(catalog.lanes.t1, new Set());
    const host = document.createElement("div"); document.body.append(host);
    const root = createRoot(host); cleanup.push(() => act(() => root.unmount()));
    await act(async () => root.render(<DocPanel machine="pc" tab="t1" items={items} active="Scope" select={() => {}} error="" />));
    expect(host.querySelector("iframe")?.getAttribute("src") ?? null).toBe(allowed ? url : null);
    expect(host.querySelector("button.muted") !== null).toBe(allowed);
  });
}
