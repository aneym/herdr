import { bridge } from "./bridge";
import type { AutomationSummary } from "./model";

function Status({ status }: { status: string }) {
  return <span className={`status ${status} state-dot`} aria-label={status} style={{ width: "var(--shell-face-dot)", height: "var(--shell-face-dot)" }} />;
}

function localTime(value?: string): string | undefined {
  if (!value) return undefined;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return undefined;
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
}

function shortCadence(value?: string): string | undefined {
  if (!value) return undefined;
  const match = value.match(/^every\s+(\d+)\s+min(?:ute)?s?$/i);
  return match ? `${match[1]}m` : value;
}

function tooltip(automation: AutomationSummary): string {
  const last = localTime(automation.last_run);
  const next = localTime(automation.next_run);
  return [automation.name, automation.cadence, last && `last ${last}`, next && `next ${next}`, automation.result].filter(Boolean).join(" · ");
}

function statusTone(status?: string): string {
  if (status === "acted") return "done";
  if (status === "error") return "blocked";
  if (status === "ok") return "done";
  return "unknown";
}

function openReport(url?: string): void {
  if (url && /^https:\/\//i.test(url)) void bridge.openUrl(url).catch(() => {});
}

export default function AutomationsSection({ automations }: { automations: AutomationSummary[] }) {
  if (!automations.length) return null;
  return <section className="automations-section">
    <h2 className="sidebar-section-title"><span>AUTOMATIONS</span></h2>
    {automations.map(automation => {
      const cadence = shortCadence(automation.cadence);
      const next = localTime(automation.next_run);
      const detail = tooltip(automation);
      return <div key={automation.id} className="automation-item">
        <button className="sidebar-row automation-row" title={detail} onClick={() => openReport(automation.report_url)}>
          <Status status={statusTone(automation.status)} />
          <span className="label">{automation.name}</span>
          {(cadence || next) && <span className="muted automation-trailing">{[cadence, next && `next ${next}`].filter(Boolean).join(" · ")}</span>}
        </button>
        {automation.result && <button className="sidebar-row automation-detail muted" title={detail} onClick={() => openReport(automation.report_url)}>{automation.result}</button>}
      </div>;
    })}
  </section>;
}
