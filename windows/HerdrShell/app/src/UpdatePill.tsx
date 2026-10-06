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

export default function UpdatePill() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const error = useSyncExternalStore(subscribeError, () => updateError);
  const applying = useRef(false);
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
  const state = pillState(status);
  return <>
    {(state.update || state.rollback) && <span className="update-actions">
      {state.update && <button className="update-pill" title={state.tooltip} disabled={busy} onClick={() => { void apply(false); }}>Update</button>}
      {state.rollback && <button className="update-rollback" title={`Roll back to ${status?.previous?.sha.slice(0, 7)}`} disabled={busy} onClick={() => { void apply(true); }}>Roll back</button>}
    </span>}
    {error && <span className="update-error" role="alert">{error}</span>}
  </>;
}
