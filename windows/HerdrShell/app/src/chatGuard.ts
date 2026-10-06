export type Prompt = "clear" | "draft" | "unknown";
export function prompt(screen: string): Prompt {
  let plain = "", visible = "", faint = false, offset = 0;
  const append = (s: string) => { const text = s.replace(/\u00a0/g, " "); plain += text; visible += faint ? text.replace(/[^\r\n]/gu, " ") : text; };
  for (const match of screen.matchAll(/\x1b\[([0-9;:]*)m|\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g)) {
    append(screen.slice(offset, match.index));
    if (match[1] !== undefined) {
      const codes = match[1] ? match[1].split(/[;:]/).map(Number) : [0];
      for (let i = 0; i < codes.length;) {
        const c = codes[i];
        if ([38, 48, 58].includes(c)) { i += codes[i + 1] === 2 ? 5 : codes[i + 1] === 5 ? 3 : 2; continue; }
        if (c === 2) faint = true;
        if (c === 0 || c === 22) faint = false;
        i++;
      }
    }
    offset = (match.index ?? 0) + match[0].length;
  }
  append(screen.slice(offset));
  const lines = plain.split(/[\r\n]/), shown = visible.split(/[\r\n]/);
  let start = -1;
  lines.forEach((line, i) => { if (line.trimStart().startsWith("❯")) start = i; });
  if (start < 0) return "unknown";
  let input = shown[start].slice(lines[start].indexOf("❯") + 1);
  for (let i = start + 1; i < lines.length; i++) { if (lines[i].trimStart().startsWith("─")) break; input += "\n" + shown[i]; }
  return input.trim() ? "draft" : "clear";
}
