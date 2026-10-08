import { useEffect, useMemo, useState } from "react";
import { bridge, fromBase64 } from "./bridge";
import { docItems, docKey, laneFor, parseCatalog, projectFolder } from "./docs";
import type { DocItem, LaneCatalog } from "./docs";
import type { Snapshot } from "./model";
import { webUrl } from "./links";
import { renderMarkdown } from "./markdown";
const catalogPaths = ["~/.agent-rails/herdr/lanes.json", "~/.agent-rails/herdr/areas.json"];
async function readWhole(machine: string, path: string): Promise<string> {
  const decoder = new TextDecoder();
  let offset = 0, text = "", stamp = "";
  do {
    const chunk = await bridge.fileRead(machine, path, offset, 2 * 1024 * 1024);
    const next = `${chunk.mtime_ms}:${chunk.size}:${chunk.inode}`;
    if (offset && stamp !== next) throw new Error("Document changed while reading; retrying on next poll");
    stamp = next;
    const bytes = fromBase64(chunk.data_b64);
    if (!bytes.length && offset < chunk.size) throw new Error("Incomplete document read");
    text += decoder.decode(bytes, { stream: true }); offset += bytes.length;
    if (offset >= chunk.size) break;
  } while (true);
  return text + decoder.decode();
}
export function useDocs(machine: string, tab: string | null, snapshot: Snapshot, open: boolean) {
  const [catalog, setCatalog] = useState<LaneCatalog>({ lanes: {}, names: {} });
  const [found, setFound] = useState<{ folder: string | null; paths: Set<string> }>({ folder: null, paths: new Set() });
  const [error, setError] = useState("");
  useEffect(() => {
    let disposed = false, busy = false, stamp = "";
    setCatalog({ lanes: {}, names: {} }); setError("");
    if (!open) return;
    const poll = async () => {
      if (disposed || busy || document.hidden) return;
      busy = true;
      try {
        const stats = await Promise.all(catalogPaths.map(path => bridge.fileStat(machine, path)));
        if (disposed) return;
        const next = JSON.stringify(stats.map(s => [s.exists, s.mtime_ms]));
        if (next !== stamp) {
          const contents = await Promise.all(catalogPaths.map((path, i) => stats[i].exists ? readWhole(machine, path) : Promise.resolve("{}")));
          const parsed = parseCatalog(contents[0], contents[1]);
          if (!disposed) { setCatalog(parsed); stamp = next; setError(""); }
        }
      } catch (err) { if (!disposed) setError(String(err)); }
      finally { busy = false; }
    };
    void poll(); const timer = setInterval(() => void poll(), 5000);
    document.addEventListener("visibilitychange", poll);
    return () => { disposed = true; clearInterval(timer); document.removeEventListener("visibilitychange", poll); };
  }, [machine, open]);
  const lane = useMemo(() => laneFor(tab, snapshot, catalog), [tab, snapshot, catalog]);
  const folder = projectFolder(lane);
  useEffect(() => {
    let disposed = false, busy = false;
    setFound({ folder: null, paths: new Set() });
    if (!open) return;
    const poll = async () => {
      if (!folder || disposed || busy || document.hidden) return;
      busy = true;
      try {
        const paths = ["RESUME", "BRIEF", "DECISIONS"].map(name => `${folder}/${name}.md`);
        const stats = await Promise.all(paths.map(path => bridge.fileStat(machine, path)));
        if (!disposed) setFound({ folder, paths: new Set(paths.filter((_, i) => stats[i].exists)) });
      } catch (err) { if (!disposed) setError(String(err)); }
      finally { busy = false; }
    };
    void poll(); const timer = setInterval(() => void poll(), 5000);
    document.addEventListener("visibilitychange", poll);
    return () => { disposed = true; clearInterval(timer); document.removeEventListener("visibilitychange", poll); };
  }, [machine, folder, open]);
  return { items: useMemo(() => open ? docItems(lane, found.folder === folder ? found.paths : new Set()) : [], [lane, found, folder, open]), error };
}
export default function DocPanel({ machine, tab, items, active, select, error: catalogError, close }: { machine: string; tab?: string | null; items: DocItem[]; active: string | null; select: (name: string) => void; error: string; close?: () => void }) {
  const item = items.find(item => docKey(item) === active);
  const [content, setContent] = useState({ path: "", text: "", error: "", source: "" });
  const [adding, setAdding] = useState(false);
  const [address, setAddress] = useState("");
  const path = item?.path;
  useEffect(() => {
    if (!path) return;
    let disposed = false, busy = false, stamp = "";
    setContent({ path, text: "", error: "", source: "" });
    const poll = async () => {
      if (disposed || busy || document.hidden) return;
      busy = true;
      try {
        if (item?.id && tab) {
          const result = await bridge.api(machine, "desk.read", { tab_id: tab, item: item.id, ...(stamp ? { known_mtime_ms: Number(stamp) } : {}) }) as { mtime_ms: number; unchanged?: boolean; data_base64?: string; mime?: string };
          if (!disposed && !result.unchanged && result.data_base64 !== undefined) {
            stamp = String(result.mtime_ms);
            const bytes = fromBase64(result.data_base64);
            const mime = result.mime ?? item.mime ?? "text/plain";
            const text = new TextDecoder().decode(bytes);
            setContent({ path, text, error: "", source: mime === "text/markdown" || mime === "text/plain" ? "" : `data:${mime};base64,${result.data_base64}` });
          }
          return;
        }
        const stat = await bridge.fileStat(machine, path);
        if (!stat.exists) throw new Error("Document no longer exists");
        const next = `${stat.mtime_ms}:${stat.size}:${stat.inode}`;
        if (next !== stamp) {
          const text = await readWhole(machine, path);
          if (!disposed) { stamp = next; setContent({ path, text, error: "", source: "" }); }
        }
      } catch (error) { stamp = ""; if (!disposed) setContent(value => ({ ...value, path, error: String(error) })); }
      finally { busy = false; }
    };
    void poll(); const timer = setInterval(() => void poll(), 1000);
    document.addEventListener("visibilitychange", poll);
    return () => { disposed = true; clearInterval(timer); document.removeEventListener("visibilitychange", poll); };
  }, [machine, path, tab, item?.id, item?.mime]);
  const html = useMemo(() => renderMarkdown(content.path === path ? content.text : ""), [content, path]);
  const open = (url: string) => { if (/^(https?:|mailto:)/i.test(url)) void bridge.openUrl(url).catch(error => setContent(value => ({ ...value, error: String(error) }))); };
  return <section className="docs" aria-label="Documents">
    <div className="docs-header"><div className="docs-tabs" role="tablist" aria-label="Documents">{items.map(doc => <button key={docKey(doc)} role="tab" aria-selected={docKey(doc) === active} onClick={() => select(docKey(doc))}>{doc.name}</button>)}</div>
      <button aria-label="Add document" onClick={() => setAdding(value => !value)}>+</button>
      <button aria-label="Close document" onClick={() => {
        if (item?.id && tab) void bridge.api(machine, "desk.close", { tab_id: tab, item: item.id }).catch(error => setContent(value => ({ ...value, error: String(error) })));
        else close?.();
      }}>✕</button>
    </div>
    {adding && <form className="docs-add" onSubmit={event => {
      event.preventDefault(); const ref = address.trim();
      if (!ref || !tab) return;
      void bridge.api(machine, "desk.open", { tab_id: tab, ref, opened_by: "user" }).then(() => { setAddress(""); setAdding(false); }, error => setContent(value => ({ ...value, error: String(error) })));
    }}><input autoFocus aria-label="Document address" placeholder="https://… or a file path" value={address} onChange={event => setAddress(event.target.value)} onKeyDown={event => { if (event.key === "Escape") setAdding(false); }} /></form>}
    {item && <div className="docs-address"><span className="docs-path" title={path ?? item.url}>{path ?? item.url}</span><button aria-label="Open document externally" className={item.url && /^(https?:|mailto:)/i.test(item.url) ? "muted" : undefined} disabled={!item.url || !/^(https?:|mailto:)/i.test(item.url)} title={!item.url ? "File is on Studio" : undefined} onClick={() => { if (item.url) open(item.url); }}>Open</button></div>}
    <div className="docs-body" role="tabpanel">
      {(catalogError || content.error) && <p className="muted" role="status">{catalogError || content.error}</p>}
      {item?.kind === "web" ? <>{item.url && webUrl(item.url) && <iframe title={item.name} src={item.url} sandbox="allow-scripts allow-forms allow-same-origin" style={{ width: "100%", height: "100%", border: 0 }} />}</> : content.source ? <iframe title={item?.name} src={content.source} sandbox="" style={{ width: "100%", height: "100%", border: 0 }} /> : item?.kind === "file" && item.mime !== "text/markdown" ? <pre>{content.text}</pre> : <div className="chat-markdown" onAuxClick={event => { if ((event.target as HTMLElement).closest("a")) event.preventDefault(); }} onClick={event => {
        const anchor = (event.target as HTMLElement).closest("a");
        if (anchor) { event.preventDefault(); const url = anchor.getAttribute("href"); if (url) open(url); }
      }} dangerouslySetInnerHTML={{ __html: html }} />}
    </div>
  </section>;
}
