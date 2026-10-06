import { bridge } from "./bridge";
import { prompt } from "./chatGuard";
import { getAgent } from "./chatTail";
import type { ChatItem } from "./transcript";
export interface SendState { text: string; status: string; warning: boolean; known: Set<string> }
const queues = new Map<string, Promise<void>>();
const wait = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms));
export class ChatSender {
  state: SendState = { text: "", status: "", warning: false, known: new Set() };
  private disposed = false;
  private generation = 0;
  constructor(private machine: string, private pane: string, private update: (state: SendState) => void) {}
  private publish(status: string, warning = false) { this.state = { ...this.state, status, warning }; if (!this.disposed) this.update(this.state); }
  dispose() { this.disposed = true; this.generation++; }
  cancel(): string { const text = this.state.text; this.generation++; this.state = { ...this.state, text: "" }; this.publish(""); return text; }
  send(text: string, items: ChatItem[], anyway = false): boolean {
    if (!text.trim() || Array.from(text).length > 20_000) { this.publish("Message must contain 1 to 20,000 characters"); return false; }
    if (this.state.text && !this.state.warning) return false;
    this.state = { text, status: "queued", warning: false, known: new Set(items.map(item => item.id)) };
    this.publish("queued");
    const generation = ++this.generation, key = `${this.machine}:${this.pane}`;
    const run = async () => {
      try {
        while (!this.disposed && generation === this.generation) {
          const agent = await getAgent(this.machine, this.pane);
          if (this.disposed || generation !== this.generation) return;
          if (agent.agent !== "claude" || !agent.agent_session?.value) { this.publish("No Claude session is running in that pane", true); return; }
          if (agent.agent_status !== "blocked") break;
          this.publish("Held: will send when the terminal stops asking"); await wait(3000);
        }
        if (this.disposed || generation !== this.generation) return;
        const screen = await bridge.api(this.machine, "pane.read", { pane_id: this.pane, source: "visible", lines: 12, format: "ansi", strip_ansi: false }) as { text: string };
        if (this.disposed || generation !== this.generation) return;
        const guard = prompt(screen.text);
        if (!anyway && guard !== "clear") { this.publish(guard === "draft" ? "There's unsent text in the terminal" : "Can't see the prompt; send anyway?", true); return; }
        this.publish("Sending");
        const points = Array.from(text);
        // Once sending begins, finish the submission even if its view is unmounted.
        for (let start = 0; start < points.length; start += 300) await bridge.api(this.machine, "pane.send_text", { pane_id: this.pane, text: points.slice(start, start + 300).join("") });
        await bridge.api(this.machine, "pane.send_text", { pane_id: this.pane, text: "\r" });
        this.publish("Waiting for transcript acknowledgement");
      } catch { this.publish("Send failed; delivery is uncertain", true); }
    };
    const queued = (queues.get(key) ?? Promise.resolve()).then(run);
    queues.set(key, queued);
    void queued.finally(() => { if (queues.get(key) === queued) queues.delete(key); });
    return true;
  }
  acknowledge(items: ChatItem[]) {
    if (!this.state.text || this.state.warning) return;
    if (items.some(item => item.kind === "user" && !this.state.known.has(item.id) && item.text.trim() === this.state.text.trim())) { this.state = { ...this.state, text: "" }; this.publish(""); }
  }
}
