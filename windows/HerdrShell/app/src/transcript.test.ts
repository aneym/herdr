import { describe, expect, it } from "vitest";
import fixture from "./fixtures/chat.jsonl?raw";
import expected from "./fixtures/chat.expected.json";
import { feed, transcriptPath, duration } from "./transcript";
// Parser goldens protect the JSONL display contract (not Swift source structure).
// New parser: no prior coverage. Expected rows are handwritten, with no test-only seams.
describe("transcript record golden", () => {
  it("renders human, queued, tool, duration and bulletin records without thinking or sidechains", () => {
    expect(feed(fixture, [], "/repo")).toEqual(expected);
    const lines = fixture.trim().split("\n");
    expect(feed(lines.slice(3), feed(lines.slice(0, 3), [], "/repo"), "/repo")).toEqual(expected);
  });
  it("caps scalar summaries/results and skips metadata, malformed records and non-human users", () => {
    const assistant = JSON.stringify({ type: "assistant", message: { content: [{ type: "tool_use", id: "t", name: "Bash", input: { command: "😀".repeat(90) + "\nextra" } }] } });
    const result = JSON.stringify({ type: "user", message: { content: [{ type: "tool_result", tool_use_id: "t", content: "😀".repeat(4200) }] } });
    const skipped = ["isMeta", "isCompactSummary"].map(flag => JSON.stringify({ type: "user", [flag]: true, origin: { kind: "human" }, message: { content: "skip" } }));
    const rows = feed([assistant, result, ...skipped, "broken", "null", JSON.stringify({ type: "user", message: { content: "automation" } })]);
    expect(rows).toHaveLength(1); expect(rows[0].text).toBe("😀".repeat(80)); expect(rows[0].result).toBe("😀".repeat(4096)); expect(rows[0].status).toBe("done");
  });
});
describe("transcript path contract", () => {
  it.each([
    [{ agent: "claude", cwd: "/repo/a_b.é😀", agent_session: { kind: "id" as const, value: "s" } }, "~/.claude/projects/-repo-a-b---/s.jsonl"],
    [{ agent: "codex", agent_session: { kind: "path" as const, value: "~/.codex/sessions/a.jsonl" } }, "~/.codex/sessions/a.jsonl"],
    [{ agent: "codex", agent_session: { kind: "id" as const, value: "s" } }, null],
    [{ agent: "claude" }, null],
    [{ agent: "claude", agent_session: { kind: "id" as const, value: "" } }, null],
  ])("resolves %j", (agent, path) => expect(transcriptPath(agent)).toBe(path));
  it.each([[800, "0.8s"], [42000, "42s"], [84000, "1m 24s"], [7500000, "2h 05m"]])("formats %i ms", (ms, text) => expect(duration(Number(ms))).toBe(text));
});
