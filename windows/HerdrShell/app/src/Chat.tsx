import { useEffect, useMemo, useRef, useState } from "react";
import { Marked } from "marked";
import DOMPurify from "dompurify";
import { bridge } from "./bridge";
import { ChatTail } from "./chatTail";
import { ChatSender } from "./chatSend";
import type { SendState } from "./chatSend";
import type { ChatItem } from "./transcript";
const markdown = new Marked({ renderer: { html: () => "" } });
function Markdown({ text }: { text: string }) {
  const html = useMemo(() => DOMPurify.sanitize(markdown.parse(text, { async: false }), { USE_PROFILES: { html: true }, FORBID_TAGS: ["img", "video", "audio", "iframe", "form", "input", "button", "style"] }), [text]);
  return <div className="chat-markdown" onAuxClick={event => { if ((event.target as HTMLElement).closest("a")) event.preventDefault(); }} onClick={event => {
    const anchor = (event.target as HTMLElement).closest("a");
    if (!anchor) return;
    event.preventDefault();
    const url = anchor.getAttribute("href");
    if (url && /^(https?:|mailto:)/i.test(url)) void bridge.openUrl(url).catch(() => {});
  }} dangerouslySetInnerHTML={{ __html: html }} />;
}
function ToolGroup({ items }: { items: ChatItem[] }) {
  return <details className="chat-tools"><summary>{items.length === 1 ? `${items[0].tool}: ${items[0].text}` : `${items.length} tool calls · ${[...new Set(items.map(item => item.tool))].join(", ")}`}</summary>
    {items.map(item => <details key={item.id} className="chat-tool"><summary>{item.tool} · {item.text} <span className="muted">{item.status}</span></summary><pre>{item.input}</pre>{item.result !== undefined && <pre>{item.result}</pre>}</details>)}
  </details>;
}
export default function Chat({ machine, pane, focused, visible, onItems }: { machine: string; pane: string; focused: boolean; visible: boolean; onItems: (count: number) => void }) {
  const [items, setItems] = useState<ChatItem[]>([]);
  const [status, setStatus] = useState("asleep"), [name, setName] = useState("Claude");
  const [waiting, setWaiting] = useState(false), [earlier, setEarlier] = useState(false), [error, setError] = useState("");
  const [send, setSend] = useState<SendState>({ text: "", status: "", warning: false, known: new Set() });
  const [text, setText] = useState("");
  const tail = useRef<ChatTail>(), sender = useRef<ChatSender>();
  const active = useRef(visible); active.current = visible;
  const refresh = useRef<() => void>(() => {});
  const load = useRef<() => void>(() => {}), composer = useRef<HTMLTextAreaElement>(null), scroll = useRef<HTMLDivElement>(null), stick = useRef(true);
  useEffect(() => {
    let disposed = false, busy = false, earlierQueued = false;
    const reader = new ChatTail(machine, pane), writer = new ChatSender(machine, pane, setSend);
    tail.current = reader; sender.current = writer;
    const publish = () => {
      if (disposed) return;
      setItems([...reader.items]); onItems(reader.items.length); writer.acknowledge(reader.items);
      setStatus(reader.agent.work_status ?? reader.agent.agent_status ?? "asleep"); setName(reader.agent.terminal_title_stripped?.trim() || reader.agent.agent || "Claude");
      setWaiting(reader.waiting); setEarlier(reader.earlier); setError("");
    };
    const run = async (earlier = false) => {
      if (earlier) earlierQueued = true;
      if (disposed || busy || document.hidden || !active.current) return;
      busy = true;
      try {
        if (!earlierQueued) { await reader.refresh(); publish(); }
        while (earlierQueued && !disposed && active.current && !document.hidden) {
          earlierQueued = false;
          await reader.loadEarlier(); publish();
        }
      }
      catch (error) { if (!disposed) setError(String(error)); }
      finally { busy = false; }
    };
    refresh.current = () => { void run(); };
    load.current = () => { stick.current = false; void run(true); };
    void run();
    const timer = setInterval(() => { void run(); }, 1000);
    const onVisible = () => { if (!document.hidden) void run(); };
    document.addEventListener("visibilitychange", onVisible);
    return () => { disposed = true; clearInterval(timer); writer.dispose(); document.removeEventListener("visibilitychange", onVisible); onItems(0); };
  }, [machine, pane, onItems]);
  useEffect(() => { if (visible) refresh.current(); }, [visible]);
  useEffect(() => { if (focused && visible) composer.current?.focus(); }, [focused, visible]);
  useEffect(() => { if (visible && stick.current && scroll.current) scroll.current.scrollTop = scroll.current.scrollHeight; }, [items, send.text, visible]);
  const groups: ChatItem[][] = [];
  for (const item of items) { if (item.kind === "tool" && groups[groups.length - 1]?.[0].kind === "tool") groups[groups.length - 1].push(item); else groups.push([item]); }
  const submit = (anyway = false) => {
    const message = anyway ? send.text : text;
    if (sender.current?.send(message, items, anyway)) { setText(""); stick.current = true; }
  };
  return <section className="chat" aria-label={`Chat with ${name}`}>
    <div className="chat-scroll" ref={scroll} onScroll={() => { const node = scroll.current; if (node) stick.current = node.scrollHeight - node.scrollTop - node.clientHeight < 60; }}>
      <div className="chat-column">
        {earlier && <button className="muted" onClick={() => load.current()}>Load earlier messages</button>}
        {waiting && <p className="muted">Waiting for transcript…</p>}
        {!waiting && !items.length && <p className="muted">{tail.current?.agent.agent_session ? "No messages yet" : "No transcript available for this pane"}</p>}
        {groups.map(group => { const item = group[0]; return <div key={item.id} className={`chat-item chat-${item.kind}`}>
          {item.kind === "tool" ? <ToolGroup items={group} /> : item.kind === "assistant" ? <Markdown text={item.text} /> : <>{item.kind === "user" && <div className="chat-caption">You{item.queued ? " · Queued" : ""}</div>}<div className="chat-plain">{item.text}</div></>}
        </div>; })}
        {send.text && <div className="chat-item chat-user chat-pending"><div className="chat-caption">You · {send.warning ? "Not sent" : "Pending"}</div><div className="chat-plain">{send.text}</div></div>}
      </div>
    </div>
    <div className="chat-bottom">
      <div className="chat-status" role="status">{name} · {status}{send.status && ` · ${send.status}`}{error && ` · ${error}`}</div>
      {send.text && <div className="chat-warning">{send.status}<button onClick={() => { const pending = sender.current?.cancel() ?? ""; setText(current => current || pending); }}>Cancel</button>{(send.status === "There's unsent text in the terminal" || send.status === "Can't see the prompt; send anyway?") && <button onClick={() => submit(true)}>Send anyway</button>}</div>}
      <form className="chat-composer" onSubmit={event => { event.preventDefault(); submit(); }}>
        <textarea ref={composer} value={text} onChange={event => setText(event.target.value)} aria-label="Message" placeholder={`Message ${name}`} rows={3} onKeyDown={event => { if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); submit(); } }} />
        <div className="chat-composer-footer"><span>Enter to send · Shift+Enter for a new line</span><button type="submit" disabled={!text.trim() || !!send.text}>Send</button></div>
      </form>
    </div>
  </section>;
}
