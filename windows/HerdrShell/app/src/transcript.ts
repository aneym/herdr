export interface ChatItem {
  id: string; kind: "user" | "assistant" | "tool" | "duration" | "note"; text: string;
  tool?: string; status?: "running" | "done" | "error"; input?: string; result?: string; queued: boolean;
}
export interface ChatAgent { agent?: string; agent_session?: { kind: "id" | "path"; value: string }; cwd?: string; agent_status?: string; work_status?: string; terminal_title_stripped?: string }
export function transcriptPath(agent: ChatAgent): string | null {
  const session = agent.agent_session;
  if (!session?.value) return null;
  if (session.kind === "path") return session.value;
  return agent.agent === "claude" ? `~/.claude/projects/${(agent.cwd ?? "").replace(/[^A-Za-z0-9]/gu, "-")}/${session.value}.jsonl` : null;
}
function sortedJSON(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortedJSON);
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).sort(([a], [b]) => a < b ? -1 : a > b ? 1 : 0).map(([key, child]) => [key, sortedJSON(child)]));
  return value;
}
const clip = (text: string, max: number) => Array.from(text).slice(0, max).join("");
const unwrap = (text: string) => text.match(/^\s*<pasted_content id="([^"]+)">\n([\s\S]*)\n<\/pasted_content id="\1">\s*$/)?.[2] ?? text;
export function duration(ms: number): string {
  const seconds = ms / 1000, whole = Math.round(seconds);
  return seconds < 10 ? `${seconds.toFixed(1)}s` : whole < 60 ? `${whole}s` : whole < 3600 ? `${Math.floor(whole / 60)}m ${whole % 60}s` : `${Math.floor(whole / 3600)}h ${String(Math.floor(whole % 3600 / 60)).padStart(2, "0")}m`;
}
/** Pure JSONL parser; previous rows carry tool results across incremental reads. */
export function feed(lines: string | readonly string[], previous: readonly ChatItem[] = [], cwd = "", offset = 0): ChatItem[] {
  const rows = previous.map(row => ({ ...row }));
  for (const line of typeof lines === "string" ? lines.split("\n") : lines) {
    let r;
    try { r = JSON.parse(line); } catch { continue; }
    if (!r || typeof r !== "object" || r.isSidechain === true || r.isMeta === true || r.isCompactSummary === true) continue;
    const type = r.type, id = typeof r.uuid === "string" ? r.uuid : `${offset}:${rows.length}`;
    const message = r.message ?? {}, attachment = r.attachment ?? {};
    const add = (kind: ChatItem["kind"], text: string, extra: Partial<ChatItem> = {}) => rows.push({ id, kind, text, queued: false, ...extra });
    if (type === "user" && r.origin?.kind === "human" && typeof message.content === "string") { add("user", unwrap(message.content)); continue; }
    const queued = type === "attachment" ? attachment : r;
    const prompt = typeof queued.prompt === "string" ? queued.prompt : queued.content;
    if ((queued.type === "queued_command" || type === "queue-operation") && queued.origin?.kind === "human" && typeof prompt === "string") { add("user", unwrap(prompt), { queued: true }); continue; }
    if (type === "system" && r.subtype === "turn_duration") { add("duration", duration(typeof r.durationMs === "number" ? r.durationMs : 0)); continue; }
    if (Array.isArray(message.content)) message.content.forEach((b: Record<string, any>, index: number) => {
      if (!b || typeof b !== "object") return;
      if (type === "assistant" && b.type === "text" && typeof b.text === "string" && b.text) add("assistant", b.text, { id: `${id}:${index}` });
      if (type === "assistant" && b.type === "tool_use" && typeof b.id === "string" && typeof b.name === "string") {
        const input = b.input && typeof b.input === "object" && !Array.isArray(b.input) ? b.input : {}, name = b.name;
        let summary = "";
        if (["Read", "Edit", "Write"].includes(name)) { summary = typeof input.file_path === "string" ? input.file_path : ""; if (cwd && summary.startsWith(cwd + "/")) summary = summary.slice(cwd.length + 1); }
        else summary = [input.description, input.command, ...Object.keys(input).sort().map(key => input[key])].find(value => typeof value === "string") ?? "";
        let detail = JSON.stringify(sortedJSON(input), null, 2);
        if (name === "Edit" || name === "Write") {
          const old = typeof input.old_string === "string" ? input.old_string : "";
          const next = typeof input.new_string === "string" ? input.new_string : typeof input.content === "string" ? input.content : "";
          detail = "--- old\n+++ new\n" + old.split("\n").map((s: string) => "-" + s).join("\n") + "\n" + next.split("\n").map((s: string) => "+" + s).join("\n");
        }
        add("tool", clip(summary.split(/[\r\n]/)[0], 80), { id: b.id, tool: name, status: "running", input: detail });
      }
      if (type === "user" && b.type === "tool_result") {
        const row = rows.find(row => row.id === b.tool_use_id);
        if (row) { row.status = b.is_error === true ? "error" : "done"; row.result = clip(typeof b.content === "string" ? b.content : (Array.isArray(b.content) ? b.content : []).flatMap((part: any) => typeof part?.text === "string" ? [part.text] : []).join("\n"), 4096); }
      }
    });
    if (type === "attachment" && attachment.type === "hook_additional_context") {
      const content = typeof attachment.content === "string" ? attachment.content : Array.isArray(attachment.content) ? attachment.content.join("\n") : "";
      [...content.matchAll(/^\[lane bulletin\] \S+ from (\S+) \([^)]*\): (.*)$/gm)].forEach((match, index) => add("note", `from ${match[1]}: ${clip(match[2], 200)}`, { id: `${id}:${index}` }));
    }
  }
  return rows;
}
