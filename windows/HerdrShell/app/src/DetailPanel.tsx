import { useCallback, useEffect, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import type { Snapshot } from "./model";
import { buildDetailContent } from "./detailContent";
export function useDetailPanel(switcherOpen = false, snapshot?: Snapshot) {
  const [rowId, setRowId] = useState<string | null>(null);
  const close = useCallback(() => setRowId(null), []);
  const toggle = useCallback((id: string) => setRowId(current => current === id ? null : id), []);
  const claimEscape = useCallback((event: KeyboardEvent) => {
    const textInput = document.activeElement?.matches('input, textarea:not(.xterm-helper-textarea), [contenteditable="true"]');
    if (switcherOpen || textInput || !rowId || event.key !== "Escape" || event.isComposing || event.keyCode === 229) return false;
    close(); return true;
  }, [rowId, close, switcherOpen]);
  useEffect(() => {
    const key = (event: KeyboardEvent) => { if (claimEscape(event)) { event.preventDefault(); event.stopImmediatePropagation(); } };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [claimEscape]);
  useEffect(() => {
    if (rowId && snapshot?.tabs && !snapshot.tabs.some(tab => tab.tab_id === rowId)) close();
  }, [snapshot, rowId, close]);
  return { rowId, toggle, close, open: setRowId };
}
const muted: CSSProperties = { color: "var(--shell-muted)" };
const line: CSSProperties = { display: "flex", alignItems: "baseline", gap: "var(--shell-space-detail-line-gap)" };
function Section({ title, count, children }: { title: string; count: number; children: ReactNode }) {
  return <section style={{ display: "grid", gap: "var(--shell-space-detail-section-gap)", borderTop: "1px solid var(--shell-line)", paddingTop: "var(--shell-space-detail-section-gap)" }}><header style={{ ...line, ...muted, fontSize: "var(--shell-type-section-label)", letterSpacing: "var(--shell-type-detail-tracking)" }}><span>{title}</span><span style={{ marginLeft: "auto" }}>{count}</span></header>{children}</section>;
}
/** Display-only workflow rows, as on Mac; only open-full changes the selected tab. */
export default function DetailPanel({ snapshot, rowId, openFull }: { snapshot: Snapshot; rowId: string; openFull: (id: string) => void }) {
  const content = buildDetailContent(snapshot, rowId);
  return <aside aria-label="Lane details" onMouseDown={event => event.preventDefault()} style={{ width: "var(--shell-space-detail-width)", flex: "0 0 var(--shell-space-detail-width)", boxSizing: "border-box", overflowY: "auto", background: "var(--shell-surface)", color: "var(--shell-text)", borderRight: "1px solid var(--shell-line)", padding: "var(--shell-space-detail-padding)", fontFamily: "var(--shell-font-ui)", fontSize: "var(--shell-type-row-title)" }}>
    {!content ? <span style={muted}>row is gone</span> : <div style={{ display: "grid", gap: "var(--shell-space-detail-gap)" }}>
      <header style={line}><strong style={{ color: `var(--shell-${content.kind === "orchestrator" ? "orch" : "lane"})`, fontSize: "var(--shell-type-switcher-row)" }}>{content.label}</strong><span style={{ ...muted, marginLeft: "auto" }}>esc</span></header>
      <div style={{ ...muted, fontSize: "var(--shell-type-switcher-meta)" }}>{content.kind} · {content.agent ?? "shell"} · {content.status} · {content.host}</div>
      <Section title="INBOX" count={content.inbox.length}>{!content.inbox.length && <span style={muted}>nothing waiting</span>}{content.inbox.map((item, index) => <div key={index} style={line}><span>{item.text}</span><span style={{ color: item.source === "blocked" ? "var(--shell-warn)" : "var(--shell-muted)", marginLeft: "auto", fontSize: "var(--shell-type-switcher-meta)" }}>{item.source}</span></div>)}</Section>
      {content.routed.length > 0 && <Section title="ROUTED" count={content.routed.length}>{content.routed.map((item, index) => <div key={index} style={line}><span>→ {item.text}</span><span style={{ ...muted, marginLeft: "auto" }}>{item.source}</span></div>)}</Section>}
      <Section title={content.kind === "orchestrator" ? "WORKFLOWS UNDER IT" : "WORKFLOWS"} count={content.groups.reduce((n, g) => n + g.workflows.length, 0)}>
        {!content.groups.length && <span style={muted}>none running</span>}
        {content.groups.map((group, index) => <div key={index}>{group.lane && <strong style={{ color: "var(--shell-lane)", fontSize: "var(--shell-type-switcher-meta)" }}>{group.lane}</strong>}{group.workflows.map(w => <div key={w.id} style={line}><span style={{ color: w.status === "working" ? "var(--shell-ok)" : w.status === "blocked" ? "var(--shell-warn)" : "var(--shell-muted)" }} aria-label={w.status}>{({ working: "●", blocked: "◐", idle: "○" } as Record<string, string>)[w.status] ?? "·"}</span><span style={{ color: "var(--shell-wf)" }}>{w.label}</span><span style={{ ...muted, marginLeft: "auto", color: w.status === "blocked" ? "var(--shell-warn)" : "var(--shell-muted)" }}>{w.phase}</span><span style={{ ...muted, border: "1px solid var(--shell-line)", borderRadius: "var(--shell-radius-row)", padding: "var(--shell-space-detail-host-pad-y) var(--shell-space-detail-host-pad-x)", fontSize: "var(--shell-type-glyph)" }}>{w.host}</span></div>)}</div>)}
      </Section>
      <button tabIndex={-1} onClick={() => openFull(content.id)} style={{ justifySelf: "start", border: "1px solid var(--shell-line)", borderRadius: "var(--shell-radius-detail-button)", padding: "var(--shell-space-detail-section-gap) var(--shell-space-terminal-pad-x)" }}>open full {content.kind} tab</button>
    </div>}
  </aside>;
}
