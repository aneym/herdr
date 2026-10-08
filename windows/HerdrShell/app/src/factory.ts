export interface FactoryBundle { overlay: unknown; boxes: unknown; poolState: unknown; disk: unknown; routing: unknown; decider: unknown; flights: unknown; landed: string | null; picks: Record<string, string | null>; pools: unknown; poolsInterval: number; poolsAgeSeconds?: number }
type Obj = Record<string, unknown>;
const obj = (v: unknown): Obj => v !== null && typeof v === "object" && !Array.isArray(v) ? v as Obj : {};
const list = (v: unknown): unknown[] => Array.isArray(v) ? v : [];
const str = (v: unknown): string => typeof v === "string" ? v.trim() : "";
const num = (v: unknown): number | undefined => typeof v === "number" && Number.isFinite(v) ? v : undefined;
const date = (v: unknown): number | undefined => { const n = num(v); const d = n === undefined ? Date.parse(str(v)) : n > 1e12 ? n : n * 1000; return Number.isFinite(d) ? d : undefined; };
export const scrub = (s: string) => s.replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, "[redacted]").replace(/\b(?:sk-|ghp_|gho_|github_pat_|xox[baprs]-|AKIA)[A-Za-z0-9_-]{8,}/g, "[redacted]").replace(/\b(?:acct|account)[_-][A-Za-z0-9]{4,}\b/gi, "[redacted]");
const reason = (v: unknown) => scrub(str(v) || str(obj(v).reason));
export function age(seconds: number, compact = false): string { const s = Math.max(0, Math.floor(seconds)); const [n, u] = s < 60 ? [s, "s"] : s < 3600 ? [Math.floor(s / 60), "m"] : s < 86400 ? [Math.floor(s / 3600), "h"] : [Math.floor(s / 86400), "d"]; return `${n}${compact ? "" : " "}${u}`; }
const clock = (d: number, military = false) => new Intl.DateTimeFormat("en-US", { timeZone: "America/New_York", hour: military ? "2-digit" : "numeric", minute: "2-digit", hour12: !military }).format(d);
const percent = (n: number) => `${Math.abs(n - Math.round(n)) < .05 ? Math.round(n) : n.toFixed(1)}%`;
const keyed = (v: unknown) => new Map(list(v).map(obj).filter(d => str(d.name)).map(d => [str(d.name).toLowerCase(), d]));
export interface MachineRow { name: string; kind: string; summary: string; slots: string; disk: string; state: string; attention: string; dimmed: boolean; usageState: string; usageLine: string }
export interface PoolRow { id: string; provider: string; counts: string; fiveHour?: number; weekly?: number; fiveHourLabel: string; weeklyLabel: string; pace: string; monthly: string; refill: string; tone: string }
export interface FactorySnapshot { updated: string; machines: MachineRow[]; poolsAge: string; poolsStale: boolean; pools: PoolRow[]; ladderMode: string; routes: { name: string; chips: string[]; pick: string }[]; decider: string; flights: { id: string; name: string; lane: string; tab: string; host: string; age: string; headless: boolean }[]; landedCount: number; landed: { id: string; time: string; subject: string }[] }
function reset(d: number, now: number): string {
  const day = (v: number) => new Intl.DateTimeFormat("en-US", { timeZone: "America/New_York", dateStyle: "short" }).format(v);
  const time = clock(d).replace(":00", "");
  return `resets ${day(d) === day(now) ? "" : new Intl.DateTimeFormat("en-US", { timeZone: "America/New_York", weekday: "short" }).format(d) + " "}${time}`;
}
function chip(v: unknown): string { if (str(v)) return str(v); const d = obj(v), model = str(d.model) || str(d.id), effort = str(d.effort); return str(d.label) || str(d.chip) || (model && effort && !model.endsWith(effort) ? `${model}-${effort}` : model); }
export function buildFactorySnapshot(bundle: FactoryBundle | null, now = Date.now(), poolsFetched?: number, poolsStale = false): FactorySnapshot {
  const b = bundle ?? { overlay: null, boxes: null, poolState: null, disk: null, routing: null, decider: null, flights: null, landed: null, picks: {}, pools: null, poolsInterval: 60 };
  const hosts = keyed(obj(b.overlay).hosts), boxes = keyed(obj(b.boxes).boxes), state = obj(b.poolState), disk = obj(b.disk);
  const lower = (v: unknown) => Object.fromEntries(Object.entries(obj(v)).map(([k, val]) => [k.toLowerCase(), val]));
  const down = lower(state.down), downWhy = lower(state.down_why), drainedWhy = lower(state.drained_why), disks = lower(disk);
  const drained = new Set(list(state.drained).map(v => str(v).toLowerCase()));
  const machines = [...new Set([...hosts.keys(), ...boxes.keys()])].map(key => {
    const h = hosts.get(key) ?? {}, box = boxes.get(key) ?? {}, d = obj(disks[key]), last = obj(d.last_run), until = date(down[key]);
    const isHeld = typeof d.held === "string" ? !["", "false", "null", "0"].includes(d.held.trim().toLowerCase()) : Boolean(d.held);
    let status = "up", why = "";
    if (until !== undefined && until > now) { status = `down until ${clock(until, true)}`; why = reason(downWhy[key]); }
    else if (isHeld) status = "held";
    else if (drained.has(key)) { status = "drained"; why = reason(drainedWhy[key]); }
    if (why) status += ` (${why})`;
    let diskText = str(d.status); const free = num(last.free_gib), at = date(last.at);
    if (free !== undefined) diskText = `${free.toFixed(1)} GiB${str(d.status) ? " · " + str(d.status) : ""}${at === undefined ? "" : " · " + age((now - at) / 1000) + " ago"}`;
    const u = obj(h.usage), visible = num(u.age_s) !== undefined && Number(u.age_s) <= 30;
    const usage = visible ? [num(u.load_per_core) === undefined ? "" : `${Number(u.load_per_core).toFixed(2)}/core`, num(u.cpu_pct) === undefined ? "" : `${Math.round(Number(u.cpu_pct))}%`, num(u.mem_used_mb) !== undefined && num(u.mem_total_mb) !== undefined ? `${Math.round(Number(u.mem_used_mb))}/${Math.round(Number(u.mem_total_mb))} MB` : "", num(u.slots_used) !== undefined && num(u.slots_total) !== undefined ? `${Math.round(Number(u.slots_used))}/${Math.round(Number(u.slots_total))}` : ""].filter(Boolean).join("  ") : "";
    return { name: str(box.name) || str(h.name) || key, kind: str(box.kind), summary: scrub(str(h.summary)), slots: num(box.sessions) !== undefined && num(box.checks) !== undefined ? `${box.sessions}/${box.checks}` : "", disk: diskText, state: status, attention: str(h.attention), dimmed: box.enabled === false, usageState: visible ? str(u.state) : "", usageLine: usage };
  });
  const rank = (name: string) => { const n = name.toLowerCase(); return n === "studio" ? 0 : n === "pc" || n.startsWith("pc-") ? 1 : n === "ax42" ? 2 : n.startsWith("forge") ? 3 : n.includes("macbook") ? 4 : 5; };
  machines.sort((a, b) => rank(a.name) - rank(b.name) || a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  const pools = list(obj(b.pools).pools).map(obj).filter(p => str(p.id)).map(p => {
    const label = (v: unknown, resetAt: unknown) => { const n = num(v), d = date(resetAt); return n === undefined ? "" : percent(n) + (d === undefined ? "" : " · " + reset(d, now)); };
    const refill = obj(list(p.refills)[0]), at = date(refill.at), cycle = date(p.cycleResetAt);
    return { id: str(p.id), provider: str(p.provider), counts: num(p.usableAccounts) === undefined ? "" : `${p.usableAccounts}${num(p.totalAccounts) === undefined ? "" : "/" + p.totalAccounts}`, fiveHour: num(p.fiveHourRemainingPercent), weekly: num(p.weeklyRemainingPercent), fiveHourLabel: label(p.fiveHourRemainingPercent, p.fiveHourResetAt), weeklyLabel: label(p.weeklyRemainingPercent, p.weeklyResetAt), pace: num(p.weeklyPacePercent) === undefined ? "" : "pace " + percent(Number(p.weeklyPacePercent)), monthly: num(p.monthlyRemaining) === undefined ? "" : "monthly " + percent(Number(p.monthlyRemaining)) + (cycle === undefined ? "" : " · " + reset(cycle, now)), refill: num(refill.accounts) === undefined ? "" : `+${refill.accounts} ${refill.accounts === 1 ? "account" : "accounts"}${at === undefined ? "" : " " + clock(at)}`, tone: p.usableAccounts === 0 ? "red" : num(p.headroomPercent) !== undefined && Number(p.headroomPercent) < 15 ? "amber" : "ok" };
  });
  const table = obj(b.routing), mode = str(table.ladder_mode) || str(table.ladder);
  const chains = obj(table.interim_ladder ?? obj(table.ladders)[mode]);
  const order = ["implement", "mechanical", "explore", "review", "research", "verify", "plan", "computer", "council"];
  const routes = Object.keys(chains).sort((a, b) => (order.indexOf(a) < 0 ? 99 : order.indexOf(a)) - (order.indexOf(b) < 0 ? 99 : order.indexOf(b)) || a.localeCompare(b)).map(name => {
    const v = chains[name], rungs = obj(obj(v).rungs);
    return { name, chips: Array.isArray(v) ? v.map(chip).filter(Boolean) : Object.keys(rungs).sort().map(k => chip(rungs[k]) || k), pick: scrub(b.picks?.[name] ?? "") };
  });
  const dec = obj(b.decider), who = str(dec.decider), policy = str(dec.policy) || str(dec.policy_name), version = num(dec.version) === undefined ? str(dec.version) : String(dec.version);
  const decider = !who && !version ? "" : `Decider: ${who}${policy ? " (" + policy + ")" : ""}${version ? ", decider.json v" + version : ""}`;
  const flights = list(b.flights).map(obj).sort((a, c) => (date(c.started) ?? 0) - (date(a.started) ?? 0)).map(f => ({ id: str(f.run_id), name: scrub(str(f.name) || str(f.run_id)), lane: str(f.lane), tab: str(f.tab), host: str(f.host), age: date(f.started) === undefined ? "" : age((now - date(f.started)!) / 1000, true), headless: Boolean(f.headless) }));
  const landed = (b.landed ?? "").split("\n").flatMap(line => { const [id, seconds, ...subject] = line.split("\t"); return seconds && Number.isFinite(Number(seconds)) && subject.length ? [{ id, time: clock(Number(seconds) * 1000), subject: scrub(subject.join("\t")) }] : []; });
  const generated = date(obj(b.overlay).generated_at);
  return { updated: generated === undefined ? "updated —" : `updated ${age((now - generated) / 1000)} ago`, machines, poolsAge: poolsFetched === undefined ? "" : age((now - poolsFetched) / 1000) + " ago", poolsStale, pools, ladderMode: mode, routes, decider, flights, landedCount: landed.length, landed: landed.slice(0, 5) };
}
