import { bridge } from "./bridge";
import { scaleRect } from "./model";
import type { Layout, Rect } from "./model";
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
const geometry = (layout: Layout) => JSON.stringify([layout.area, layout.zoomed ?? false, [...layout.panes].sort((a, b) => a.pane_id < b.pane_id ? -1 : 1).map(p => [p.pane_id, p.rect])]);
const sameGeometry = (a: Layout, b: Layout) => geometry(a) === geometry(b);
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
  // The newest snapshot layout, and the layouts a drop's reply returned with the snapshot layout they replace. The view
  // shows the reply until the next snapshot arrives, which then wins as it is, without motion.
  private latest?: Layout;
  // `from` is the tab's layout at the release, when the drop's own snapshot came first and already shows the reply:
  // the panes then settle from where they stood at the drop. `id` marks each reply so the view starts its settle once.
  private settled?: { base: Layout | undefined; layouts: Layout[]; from?: Layout; id: number };
  private settles = 0;
  constructor(private machine: string, private options: { onChange?: (state: PaneDragState) => void; onError?: (error: unknown) => void } = {}) {}
  get state(): PaneDragState { return this.value; }
  private emit(patch: Partial<PaneDragState>) {
    const next = { ...this.value, ...patch };
    if (JSON.stringify(next) === JSON.stringify(this.value)) return;
    this.value = next; this.options.onChange?.(next);
  }
  private area(): Rect { return { x: 0, y: 0, ...this.input!.size }; }
  private rebuild() { const i = this.input!; this.boxes = i.layout.panes.map(p => ({ id: p.pane_id, rect: scaleRect(p.rect, i.layout.area, i.size.width, i.size.height) })); }
  press(input: PaneDragPress): boolean {
    if (!canDragPane(input.layout, input.supported) || !input.layout.panes.some(p => p.pane_id === input.pane)) return false;
    this.cancel(); this.input = input; this.rebuild(); this.target = input.pane;
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
    const generation = ++this.generation, before = this.input!.layout;
    this.emit({ phase: "dropped", pending: false, keyboard: false });
    // No reply in time: the server may still have applied the drop, so end without a rollback.
    this.timer = setTimeout(() => { if (generation === this.generation) this.finish("quiet"); }, 4000);
    void bridge.api(this.machine, method, params).then(result => {
      if (generation !== this.generation) return;
      const reply = result as DropReply, answer = reply?.place ?? reply?.swap ?? reply?.move_result;
      if (answer?.changed === false || reply?.changed === false) { this.finish("cancel"); return; }
      if (!answer) { this.finish("quiet"); return; }
      const layouts = [answer.layout, answer.target_layout, answer.source_layout].filter((l): l is Layout => !!l);
      const shown = this.latest, replied = layouts.find(l => l.tab_id === shown?.tab_id);
      this.settled = { base: shown, layouts, id: ++this.settles, ...(shown && replied && sameGeometry(shown, replied) ? { from: before } : {}) };
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
  private finish(end: PaneDragState["end"]) { ++this.generation; clearTimeout(this.timer); this.cache.clear(); this.flight = false; this.raw = null; this.row = null; this.target = null; this.emit({ ...idle(), end }); }
  /** What the tab shows: a drop reply's layout from the moment it lands until the next snapshot replaces `layout`. */
  shownLayout(layout: Layout | undefined): Layout | undefined {
    if (!layout || !this.settled || this.settled.base !== layout) return layout;
    return this.settled.layouts.find(l => l.tab_id === layout.tab_id) ?? layout;
  }
  /** Where a reply's settle starts when the drop's own snapshot already drew its layout; else the panes as shown. */
  settleFrom(layout: Layout | undefined): { from?: Layout; id: number } {
    const settled = this.settled;
    if (!layout || !settled?.layouts.includes(layout)) return { id: settled?.id ?? 0 };
    return { id: settled.id, ...(settled.from?.tab_id === layout.tab_id ? { from: settled.from } : {}) };
  }
  /** Only a drop reply's layout settles; snapshots, pending drop or not, apply at once. */
  shouldAnimateLayout(layout: Layout | undefined): boolean { return !!layout && !!this.settled?.layouts.includes(layout); }
  layoutChanged(layout: Layout | undefined): void {
    this.latest = layout;
    // A pending drop ends only with its reply or the timeout; snapshots meanwhile apply as they come. Leaving its tab
    // ends it quietly: the chip and ghost belong to that tab, and the server may still apply the drop.
    if (this.value.phase === "dropped" && layout?.tab_id !== this.input!.layout.tab_id) { this.finish("quiet"); return; }
    if (this.value.phase === "idle" || this.value.phase === "dropped") return;
    if (!layout || layout.tab_id !== this.input!.layout.tab_id || !layout.panes.some(p => p.pane_id === this.value.source)) { this.cancel(); return; }
    if (JSON.stringify(layout.panes) === JSON.stringify(this.input!.layout.panes) && JSON.stringify(layout.area) === JSON.stringify(this.input!.layout.area) && layout.zoomed === this.input!.layout.zoomed) return;
    if (layout.zoomed) { this.cancel(); return; }
    ++this.generation; this.flight = false; this.cache.clear(); this.input = { ...this.input!, layout }; this.rebuild();
    this.emit({ sourceRect: this.boxes.find(p => p.id === this.value.source)!.rect });
    if (this.value.phase === "dragging") this.setZone(this.value.keyboard ? this.raw : this.at(this.value.pointer!), true);
  }
}
