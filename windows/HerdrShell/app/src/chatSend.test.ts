import { afterEach, expect, it, vi } from "vitest";
import { bridge } from "./bridge";
import { ChatSender } from "./chatSend";
// Requested API-edge regression: tagged responses from response.rs must reach
// prompt guarding and delivery. Parser-only coverage cannot catch the wrapper;
// bridge.api is stubbed as requested, with no production test seam.
afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); });
it.each([false, true])("delivers tagged API responses (held=%s) and cancels a missing ack", async held => {
  vi.useFakeTimers();
  const sent: string[] = [];
  let reads = 0;
  vi.spyOn(bridge, "api").mockImplementation(async (_machine, method, params) => {
    if (method === "agent.get") return { type: "agent_info", agent: { agent: "claude", agent_session: { kind: "id", value: "session" }, agent_status: held && reads++ === 0 ? "blocked" : "idle" } };
    if (method === "pane.read") return { type: "pane_read", read: { text: "❯ \n────", pane_id: "pane", source: "visible", format: "ansi", revision: 1, truncated: false } };
    if (method === "pane.send_text") { sent.push((params as { text: string }).text); return { type: "ok" }; }
    throw new Error(`Unexpected method ${method}`);
  });
  const sender = new ChatSender("studio", `send-${held}`, () => {});
  const text = "😀".repeat(301);
  expect(sender.send(text, [])).toBe(true);
  await vi.advanceTimersByTimeAsync(held ? 3000 : 0);
  expect(sent).toEqual(["😀".repeat(300), "😀", "\r"]);
  expect(sender.state.status).toBe("Waiting for transcript acknowledgement");
  expect(sender.cancel()).toBe(text);
  expect(sender.state.text).toBe("");
  expect(sender.send("again", [])).toBe(true);
  await vi.advanceTimersByTimeAsync(0);
  sender.acknowledge([{ id: "new", kind: "user", text: "again", queued: false }]);
  expect(sender.state.text).toBe("");
  sender.dispose();
});

// The held-send state machine owns draft preservation across rejection, explicit
// override, and cancellation; exercise both terminal prompt warning outcomes.
it.each(["❯ existing draft\n────", "unrecognized prompt"])("preserves a held message when a new send is rejected (%s)", async screen => {
  vi.useFakeTimers();
  const sent: string[] = [];
  vi.spyOn(bridge, "api").mockImplementation(async (_machine, method, params) => {
    if (method === "agent.get") return { type: "agent_info", agent: { agent: "claude", agent_session: { kind: "id", value: "session" }, agent_status: "idle" } };
    if (method === "pane.read") return { type: "pane_read", read: { text: screen } };
    if (method === "pane.send_text") { sent.push((params as { text: string }).text); return { type: "ok" }; }
    throw new Error(`Unexpected method ${method}`);
  });
  const sender = new ChatSender("studio", `warning-${screen}`, () => {});
  expect(sender.send("held message", [])).toBe(true);
  await vi.advanceTimersByTimeAsync(0);
  expect(sender.state.warning).toBe(true);
  const warning = sender.state.status;
  expect(sender.send("new composer text", [])).toBe(false);
  expect(sender.state.text).toBe("held message");
  expect(sender.state.warning).toBe(true);
  expect(sender.state.status).toBe("A message is waiting; send or cancel it first");
  expect(sent).toEqual([]);
  await vi.advanceTimersByTimeAsync(3000);
  expect(sender.state.status).toBe(warning);
  expect(sender.state.text).toBe("held message");
  expect(sender.send("held message", [], true)).toBe(true);
  await vi.advanceTimersByTimeAsync(0);
  expect(sent).toEqual(["held message", "\r"]);
  expect(sender.cancel()).toBe("held message");
  sender.dispose();
});
