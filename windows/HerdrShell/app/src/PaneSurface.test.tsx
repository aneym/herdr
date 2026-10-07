// @vitest-environment happy-dom
// DOM-to-native API boundary: a menu click must send the pane's identity, with force only after consent.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
const api = vi.fn();
vi.mock("./bridge", () => ({ bridge: { api: (...args: unknown[]) => api(...args) } }));
// Terminal and transcript I/O are outside this header interaction contract.
vi.mock("./PaneTerm", () => ({ default: () => null }));
vi.mock("./Chat", () => ({ default: () => null }));
const { default: PaneSurface } = await import("./PaneSurface");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
Object.defineProperty(globalThis, "localStorage", { configurable: true, value: { getItem: () => null, setItem: () => {} } });
let dispose = () => {};
afterEach(() => { dispose(); api.mockReset(); });
function mount(hasAgent = true) {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); const onError = vi.fn();
  act(() => root.render(<PaneSurface pane={{ pane_id: "pane_1", terminal_id: "term_1", tab_id: "tab_1", workspace_id: "ws_1" }} machine="studio" focused hasAgent={hasAgent} onFocus={() => {}} shortcut={() => false} register={() => {}} onError={onError} />));
  dispose = () => { act(() => root.unmount()); host.remove(); };
  const click = async (selector: string) => { const button = host.querySelector<HTMLButtonElement>(selector); expect(button, selector).not.toBeNull(); await act(async () => { button!.click(); }); };
  return { host, click, onError };
}
it("opens the pane menu and disables restart without an agent", async () => {
  const { host, click } = mount(false);
  await click('[aria-label="Pane actions"]');
  expect(host.querySelector('[role="menuitem"]')?.textContent).toContain("Restart agent");
  expect((host.querySelector('[role="menuitem"]') as HTMLButtonElement).disabled).toBe(true);
  expect(api).not.toHaveBeenCalled();
});
it("restarts the same pane and only forces after busy confirmation", async () => {
  api.mockRejectedValueOnce("herdr api error busy: Agent is working").mockResolvedValueOnce({ ok: true });
  const { host, click } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  expect(api.mock.calls).toEqual([["studio", "agent.restart", { pane_id: "pane_1" }]]);
  expect(host.querySelector('[role="dialog"]')?.textContent).toContain("It will resume the same chat.");
  await click('[role="dialog"] button');
  expect(api.mock.calls[1]).toEqual(["studio", "agent.restart", { pane_id: "pane_1", force: true }]);
  expect(host.querySelector('[role="dialog"]')).toBeNull();
});
it("reports non-busy errors through the existing shell notice", async () => {
  api.mockRejectedValueOnce("herdr api error not_resumable: No resumable session");
  const { click, onError } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  expect(onError).toHaveBeenCalledWith("herdr api error not_resumable: No resumable session");
});
it("canceling busy confirmation never sends a forced restart", async () => {
  api.mockRejectedValueOnce({ code: "busy", message: "Working" });
  const { host, click } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  await click('[role="dialog"] button:last-child');
  expect(api).toHaveBeenCalledTimes(1);
  expect(host.querySelector('[role="dialog"]')).toBeNull();
});
