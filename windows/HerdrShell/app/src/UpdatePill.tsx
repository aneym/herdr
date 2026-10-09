import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { bridge, type UpdateStatus } from "./bridge";

let updateError: string | null = null;
const errorListeners = new Set<() => void>();
export function showUpdateError(failure: unknown) {
  updateError = failure == null ? null : String(failure);
  errorListeners.forEach(listener => listener());
}
const subscribeError = (listener: () => void) => {
  errorListeners.add(listener);
  return () => { errorListeners.delete(listener); };
};

export function pillState(status: UpdateStatus | null) {
  return {
    update: !!status?.available && !!status.staged,
    rollback: !!status?.previous,
    tooltip: status?.staged ? `${status.staged.sha.slice(0, 7)} · ${status.staged.built_at}` : "",
  };
}

const DISMISSED_KEY = "herdr-shell.update.dismissed";
const readDismissed = () => { try { return localStorage.getItem(DISMISSED_KEY); } catch { return null; } };

/** "Oct 2, 7:08 PM" in local time from the bundle's UTC stamp, as the Mac popover shows it. */
function when(iso: string) {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString("en-US", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

/**
 * The title-bar Update button, ported from the Mac's titlebar accessory (UpdatePill.swift):
 * it shows while a staged release is on offer and opens a menu with Restart to update and Later.
 * The Mac has no roll back, so Roll back lives in the same menu; with no update on offer the
 * button stays out of sight until the header strip is hovered, as the old footer link did.
 */
export default function UpdatePill() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState(false);
  const [dismissed, setDismissed] = useState(readDismissed);
  const error = useSyncExternalStore(subscribeError, () => updateError);
  const applying = useRef(false);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let disposed = false;
    const refresh = async () => {
      try {
        const next = await bridge.updateStatus();
        if (!disposed) setStatus(next);
      } catch {
        if (!disposed) setStatus(null);
      }
    };
    void refresh();
    const interval = window.setInterval(() => { void refresh(); }, 60_000);
    return () => { disposed = true; window.clearInterval(interval); };
  }, []);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => { if (!box.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener("mousedown", close);
    box.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus();
    return () => document.removeEventListener("mousedown", close);
  }, [open]);
  useEffect(() => { if (error) setOpen(true); }, [error]);
  const apply = async (rollback: boolean) => {
    if (applying.current) return;
    applying.current = true;
    setBusy(true);
    showUpdateError(null);
    try {
      await (rollback ? bridge.updateRollback() : bridge.updateApply());
    } catch (failure) {
      showUpdateError(failure);
      applying.current = false;
      setBusy(false);
    }
  };
  const later = () => {
    const sha = status?.staged?.sha;
    if (sha) { try { localStorage.setItem(DISMISSED_KEY, sha); } catch { /* Later still hides it for this run. */ } setDismissed(sha); }
    showUpdateError(null);
    setOpen(false);
  };
  const state = pillState(status);
  const offered = state.update && status?.staged?.sha !== dismissed;
  if (!offered && !state.rollback && !error) return null;
  const staged = status?.staged;
  return <div ref={box} className={`title-update ${offered || error ? "" : "is-quiet"}`} onKeyDown={event => { if (event.key === "Escape") setOpen(false); }}>
    <button className="title-update-button" aria-haspopup="menu" aria-expanded={open} title={state.tooltip || undefined} onClick={() => setOpen(value => !value)}>{error ? "Update failed" : offered ? "Update" : "Roll back"}</button>
    {open && <div className="pane-menu title-update-menu" role="menu" aria-label="Update">
      {offered && staged && <p className="title-update-meta">{[staged.sha.slice(0, 8), when(staged.built_at)].filter(Boolean).join(" · ")}</p>}
      {error && <p className="title-update-meta" role="alert">{error}</p>}
      {state.update && <button role="menuitem" disabled={busy} onClick={() => { void apply(false); }}>{error ? "Retry" : "Restart to update"}</button>}
      {state.rollback && <button role="menuitem" disabled={busy} onClick={() => { void apply(true); }}>{`Roll back to ${status?.previous?.sha.slice(0, 7)}`}</button>}
      {offered && <button role="menuitem" onClick={later}>Later</button>}
    </div>}
  </div>;
}
