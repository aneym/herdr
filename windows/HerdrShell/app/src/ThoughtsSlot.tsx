import { useEffect, useRef, useState } from "react";
import type { OverlayHost } from "./spacesOverlay";
import type { useThoughts } from "./useThoughts";

export default function ThoughtsSlot({ thoughts, hosts }: { thoughts?: ReturnType<typeof useThoughts>; hosts: OverlayHost[] }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const saving = useRef(false);
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const close = () => {
    setOpen(false);
    trigger.current?.focus();
  };
  useEffect(() => { if (open) { dialog.current?.showModal(); textarea.current?.focus(); } }, [open]);
  const save = async () => {
    if (!thoughts || saving.current) return;
    if (!text.trim()) { setError("Write a thought first."); return; }
    saving.current = true;
    setBusy(true);
    setError(null);
    try {
      await thoughts.save(text);
      setText("");
      setOpen(false);
      trigger.current?.focus();
    } catch (reason) {
      setError(String(reason));
      textarea.current?.focus();
    } finally {
      saving.current = false;
      setBusy(false);
    }
  };
  return <div className="sidebar-hosts" aria-label={hosts.length ? "Hosts" : undefined}>
    <button ref={trigger} className="sidebar-row thoughts-row" data-testid="thoughts" onClick={() => { setError(null); setOpen(true); }}>
      <span className="muted">Thoughts</span>{!!thoughts?.count && <span className="thoughts-count">{thoughts.count}</span>}
    </button>
    {hosts.map((host, i) => {
      const trailing = (host.summary ?? "").replace(/load /g, "").replace(/ live/g, "").split(" · ").join(" ");
      return <div key={`${i}:${host.name}`} className="sidebar-row host-row" data-row={`host:${i}`} title={host.detail ? `${host.name}: ${host.detail}` : trailing ? `${host.name} ${trailing}` : host.name}>
        <span className="host-name">{host.name}</span><span className={`host-summary${host.attention === "act" ? " act" : host.attention === "warn" ? " warn" : ""}`}>{trailing}</span>{host.attention !== "none" && <span className={`host-alert ${host.attention === "act" ? "act" : "warn"}`}>!</span>}
      </div>;
    })}
    {open && <dialog ref={dialog} className="thought-dialog" role="dialog" data-open="true" aria-labelledby="thought-title" onCancel={event => { event.preventDefault(); close(); }} onKeyDown={event => {
      event.stopPropagation();
      if (event.key === "Escape") { event.preventDefault(); close(); }
    }}>
      <header><h2 id="thought-title">Thought</h2><button aria-label="Close thought" disabled={busy} onClick={close}>×</button></header>
      <textarea ref={textarea} aria-label="Thought" placeholder="Type or paste. Enter saves." value={text} readOnly={busy} aria-invalid={!!error} aria-describedby={error ? "thought-error" : undefined} onChange={event => setText(event.target.value)} onKeyDown={event => {
        if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); void save(); }
      }} />
      {error && <p id="thought-error" className="thought-error" role="alert">{error}</p>}
      <div className="thought-actions"><button disabled={busy} onClick={close}>Cancel</button><button disabled={busy || !text.trim()} onClick={() => void save()}>{busy ? "Saving…" : "Save"}</button></div>
    </dialog>}
  </div>;
}
