import { bridge, fromBase64 } from "./bridge";
import { feed, transcriptPath } from "./transcript";
import type { ChatAgent, ChatItem } from "./transcript";
export const BLOCK = 2 * 1024 * 1024;
export async function getAgent(machine: string, pane: string): Promise<ChatAgent> {
  const result = await bridge.api(machine, "agent.get", { target: pane }) as { agent: ChatAgent };
  return result.agent;
}
/** All calls are serialized by the visible chat, including backward reads. */
export class ChatTail {
  items: ChatItem[] = [];
  agent: ChatAgent = {};
  earlier = false;
  waiting = false;
  private path = "";
  private offset = 0;
  private first = 0;
  private inode = -1;
  private mtime = -1;
  private limit = 400;
  private recent = "";
  private pending = new Uint8Array();
  constructor(private machine: string, private pane: string) {}
  private reset() { this.items = []; this.offset = 0; this.first = 0; this.inode = -1; this.mtime = -1; this.pending = new Uint8Array(); this.limit = 400; this.recent = ""; }
  async refresh() {
    this.agent = await getAgent(this.machine, this.pane);
    const path = transcriptPath(this.agent) ?? "";
    if (path !== this.path) { this.path = path; this.reset(); }
    if (!path) { this.waiting = false; this.earlier = false; return; }
    const stat = await bridge.fileStat(this.machine, path);
    if (!stat.exists) { this.reset(); this.waiting = true; this.earlier = false; return; }
    const reset = this.inode !== stat.inode || stat.size < this.offset || (stat.size === this.offset && stat.mtime_ms !== this.mtime);
    let dropPartial = false;
    if (reset) { this.reset(); this.offset = Math.max(0, stat.size - BLOCK); this.first = this.offset; dropPartial = this.offset > 0; }
    if (stat.size - this.offset > BLOCK) { this.offset = stat.size - BLOCK; this.pending = new Uint8Array(); dropPartial = true; }
    if (stat.size > this.offset) {
      const read = await bridge.fileRead(this.machine, path, this.offset, BLOCK);
      if (read.inode !== stat.inode || read.size < stat.size || read.offset !== this.offset) { this.reset(); return; }
      const bytes = fromBase64(read.data_b64);
      this.offset += bytes.length;
      let data = new Uint8Array(this.pending.length + bytes.length);
      data.set(this.pending); data.set(bytes, this.pending.length);
      if (dropPartial) { const first = data.indexOf(10); data = first < 0 ? new Uint8Array() : data.slice(first + 1); }
      const last = data.lastIndexOf(10);
      if (last >= 0) {
        const lines = new TextDecoder().decode(data.slice(0, last));
        this.items = feed(lines, this.items, this.agent.cwd, this.offset);
        this.recent = (this.recent + lines + "\n").slice(-BLOCK * 2);
        this.pending = data.slice(last + 1);
      }
      else this.pending = data;
      if (this.pending.length > BLOCK) this.pending = new Uint8Array();
      this.inode = read.inode; this.mtime = read.mtime_ms;
    } else { this.inode = stat.inode; this.mtime = stat.mtime_ms; }
    this.items = this.items.slice(-this.limit);
    this.waiting = false; this.earlier = this.first > 0;
  }
  async loadEarlier() {
    if (!this.path || !this.first) return;
    const end = this.first, start = Math.max(0, end - BLOCK);
    // Read forward until the boundary line ends; never ask the bridge for >2MiB.
    let data = new Uint8Array();
    let cursor = start;
    while (cursor <= end) {
      const read = await bridge.fileRead(this.machine, this.path, cursor, BLOCK);
      if (read.inode !== this.inode || read.size < this.offset || (read.size === this.offset && read.mtime_ms !== this.mtime)) { this.reset(); await this.refresh(); return; }
      const bytes = fromBase64(read.data_b64);
      if (!bytes.length) break;
      const joined = new Uint8Array(data.length + bytes.length); joined.set(data); joined.set(bytes, data.length); data = joined;
      cursor += bytes.length;
      const boundaryEnd = data.indexOf(10, end - start);
      if (boundaryEnd >= 0) { data = data.slice(0, boundaryEnd + 1); break; }
      if (data.length >= BLOCK * 2) { data = data.slice(0, end - start); break; }
    }
    if (start > 0) { const newline = data.indexOf(10); data = newline < 0 ? new Uint8Array() : data.slice(newline + 1); }
    let older = feed(new TextDecoder().decode(data), [], this.agent.cwd, start);
    const results = this.recent.split("\n").filter(line => { try { const row = JSON.parse(line); return row?.type === "user" && Array.isArray(row.message?.content) && row.message.content.some((block: { type?: string }) => block?.type === "tool_result"); } catch { return false; } });
    older = feed(results, older, this.agent.cwd, this.offset);
    const ids = new Set(older.map(item => item.id));
    const merged = [...older, ...this.items.filter(item => !ids.has(item.id))];
    // A result may be in the retained tail while its tool_use was in the older block.
    for (const row of merged) { const current = this.items.find(item => item.id === row.id); if (current) Object.assign(row, current); }
    this.limit += Math.max(0, merged.length - this.items.length);
    this.items = merged.slice(-this.limit); this.first = start; this.earlier = start > 0;
  }
}
