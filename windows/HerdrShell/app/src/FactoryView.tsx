import { useEffect, useState } from "react";
import { bridge } from "./bridge";
import { buildFactorySnapshot } from "./factory";
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
        if (next.pools) fetched = Date.now() - (next.poolsAgeSeconds ?? 0) * 1000;
        bundle = next.pools ? next : { ...next, pools: bundle?.pools ?? null };
        setSnapshot(buildFactorySnapshot(bundle, Date.now(), fetched, !next.pools));
      } catch { if (!disposed) setSnapshot(buildFactorySnapshot(bundle, Date.now(), fetched, true)); }
      finally { busy = false; }
    };
    void poll(); const timer = setInterval(() => void poll(), 1000);
    return () => { disposed = true; clearInterval(timer); };
  }, [machine, online]);
  return snapshot;
}
export default function FactoryPage({ machine, online }: { machine: string; online: boolean }) {
  return <FactoryView snapshot={useFactory(machine, online)} />;
}
export function FactoryView({ snapshot: s }: { snapshot: FactorySnapshot }) {
  const [expanded, setExpanded] = useState<string | null>(null);
  return <main className="factory-view" aria-label="Factory">
    <header><h1>Factory</h1><span>{s.updated}</span></header>
    <section><h2>Machines</h2>{!s.machines.length && <p>no machines</p>}{s.machines.map(r => <div className={r.dimmed ? "factory-dim" : ""} key={r.name}><div className="factory-row"><strong>{r.name}</strong><span>{r.kind}</span><span>{r.summary}</span><span className="factory-trailing">{r.state}</span></div><div className="factory-detail"><span>{r.usageLine}</span><span>{r.slots}</span><span>{r.disk}</span></div></div>)}</section>
    <section><h2>Pools{s.poolsAge ? ` · ${s.poolsAge}` : ""}</h2>{!s.pools.length && <p>{s.poolsStale ? "no pool data" : "waiting for pools"}</p>}{s.pools.map(r => <div key={r.id} className={`factory-pool tone-${r.tone}`}><div className="factory-row"><span>{r.provider}</span><span>{r.id}</span><span className="factory-trailing">{r.counts}</span></div>{[["5h", r.fiveHour, r.fiveHourLabel], ["week", r.weekly, r.weeklyLabel]].map(([label, value, text]) => <div className="factory-bar-row" key={String(label)}><span>{label}</span><meter min={0} max={100} value={typeof value === "number" ? Math.max(0, Math.min(100, value)) : 0} aria-label={`${r.id} ${label}`} /><span>{text || "—"}</span></div>)}<div className="factory-detail"><span>{r.pace}</span><span>{r.monthly}</span><span>{r.refill}</span></div></div>)}</section>
    <section><h2>Routing · ladder: {s.ladderMode || "—"}</h2>{!s.routes.length && <p>no ladder</p>}{s.routes.map(r => <div key={r.name}><button className="factory-route" aria-expanded={expanded === r.name} onClick={() => setExpanded(expanded === r.name ? null : r.name)}>{expanded === r.name ? "▾" : "▸"} {r.name} {r.chips.map((c, i) => <span className="factory-chip" key={i}>{c}</span>)}</button>{expanded === r.name && <p>{r.pick || "asking route pick…"}</p>}</div>)}{s.decider && <p>{s.decider}</p>}<details><summary>Open routing table</summary><pre>{s.routes.map(r => `${r.name}: ${r.chips.join(" → ")}`).join("\n")}</pre></details></section>
    <section><h2>In flight</h2>{!s.flights.length && <p>nothing live</p>}{s.flights.map(r => <div className="factory-row" key={r.id}><span>{r.name}</span><span>{r.lane}</span><span>{r.tab}</span><span>{r.host}</span>{r.headless && <span className="factory-chip">headless</span>}<span className="factory-trailing">{r.age}</span></div>)}<p>Landed today: {s.landedCount}</p>{s.landed.map(r => <div className="factory-row" key={r.id}><span>{r.time}</span><span>{r.subject}</span></div>)}</section>
  </main>;
}
