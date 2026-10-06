import { useEffect, useRef, useState } from "react";
import { bridge, type UpdateStatus } from "./bridge";

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
  const [error, setError] = useState<string | null>(null);
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
    setError(null);
    try {
      await (rollback ? bridge.updateRollback() : bridge.updateApply());
    } catch (failure) {
      setError(String(failure));
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

// In-source tests keep the slice within its file allowlist; Vitest includeSource discovers them.
if (import.meta.vitest) {
  const { describe, it, expect } = import.meta.vitest;
  describe("pillState", () => {
    // Pure visibility projection guards independent update/rollback availability and absent metadata.
    it("shows only actions supported by the status, including rollback without an update", () => {
      const staged = { sha: "abcdef123456", built_at: "2026-10-06T12:00:00Z" };
      const previous = { sha: "fedcba123456" };
      expect(pillState(null)).toEqual({ update: false, rollback: false, tooltip: "" });
      for (const [available, build, prior, update, rollback] of [
        [false, null, null, false, false],
        [false, staged, null, false, false],
        [true, staged, null, true, false],
        [false, staged, previous, false, true],
        [true, staged, previous, true, true],
        [true, null, previous, false, true],
      ] as const) {
        expect(pillState({ current: "1234567", available, staged: build, previous: prior })).toEqual({
          update, rollback, tooltip: build ? "abcdef1 · 2026-10-06T12:00:00Z" : "",
        });
      }
    });
  });
}

declare global {
  interface ImportMeta { readonly vitest?: typeof import("vitest"); }
}
