import { useEffect, useState } from "react";
import { bridge } from "./bridge";
import { buildFactorySnapshot, scrub } from "./factory";
import type { FactoryBundle, FactorySnapshot } from "./factory";
export function useFactory(machine: string, online: boolean): FactorySnapshot {
  const [snapshot, setSnapshot] = useState(() => buildFactorySnapshot(null));
  useEffect(() => {
    let disposed = false, busy = false, bundle: FactoryBundle | null = null, fetched: number | undefined;
    const poll = async () => {
      if (busy || disposed) return;
      busy = true;
      try {
        if (!online) throw new Error("unavailable");
        const next = await bridge.factory(machine);
        if (disposed) return;
        const validPools = !!next.pools && Array.isArray((next.pools as { pools?: unknown }).pools) && ((next.pools as { pools: unknown[] }).pools).length > 0;
        if (validPools) fetched = Date.now() - (next.poolsAgeSeconds ?? 0) * 1000;
        bundle = validPools ? next : { ...next, pools: bundle?.pools ?? null };
        setSnapshot(buildFactorySnapshot(bundle, Date.now(), fetched, !validPools));
      } catch { if (!disposed) setSnapshot(buildFactorySnapshot(bundle, Date.now(), fetched, true)); }
      finally { busy = false; }
    };
    void poll(); const timer = setInterval(() => void poll(), 1000);
    return () => { disposed = true; clearInterval(timer); };
  }, [machine, online]);
  return snapshot;
}
export default function FactoryPage({ machine, online }: { machine: string; online: boolean }) {
  return <FactoryView snapshot={useFactory(machine, online)} machine={machine} />;
}
export function FactoryView({ snapshot: s, machine }: { snapshot: FactorySnapshot; machine: string }) {
  const [expanded, setExpanded] = useState<string | null>(null);
  const [picks, setPicks] = useState<Record<string, { text: string; at: number }>>({});
  const toggleRoute = (name: string) => {
    setExpanded(expanded === name ? null : name);
    if (expanded === name || (name !== "implement" && name !== "mechanical")) return;
    if (picks[name] && Date.now() - picks[name].at < 60000) return;
    setPicks(value => ({ ...value, [name]: { text: "asking route pick…", at: Date.now() } }));
    void bridge.factoryRoutePick(machine, name).then(text => {
      setPicks(value => ({ ...value, [name]: { text: scrub(text ?? "route pick failed"), at: Date.now() } }));
    }, () => setPicks(value => ({ ...value, [name]: { text: "route pick failed", at: Date.now() } })));
  };
  return <main className="factory-view" aria-label="Factory">
    <header><h1>Factory</h1><span>{s.updated}</span></header>
    <section><h2>Machines</h2>{!s.machines.length && <p>no machines</p>}{s.machines.map(r => {
      const attention = r.attention === "warn" ? "var(--shell-warn)" : r.attention === "act" ? "var(--shell-attention-act)" : "var(--shell-text)";
      const usage = r.usageState === "overloaded" ? "var(--shell-bad)" : r.usageState === "idle" ? "var(--shell-muted)" : "var(--shell-ok)";
      const glyph = r.usageState === "overloaded" ? "blocked" : r.usageState === "idle" ? "idle" : "working";
      const state = r.state.startsWith("drained") || r.state.startsWith("down") || r.state === "held" ? "var(--shell-warn)" : "var(--shell-ok)";
      return <div data-machine={r.name} className={r.dimmed ? "factory-dim" : ""} key={r.name}><div className="factory-row">
        {r.usageState && <span className={`factory-usage-glyph ${glyph}`} data-usage-glyph={glyph} aria-label={r.usageState} style={{ color: usage }} />}
        <strong className="factory-machine-name" style={{ color: attention }}>{r.name}</strong><span>{r.kind}</span>
        <span className="factory-machine-summary" style={{ color: r.attention ? attention : "var(--shell-muted)" }}>{r.summary}</span>
        <span className="factory-trailing factory-machine-state" style={{ color: state }}>{r.state}</span></div>
        <div className="factory-detail"><span className="factory-usage-line" style={{ color: usage }}>{r.usageLine}</span><span>{r.slots}</span><span>{r.disk}</span></div></div>;
    })}</section>
    <section><h2>Pools{s.poolsAge ? ` · ${s.poolsAge}` : ""}</h2>{!s.pools.length && <p>{s.poolsStale ? "no pool data" : "waiting for pools"}</p>}{s.pools.map(r => <div key={r.id} className={`factory-pool tone-${r.tone}`}><div className="factory-row"><span>{r.provider}</span><span>{r.id}</span><span className="factory-trailing">{r.counts}</span></div>{[["5h", r.fiveHour, r.fiveHourLabel], ["wk", r.weekly, r.weeklyLabel]].map(([label, value, text]) => <div className="factory-bar-row" key={String(label)}><span>{label}</span><meter min={0} max={100} value={typeof value === "number" ? Math.max(0, Math.min(100, value)) : 0} aria-label={`${r.id} ${label}`} /><span>{text || "—"}</span></div>)}<div className="factory-detail"><span>{r.pace}</span><span>{r.monthly}</span><span>{r.refill}</span></div></div>)}</section>
    <section><h2>Routing · ladder: {s.ladderMode || "—"}</h2>{!s.routes.length && <p>no ladder</p>}{s.routes.map(r => <div key={r.name}><button className="factory-route" aria-expanded={expanded === r.name} onClick={() => toggleRoute(r.name)}>{expanded === r.name ? "▾" : "▸"} {r.name} {r.chips.map((c, i) => <span className="factory-chip" key={i}>{c}</span>)}</button>{expanded === r.name && <p>{picks[r.name]?.text ?? "asking route pick…"}</p>}</div>)}{s.decider && <p>{s.decider}</p>}<details><summary>Open routing table</summary><pre>{s.routes.map(r => `${r.name}: ${r.chips.join(" → ")}`).join("\n")}</pre></details></section>
    <section><h2>In flight</h2>{!s.flights.length && <p>nothing live</p>}{s.flights.map(r => <div className="factory-row" key={r.id}><span>{r.name}</span><span>{r.lane}</span><span>{r.tab}</span><span>{r.host}</span>{r.headless && <span className="factory-chip">headless</span>}<span className="factory-trailing">{r.age}</span></div>)}<p>Landed today: {s.landedCount}</p>{s.landed.map(r => <div className="factory-row" key={r.id}><span>{r.time}</span><span>{r.subject}</span></div>)}</section>
  </main>;
}
