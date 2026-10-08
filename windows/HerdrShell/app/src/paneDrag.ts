import { bridge } from "./bridge";
import { scaleRect } from "./model";
import type { Layout, Pane, Rect } from "./model";
import { dropZoneAt, neighbour, pixelMetrics, zoneEstimateRect } from "./paneDrop";
import type { DropSide, DropZone, PaneBox } from "./paneDrop";
import { motion } from "./tokens";
export type RowTarget = { kind: "tab"; tab_id: string } | { kind: "space"; workspace_id: string };
export type PaneZone = DropZone | { kind: "into_tab"; tab_id: string } | { kind: "new_tab_in"; workspace_id: string };
export interface PaneDragState {
  phase: "idle" | "pressed" | "dragging" | "dropped"; keyboard: boolean; source: string | null; label: string;
  pointer: { x: number; y: number } | null; sourceRect: Rect | null; zone: PaneZone | null; ghost: Rect | null; exact: boolean; pending: boolean;
  /** How the last drag ended: `cancel` springs the chip back, `settle` and `quiet` fade it. */
  end: "cancel" | "settle" | "quiet" | null;
}
export interface PaneDragPress { pane: string; label: string; point: { x: number; y: number }; layout: Layout; size: { width: number; height: number }; supported: boolean }
const idle = (): PaneDragState => ({ phase: "idle", keyboard: false, source: null, label: "", pointer: null, sourceRect: null, zone: null, ghost: null, exact: false, pending: false, end: null });
type Placement = { changed: boolean; placed_rect?: Rect };
// The drop's reply: pane.place, pane.swap or pane.move, each with the server's layouts after the change.
type DropReply = { place?: DropResult; swap?: DropResult; move_result?: DropResult; changed?: boolean } | null | undefined;
type DropResult = { changed: boolean; layout?: Layout; target_layout?: Layout; source_layout?: Layout | null };
const zoneKey = (zone: PaneZone | null) => JSON.stringify(zone);
const paneSet = (layout: Layout) => layout.panes.map(p => p.pane_id).sort().join("\n");
const geometry = (layout: Layout) => JSON.stringify([layout.area, layout.zoomed ?? false, [...layout.panes].sort((a, b) => a.pane_id < b.pane_id ? -1 : 1).map(p => [p.pane_id, p.rect])]);
export function canDragPane(layout: Layout | undefined, supported: boolean): boolean { return supported && !!layout && !layout.zoomed && layout.panes.length >= 2; }
export async function probePlace(machine: string, paneId: string): Promise<boolean | null> {
  try { await bridge.api(machine, "pane.place", { pane_id: paneId, target: { type: "pane", pane_id: paneId }, side: "right", dry_run: true }); return true; }
  catch (error) { const code = (error as { code?: string })?.code; return code ? !["invalid_request", "unknown_method"].includes(code) : null; }
}
export function prefersReducedMotion(): boolean { return globalThis.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false; }
export function transitionFor(kind: "zone" | "settle" | "cancel" | "lift", reduced = prefersReducedMotion()): string {
  if (reduced) return "opacity var(--shell-motion-reduced-fade-ms) var(--shell-motion-ease)";
  const duration = kind === "zone" ? "--shell-motion-zone-morph-ms" : kind === "lift" ? "--shell-motion-fade-ms" : kind === "settle" ? "--shell-motion-settle-ms" : "--shell-motion-cancel-ms";
  return (kind === "lift" ? ["opacity"] : ["transform", "width", "height", "opacity"]).map(p => `${p} var(${duration}) var(--shell-motion-ease)`).join(", ");
}
export class PaneDrag {
  private value = idle();
  private input?: PaneDragPress;
  private boxes: PaneBox[] = [];
  private generation = 0;
  private cache = new Map<string, Placement>();
  private flight = false;
  private raw: PaneZone | null = null;
  private row: RowTarget | null = null;
  private target: string | null = null;
  private timer?: ReturnType<typeof setTimeout>;
  private springTimer?: ReturnType<typeof setTimeout>;
  private dwell?: { tab: string; point: { x: number; y: number } };
  private origin?: string;
  private springTab?: string;
  // The newest snapshot layout and the tab's pane descriptors. While a sent drop waits for its reply, snapshots only
  // land here (the newest wins) and the tab keeps showing `frozen`, its layout at the release, with the panes it had
  // then (`frozenPanes`): no box moves, appears or leaves and no terminal resizes until the drop ends.
  private latest?: Layout;
  private livePanes: Pane[] = [];
  private frozen?: Layout;
  private frozenPanes: Pane[] = [];
  private latestAtRelease?: Layout;
  // An accepted drop: the layout its panes settle to, shown until the next snapshot replaces `base`, and what the
  // settle may animate across (the pane set and host size at the release). `settling` lasts until the latest
  // retarget's transition has ended (`settleEnded`) or its timer fires, so any snapshot before then retargets it.
  private settled?: { base: Layout | undefined; layout: Layout; panes: string; size: { width: number; height: number } };
  private settling = false;
  private settleTimer?: ReturnType<typeof setTimeout>;
  constructor(private machine: string, private options: { onChange?: (state: PaneDragState) => void; onError?: (error: unknown) => void; onSpring?: (tabId: string) => void } = {}) {}
  get state(): PaneDragState { return this.value; }
  private emit(patch: Partial<PaneDragState>) {
    const next = { ...this.value, ...patch };
    if (JSON.stringify(next) === JSON.stringify(this.value)) return;
    this.value = next; this.options.onChange?.(next);
  }
  private area(): Rect { return { x: 0, y: 0, ...this.input!.size }; }
  private rebuild() { const i = this.input!; this.boxes = i.layout.panes.map(p => ({ id: p.pane_id, rect: scaleRect(p.rect, i.layout.area, i.size.width, i.size.height) })); }
  press(input: PaneDragPress): boolean {
    // A press ends a drop still waiting on its reply, quietly, and the new drag starts from the layout that shows
    // after that (the buffered snapshot, or a settle's target), never the release-time one the press was aimed at.
    if (this.value.phase === "dropped") this.finish("quiet");
    const shown = this.shownLayout(this.latest);
    if (shown?.tab_id === input.layout.tab_id) input = { ...input, layout: shown };
    if (!canDragPane(input.layout, input.supported) || !input.layout.panes.some(p => p.pane_id === input.pane)) return false;
    this.cancel(); this.origin = input.layout.tab_id; this.input = input; this.rebuild(); this.target = input.pane;
    this.emit({ phase: "pressed", source: input.pane, label: input.label, pointer: input.point, sourceRect: this.boxes.find(p => p.id === input.pane)!.rect }); return true;
  }
  lift(input: Omit<PaneDragPress, "point">): boolean {
    if (!this.press({ ...input, point: { x: 0, y: 0 } })) return false;
    this.emit({ phase: "dragging", keyboard: true, pointer: null });
    this.target = (["right", "down", "left", "up"] as DropSide[]).map(s => neighbour(this.boxes, input.pane, s)).find(Boolean) ?? input.pane;
    this.setZone(this.target === input.pane ? null : { kind: "centre", target: this.target }); return true;
  }
  move(point: { x: number; y: number }, row: RowTarget | null = null): void {
    if (!["pressed", "dragging"].includes(this.value.phase) || this.value.keyboard) return;
    if (this.value.phase === "pressed" && Math.hypot(point.x - this.input!.point.x, point.y - this.input!.point.y) < motion.dragThresholdPx) { this.emit({ pointer: point }); return; }
    const lifting = this.value.phase === "pressed";
    this.row = row; this.emit({ phase: "dragging", pointer: point });
    this.setZone(this.at(point), false, lifting);
    this.dwellOn(point, row);
  }
  private clearDwell() { clearTimeout(this.springTimer); this.dwell = undefined; }
  private dwellOn(point: { x: number; y: number }, row: RowTarget | null) {
    const tab = row?.kind === "tab" ? row.tab_id : undefined;
    if (!tab || tab === this.input!.layout.tab_id || tab === this.springTab) { this.clearDwell(); return; }
    if (this.dwell?.tab === tab && Math.hypot(point.x - this.dwell.point.x, point.y - this.dwell.point.y) < motion.dragThresholdPx) return;
    this.clearDwell(); this.dwell = { tab, point };
    this.springTimer = setTimeout(() => {
      this.clearDwell();
      if (this.value.phase !== "dragging") return;
      this.springTab = tab;
      const generation = this.generation;
      void bridge.api(this.machine, "tab.focus", { tab_id: tab }).catch(error => {
        if (generation !== this.generation) return;
        this.cancel(); this.options.onError?.(error);
      });
      // Mark the switch before notifying the view: its next layout belongs to this spring, not an external switch.
      this.options.onSpring?.(tab);
    }, motion.springLoadMs);
  }
  private at(point: { x: number; y: number }): PaneZone | null {
    if (this.row) return this.row.kind === "tab" ? this.row.tab_id === this.input!.layout.tab_id ? null : { kind: "into_tab", tab_id: this.row.tab_id } : { kind: "new_tab_in", workspace_id: this.row.workspace_id };
    const i = this.input!;
    return dropZoneAt(this.area(), this.boxes, i.pane, point, pixelMetrics({ width: i.size.width / i.layout.area.width, height: i.size.height / i.layout.area.height }));
  }
  key(side: DropSide, edge: boolean): void {
    if (this.value.phase !== "dragging" || !this.value.keyboard) return;
    if (!edge) this.target = neighbour(this.boxes, this.target!, side) ?? this.target;
    this.setZone(edge ? this.target === this.value.source ? { kind: "tab_edge", side } : { kind: "pane_edge", target: this.target!, side } : this.target === this.value.source ? null : { kind: "centre", target: this.target! });
  }
  private setZone(zone: PaneZone | null, force = false, deferProbe = false) {
    if (!force && zoneKey(zone) === zoneKey(this.raw)) return;
    this.raw = zone;
    if (!zone || zone.kind === "into_tab" || zone.kind === "new_tab_in") { this.emit({ zone, ghost: null, exact: false, pending: false }); return; }
    if (zone.kind === "centre") { this.emit({ zone, ghost: zoneEstimateRect(this.area(), this.boxes, zone), exact: true, pending: false }); return; }
    const cached = this.cache.get(zoneKey(zone));
    if (cached) { this.answer(zone, cached); return; }
    this.emit({ zone, ghost: zoneEstimateRect(this.area(), this.boxes, zone), exact: false, pending: true });
    // Select immediately on lift; let a same-event release supersede the preview request.
    if (deferProbe) {
      const generation = this.generation;
      queueMicrotask(() => { if (generation === this.generation) this.pump(); });
    } else this.pump();
  }
  private place(zone: Exclude<DropZone, { kind: "centre" }>) {
    return { pane_id: this.value.source, target: zone.kind === "pane_edge" ? { type: "pane", pane_id: zone.target } : { type: "tab", tab_id: this.input!.layout.tab_id }, side: zone.side };
  }
  private answer(zone: PaneZone, answer: Placement) {
    if (!answer.changed) this.emit({ zone: null, ghost: null, exact: false, pending: false });
    else this.emit({ zone, ...(answer.placed_rect ? { ghost: scaleRect(answer.placed_rect, this.input!.layout.area, this.input!.size.width, this.input!.size.height), exact: true } : {}), pending: false });
  }
  private pump() {
    const zone = this.raw;
    if (this.flight || this.value.phase !== "dragging" || !zone || (zone.kind !== "pane_edge" && zone.kind !== "tab_edge") || this.cache.has(zoneKey(zone))) return;
    const generation = this.generation; this.flight = true;
    void bridge.api(this.machine, "pane.place", { ...this.place(zone), dry_run: true }).then(result => {
      if (generation !== this.generation) return;
      const answer = (result as { place: Placement }).place;
      this.cache.set(zoneKey(zone), answer);
      if (zoneKey(this.raw) === zoneKey(zone)) this.answer(zone, answer);
    }).catch(() => { if (generation === this.generation && zoneKey(this.raw) === zoneKey(zone)) this.emit({ pending: false }); }).finally(() => {
      if (generation !== this.generation) return;
      this.flight = false;
      if (zoneKey(this.raw) !== zoneKey(zone)) this.pump();
    });
  }
  release(): "click" | "dropped" | "cancelled" | "none" {
    if (this.value.phase === "idle" || this.value.phase === "dropped") return "none";
    if (this.value.phase === "pressed") { this.cancel(); return "click"; }
    const zone = this.value.zone;
    if (!zone) { this.cancel(); return "cancelled"; }
    const source = this.value.source;
    let method: string, params: object;
    if (zone.kind === "centre") { method = "pane.swap"; params = { source_pane_id: source, target_pane_id: zone.target }; }
    else if (zone.kind === "new_tab_in") { method = "pane.move"; params = { pane_id: source, destination: { type: "new_tab", workspace_id: zone.workspace_id }, focus: true }; }
    else { method = "pane.place"; params = { ...(zone.kind === "into_tab" ? { pane_id: source, target: { type: "tab", tab_id: zone.tab_id }, side: "right" } : this.place(zone)), focus: true, dry_run: false }; }
    this.clearDwell();
    const generation = ++this.generation;
    this.frozen = this.input!.layout; this.latestAtRelease = this.latest;
    this.frozenPanes = this.livePanes.filter(p => this.frozen!.panes.some(lp => lp.pane_id === p.pane_id));
    this.settled = undefined; this.settling = false; clearTimeout(this.settleTimer);
    this.emit({ phase: "dropped", pending: false, keyboard: false });
    // No reply in time: the server may still have applied the drop, so end without a rollback.
    this.timer = setTimeout(() => { if (generation === this.generation) this.finish("quiet"); }, 4000);
    void bridge.api(this.machine, method, params).then(result => {
      if (generation !== this.generation) return;
      const reply = result as DropReply, answer = reply?.place ?? reply?.swap ?? reply?.move_result;
      if (answer?.changed === false || reply?.changed === false) { this.finish("cancel"); return; }
      if (!answer) { this.finish("quiet"); return; }
      // Settle once from the frozen boxes: to the newest snapshot if one that moved panes came while the drop was
      // pending (a title or status snapshot does not count), else to the reply's layout for this tab. Either way the
      // final layout is never drawn before the settle starts.
      const frozen = this.frozen!, latest = this.latest;
      const buffered = !!latest && latest !== this.latestAtRelease && geometry(latest) !== geometry(frozen);
      const replied = [answer.layout, answer.target_layout, answer.source_layout].find(l => l?.tab_id === frozen.tab_id);
      const target = buffered || !replied ? latest : replied;
      if (target) {
        this.settled = { base: latest, layout: target, panes: paneSet(frozen), size: { ...this.input!.size } };
        this.armSettle();
      }
      this.finish("settle");
    }).catch(error => {
      if (generation !== this.generation) return;
      // A server error (it carries a code) moved nothing; a lost connection leaves the outcome unknown.
      this.finish((error as { code?: string })?.code ? "cancel" : "quiet");
      this.options.onError?.(error);
    });
    return "dropped";
  }
  /** Esc, a right click, a release on no zone or the view going away. A drop already sent ends quietly. */
  cancel(): void { this.finish(this.value.phase === "dragging" ? "cancel" : this.value.phase === "dropped" ? "quiet" : null); }
  /** The host changed size (a window resize, a sidebar toggle or resize). A pending drop holds its release-time boxes,
   * so it ends quietly first: the buffered snapshot is then drawn and its terminals fitted at the new size. */
  hostResized(): void { if (this.value.phase === "dropped") this.finish("quiet"); }
  /** Every end unfreezes the tab: a refusal, the timeout or a quiet end shows the buffered snapshot as it is. */
  private finish(end: PaneDragState["end"]) {
    const restore = end === "cancel" && this.springTab && this.springTab !== this.origin ? this.origin : undefined;
    this.clearDwell(); this.springTab = undefined; this.origin = undefined;
    if (restore) {
      void bridge.api(this.machine, "tab.focus", { tab_id: restore }).catch(error => this.options.onError?.(error));
      this.options.onSpring?.(restore);
    }
    ++this.generation; clearTimeout(this.timer); this.frozen = undefined; this.frozenPanes = []; this.cache.clear(); this.flight = false; this.raw = null; this.row = null; this.target = null; this.emit({ ...idle(), end }); }
  /** What the tab shows: the release-time layout while a drop is pending, then an accepted drop's settle target until
   * the next snapshot replaces `layout`. */
  shownLayout(layout: Layout | undefined): Layout | undefined {
    if (!layout) return layout;
    if (this.frozen?.tab_id === layout.tab_id) return this.frozen;
    return this.settled?.base === layout ? this.settled.layout : layout;
  }
  /** The tab's pane descriptors from the newest snapshot. */
  panesChanged(panes: Pane[]): void { this.livePanes = panes; }
  /** The panes the tab draws: while a drop is pending, the ones it had at the release, even one another client closed
   * or moved away meanwhile; a snapshot updates only their titles, status and terminals. */
  shownPanes(panes: Pane[]): Pane[] {
    if (!this.frozen) return panes;
    const live = new Map(panes.map(p => [p.pane_id, p]));
    return this.frozenPanes.map(p => { const now = live.get(p.pane_id); return now ? { ...now, tab_id: p.tab_id } : p; });
  }
  /** A settle (re)started: snapshots retarget it until its transition ends, or at the latest its full duration on. */
  /** The timer is only a backstop for a settle no pane box moves in: once one moves, its transition ends it. */
  private armSettle() {
    this.settling = true; clearTimeout(this.settleTimer);
    if (!this.boxesMoving) this.settleTimer = setTimeout(() => { this.settling = false; }, motion.settleMs + 50);
  }
  private boxesMoving = false;
  /** A pane box began its settle; its transition, timed from when it really starts, now decides the end. */
  settleMoving(): void { this.boxesMoving = true; clearTimeout(this.settleTimer); }
  /** Every pane box that moved in the settle has finished moving: the settle is over. */
  settleEnded(): void { this.boxesMoving = false; this.settling = false; clearTimeout(this.settleTimer); }
  /** Only an accepted drop settles, once, and a snapshot during that settle retargets it. As on the Mac, nothing
   * animates across a different pane set, a host size change (a live window resize) or a divider drag. */
  shouldAnimateLayout(layout: Layout | undefined, size?: { width: number; height: number }, busy = false): boolean {
    const s = this.settled;
    return !!layout && !!s && this.settling && !busy && !layout.zoomed && layout.tab_id === s.layout.tab_id
      && paneSet(layout) === s.panes && (!size || (size.width === s.size.width && size.height === s.size.height));
  }
  layoutChanged(layout: Layout | undefined): void {
    const before = this.settling ? this.shownLayout(this.latest) : undefined;
    this.latest = layout;
    // A snapshot that moves panes mid-settle retargets it, so the settle runs on from now.
    const after = before && this.shownLayout(layout);
    if (before && after && geometry(before) !== geometry(after)) this.armSettle();
    // A pending drop ends only with its reply or the timeout; snapshots meanwhile are buffered in `latest`. Leaving its
    // tab ends it quietly: the chip and ghost belong to that tab, and the server may still apply the drop.
    if (this.value.phase === "dropped" && layout?.tab_id !== this.input!.layout.tab_id) { this.finish("quiet"); return; }
    if (this.value.phase === "idle" || this.value.phase === "dropped") return;
    const springLayout = !!layout && layout.tab_id === this.springTab;
    if (!layout || (!springLayout && (layout.tab_id !== this.input!.layout.tab_id || !layout.panes.some(p => p.pane_id === this.value.source)))) { this.cancel(); return; }
    if (layout.tab_id === this.input!.layout.tab_id && JSON.stringify(layout.panes) === JSON.stringify(this.input!.layout.panes) && JSON.stringify(layout.area) === JSON.stringify(this.input!.layout.area) && layout.zoomed === this.input!.layout.zoomed) return;
    if (layout.zoomed) { this.cancel(); return; }
    ++this.generation; this.flight = false; this.cache.clear(); this.input = { ...this.input!, layout }; this.rebuild();
    this.emit({ sourceRect: this.boxes.find(p => p.id === this.value.source)?.rect ?? this.value.sourceRect });
    if (this.value.phase === "dragging") this.setZone(this.value.keyboard ? this.raw : this.at(this.value.pointer!), true);
  }
}
