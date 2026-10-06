import { afterEach, expect, it, vi } from "vitest";
import { bridge, toBase64 } from "./bridge";
import { ChatTail, BLOCK } from "./chatTail";
// File-edge contract tests: the real reader consumes stat/read receipts and
// tagged agent responses. Existing feed goldens cannot detect stale offsets or
// UTF-8 boundary corruption. Only the remote bridge boundary is stubbed;
// no test-only production seam is added.
afterEach(() => vi.restoreAllMocks());
const line = (id: string, text: string) => JSON.stringify({ type: "user", uuid: id, origin: { kind: "human" }, message: { content: text } }) + "\n";
function remote(initial: string) {
  let bytes = new TextEncoder().encode(initial), inode = 1, mtime = 1;
  const stat = () => ({ exists: true, size: bytes.length, inode, mtime_ms: mtime });
  vi.spyOn(bridge, "api").mockResolvedValue({ type: "agent_info", agent: { agent: "claude", cwd: "/repo", agent_session: { kind: "path", value: "~/.claude/projects/test.jsonl" } } });
  vi.spyOn(bridge, "fileStat").mockImplementation(async () => stat());
  vi.spyOn(bridge, "fileRead").mockImplementation(async (_machine, _path, offset, max) => ({ ...stat(), offset, data_b64: toBase64(bytes.slice(offset, offset + max)) }));
  return { replace: (text: string, nextInode: number, nextMtime: number) => { bytes = new TextEncoder().encode(text); inode = nextInode; mtime = nextMtime; }, raw: (next: Uint8Array) => { bytes = new Uint8Array(next); mtime++; } };
}
it.each(["inode", "shrink", "mtime"])("replaces stale rows after %s reset", async reset => {
  const old = line("old", "old message"), next = line("new", reset === "shrink" ? "x" : "new message");
  const file = remote(old), tail = new ChatTail("studio", "pane");
  await tail.refresh();
  expect(tail.items.map(row => row.text)).toEqual(["old message"]);
  if (reset === "mtime") expect(new TextEncoder().encode(next).length).toBe(new TextEncoder().encode(old).length);
  file.replace(next, reset === "inode" ? 2 : 1, reset === "mtime" ? 2 : 1);
  await tail.refresh();
  expect(tail.items.map(row => [row.id, row.text])).toEqual([["new", reset === "shrink" ? "x" : "new message"]]);
});
it("joins an incomplete JSONL record and UTF-8 scalar across two reads", async () => {
  const record = new TextEncoder().encode(line("split", "hello 😀"));
  const split = record.indexOf(0xf0) + 2;
  const file = remote(""); file.raw(record.slice(0, split));
  const tail = new ChatTail("studio", "pane");
  await tail.refresh(); expect(tail.items).toEqual([]);
  file.raw(record); await tail.refresh();
  expect(tail.items.map(row => [row.id, row.text])).toEqual([["split", "hello 😀"]]);
});
it("drops the initial partial line and loads the earlier boundary without duplicates", async () => {
  const file = remote(line("older", "before") + " ".repeat(BLOCK) + "\n" + line("newer", "after"));
  const tail = new ChatTail("studio", "pane");
  await tail.refresh();
  expect(tail.items.map(row => row.id)).toEqual(["newer"]); expect(tail.earlier).toBe(true);
  await tail.loadEarlier();
  expect(tail.items.map(row => row.id)).toEqual(["older", "newer"]); expect(tail.earlier).toBe(false);
  file.replace(line("fresh", "replacement"), 2, 2);
  await tail.refresh(); expect(tail.items.map(row => row.id)).toEqual(["fresh"]);
});
