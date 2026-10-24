import { useEffect, useMemo, useRef, useState } from "react";
import { setAgentHidden } from "./control";
import { settingsAgentRows, settleSettings } from "./settingsAgents";
import type { Snapshot } from "./model";
import type { Face } from "./faces";
// Settings, full window (design rule 2026-10-01: settings is always full screen and shares the Rails
// Settings chrome): a section list on the left, the section's page on the right with one large title
// and its action beside it. Sections today: Agents. Ctrl+, opens and closes it, Esc closes.
function SettingsFace({ face }: { face: Face }) {
  const [broken, setBroken] = useState<string | null>(null);
  const picture = face.avatar && broken !== face.avatar ? face.avatar : null;
  return <span className="settings-face" data-face={`${face.initial}:${face.tint}${picture ? ":picture" : ""}`}>
    {picture
      ? <span className="face-disc picture"><img src={picture} alt="" referrerPolicy="no-referrer" onError={() => setBroken(picture)} /></span>
      : <span className={`face-disc face-tint-${face.tint}`}><span className="face-initial">{face.initial}</span></span>}
  </span>;
}
export default function Settings({ machine, snapshot, close, onError }: { machine: string; snapshot: Snapshot; close: () => void; onError: (error: unknown) => void }) {
  const [pending, setPending] = useState<Record<string, boolean>>({});
  const page = useRef<HTMLDivElement>(null);
  useEffect(() => { setPending(value => settleSettings(value, snapshot)); }, [snapshot]);
  useEffect(() => { page.current?.focus(); }, []);
  const rows = useMemo(() => settingsAgentRows(snapshot, pending), [snapshot, pending]);
  const set = (tab: string, shown: boolean) => {
    setPending(value => ({ ...value, [tab]: shown }));
    void setAgentHidden(machine, tab, !shown).catch(error => {
      setPending(value => { const next = { ...value }; delete next[tab]; return next; });
      onError(error);
    });
  };
  return <div className="settings" role="dialog" aria-label="Settings" ref={page} tabIndex={-1}
    onKeyDown={event => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(); } }}>
    <nav className="settings-nav" aria-label="Settings sections">
      <div className="settings-nav-title">Settings</div>
      <button className="settings-nav-row selected" aria-current="page">Agents</button>
    </nav>
    <main className="settings-page">
      <div className="settings-column">
        <header className="settings-header"><h1>Agents</h1><button className="settings-done" onClick={close}>Done</button></header>
        <p className="settings-lede">Shown agents sit in the Agents list. A hidden agent keeps running and keeps its place, and you can still reach it from search.</p>
        {rows.length === 0 && <p className="settings-empty">No agents yet. Right-click a pinned chat and choose Add to Agents.</p>}
        <ul className="settings-rows">
          {rows.map(row => <li key={row.tab} className="settings-row" data-tab={row.tab}>
            <SettingsFace face={row.face} />
            <span className="settings-row-text"><span className="settings-row-title">{row.title}</span>{row.meta && <span className="settings-row-meta">{row.meta}</span>}</span>
            <button role="switch" aria-checked={row.shown} aria-label={`Show ${row.title} in Agents`} className="settings-switch" onClick={() => set(row.tab, !row.shown)}><span className="settings-switch-knob" /></button>
          </li>)}
        </ul>
      </div>
    </main>
  </div>;
}
