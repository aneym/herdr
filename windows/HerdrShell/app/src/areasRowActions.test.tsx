// @vitest-environment happy-dom
// Integration scenario: real sidebar and bridge, only native Tauri IPC is fake.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn(async (cmd: string) => cmd === "remote_action" ? { ok: true } : new Promise(() => {})));
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: class {} }));
import Sidebar from "./Sidebar";
import { LaneSnapshot } from "./laneFiles";
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const stored = new Map<string, string>();
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: (k: string) => stored.get(k) ?? null, setItem: (k: string, v: string) => void stored.set(k, v) } });
it("parks, resumes, approves and highlights Focus", async () => {
 const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
 stored.set("herdr-shell.areas.mode", JSON.stringify("areas"));
 const catalog = new LaneSnapshot(); catalog.hasFiles = true;
 catalog.lanes = { "w1:t1": { tab: "w1:t1", name: "Active lane", label: "", scopeURL: "http://studio/?route=scoping/test-scope" }, "w1:t2": { tab: "w1:t2", name: "Parked lane", label: "" } };
 catalog.parked = { "w1:t2": { note: "later" } };
 const noop = () => {};
 const snapshot = { workspaces: [{ workspace_id: "w1", number: 1 }], tabs: [{ tab_id: "w1:t1", workspace_id: "w1", number: 1 }, { tab_id: "w1:t2", workspace_id: "w1", number: 2 }] };
 act(() => root.render(<Sidebar snapshot={snapshot} catalog={catalog} machines={[]} chooseMachine={noop} rows={[]} selected={null} revealed={{ last: null, pending: null }} machine={{ name: "studio", state: "up" }} notice={null} select={noop} pin={noop} movePin={noop} renaming={null} startRename={noop} cancelRename={noop} commitRename={async () => {}} />));
 const click = async (text: string) => { const button = [...host.querySelectorAll("button")].find(b => b.textContent === text)!; expect(button).toBeTruthy(); await act(async () => button.click()); };
 const context = (title: string) => act(() => [...host.querySelectorAll(".areas-line")].find(b => b.textContent?.includes(title))!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true })));
 try {
 context("Active lane"); expect(host.querySelector('[role="menu"]')?.textContent).toContain("Park…"); expect(host.querySelector('[role="menu"]')?.textContent).toContain("Approve scope…");
 await click("Park…"); const input = host.querySelector<HTMLInputElement>('[role="dialog"] input')!;
 act(() => { const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!; setter.call(input, "  waiting on review  "); input.dispatchEvent(new Event("input", { bubbles: true })); });
 await click("Park"); expect(invoke).toHaveBeenCalledWith("remote_action", { machine: "studio", verb: "park", args: ["w1:t1", "--note=waiting on review"] });
 context("Active lane"); await click("Approve scope…"); await click("Approve"); expect(invoke).toHaveBeenCalledWith("remote_action", { machine: "studio", verb: "approve", args: ["test-scope", "--quote=approved", "--by=alex"] });
 await click("Parked 1"); context("Parked lane"); expect(host.querySelector('[role="menu"]')?.textContent).not.toContain("Park…"); await click("Resume"); expect(invoke).toHaveBeenCalledWith("remote_action", { machine: "studio", verb: "unpark", args: ["w1:t2"] });
 await click("All"); await click("Focus0"); expect(host.querySelector('[data-row="focus"]')?.classList.contains("selected")).toBe(true);
 } finally { act(() => root.unmount()); host.remove(); stored.clear(); }
});
