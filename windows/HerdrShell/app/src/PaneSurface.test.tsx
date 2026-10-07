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
function mount(hasAgent = true, restoreError?: string) {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); const onError = vi.fn();
  const render = (restoreError?: string, machine = "studio", agentStatus?: string) => act(() => root.render(<PaneSurface pane={{ restore_error: restoreError, agent_status: agentStatus, pane_id: "pane_1", terminal_id: "term_1", tab_id: "tab_1", workspace_id: "ws_1" }} machine={machine} focused hasAgent={hasAgent} onFocus={() => {}} shortcut={() => false} register={() => {}} onError={onError} />));
  render(restoreError);
  dispose = () => { act(() => root.unmount()); host.remove(); };
  const click = async (selector: string) => { const button = host.querySelector<HTMLButtonElement>(selector); expect(button, selector).not.toBeNull(); await act(async () => { button!.click(); }); };
  return { host, click, onError, render };
}
it("opens the pane menu and disables restart without an agent", async () => {
  const { host, click } = mount(false);
  await click('[aria-label="Pane actions"]');
  expect(host.querySelector('[role="menuitem"]')?.textContent).toContain("Restart agent");
  expect((host.querySelector('[role="menuitem"]') as HTMLButtonElement).disabled).toBe(true);
  expect(api).not.toHaveBeenCalled();
});
it("restarts the same pane and only forces after busy confirmation", async () => {
  api.mockRejectedValueOnce("herdr api error busy: agent in pane pane_1 is Working").mockResolvedValueOnce({ ok: true });
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
  const { host, click, onError } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  expect(host.querySelector('[role="dialog"]')?.textContent).toContain("This agent can't be resumed: no saved chat found.");
  expect(onError).not.toHaveBeenCalled();
});
it("canceling busy confirmation never sends a forced restart", async () => {
  api.mockRejectedValueOnce({ code: "busy", reason: "working", message: "server wording changed" });
  const { host, click } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  await click('[role="dialog"] button:last-child');
  expect(api).toHaveBeenCalledTimes(1);
  expect(host.querySelector('[role="dialog"]')).toBeNull();
});

it("busy confirmation uses Enter to restart and Escape to cancel", async () => {
  api.mockRejectedValueOnce({ code: "busy", reason: "working", message: "server wording changed" }).mockResolvedValueOnce({ ok: true });
  const { host, click } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  await act(async () => { host.querySelector('[role="dialog"]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })); });
  expect(api).toHaveBeenCalledTimes(1);
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  // Reopen with another busy reply, then confirm through the focused primary action.
  api.mockReset(); api.mockRejectedValueOnce({ code: "busy", reason: "working", message: "server wording changed" }).mockResolvedValueOnce({ ok: true });
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  await act(async () => { host.querySelector('[role="dialog"]')!.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true })); });
  expect(api.mock.calls[1]).toEqual(["studio", "agent.restart", { pane_id: "pane_1", force: true }]);
});

it("does not alert on a pre-existing restore failure at mount", () => {
  const { onError } = mount(true, "start_failed: could not resume agent");
  expect(onError).not.toHaveBeenCalled();
});

it.each([
  ["blocked", "agent in pane pane_1 is Working", "This agent is blocked. Resolve its prompt before restarting."],
  ["restart_pending", "server wording changed", "This agent is already restarting. Wait for it to finish."],
  [undefined, "agent in pane pane_1 is Blocked", "This agent is blocked. Resolve its prompt before restarting."],
  ["unknown", "agent in pane pane_1 is Working", "This agent can't restart right now"],
  [undefined, "agent in pane pane_1 is Unknown", "This agent can't restart right now"],
  [undefined, "previous restart is still completing", "This agent is already restarting. Wait for it to finish."],
])("does not offer force for non-working busy: %s", async (reason, message, notice) => {
  api.mockRejectedValueOnce({ code: "busy", reason, message });
  const { host, click, onError } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  expect(host.querySelector('[role="dialog"]')?.textContent).toContain(notice);
  expect(host.querySelector('[role="dialog"] button')?.textContent).toBe("OK");
  expect(onError).not.toHaveBeenCalled();
  expect(api).toHaveBeenCalledTimes(1);
});

it("alerts once when an accepted restart later fails to start", async () => {
  api.mockResolvedValueOnce({ ok: true });
  const { host, click, render } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  render("start_failed: could not resume agent");
  expect(host.querySelector('[role="dialog"]')?.textContent).toContain("The agent didn't come back up.");
  await click('[role="dialog"] button');
  render("start_failed: another failure");
  expect(host.querySelector('[role="dialog"]')).toBeNull();
});

it("clears accepted restart tracking when the agent is running again", async () => {
  api.mockResolvedValueOnce({ ok: true });
  const { host, click, render } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  render(undefined, "studio", "idle");
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  render("start_failed: unrelated later error", "studio", "idle");
  expect(host.querySelector('[role="dialog"]')).toBeNull();
});

it("expires accepted restart tracking after 30 seconds without an error", async () => {
  vi.useFakeTimers();
  try {
    api.mockResolvedValueOnce({ ok: true });
    const { host, click, render } = mount();
    await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
    await act(async () => { vi.advanceTimersByTime(30_000); });
    render("start_failed: unrelated later error");
    expect(host.querySelector('[role="dialog"]')).toBeNull();
  } finally { vi.useRealTimers(); }
});

it("dismisses pane-local restart errors on OK and outside click", async () => {
  api.mockRejectedValue({ code: "busy", reason: "blocked", message: "blocked" });
  const { host, click } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  await click('[role="dialog"] button');
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  await act(async () => { document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true })); });
  expect(host.querySelector('[role="dialog"]')).toBeNull();
});
it("does not carry pending restart tracking across machines with the same pane id", async () => {
  let finish!: (value: unknown) => void;
  api.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const { host, click, render, onError } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  render("other machine error", "book");
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  expect(onError).not.toHaveBeenCalled();
  await act(async () => { finish({ ok: true }); });
});

it("ignores a late restart error after moving to another machine", async () => {
  let fail!: (error: unknown) => void;
  api.mockImplementationOnce(() => new Promise((_, reject) => { fail = reject; }));
  const { host, click, render } = mount();
  await click('[aria-label="Pane actions"]'); await click('[role="menuitem"]');
  render(undefined, "book");
  await act(async () => { fail({ code: "busy", reason: "blocked" }); });
  expect(host.querySelector('[role="dialog"]')).toBeNull();
});
