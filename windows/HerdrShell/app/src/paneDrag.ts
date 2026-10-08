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
}
export interface PaneDragPress { pane: string; label: string; point: { x: number; y: number }; layout: Layout; size: { width: number; height: number }; supported: boolean }
const idle = (): PaneDragState => ({ phase: "idle", keyboard: false, source: null, label: "", pointer: null, sourceRect: null, zone: null, ghost: null, exact: false, pending: false });
type Placement = { changed: boolean; placed_rect?: Rect };
const zoneKey = (zone: PaneZone | null) => JSON.stringify(zone);
export function canDragPane(layout: Layout | undefined, supported: boolean): boolean { return supported && !!layout && !layout.zoomed && layout.panes.length >= 2; }
export async function probePlace(machine: string, paneId: string): Promise<boolean | null> {
  try { await bridge.api(machine, "pane.place", { pane_id: paneId, target: { type: "pane", pane_id: paneId }, side: "right", dry_run: true }); return true; }
  catch (error) { const code = (error as { code?: string })?.code; return code ? !["invalid_request", "unknown_method"].includes(code) : null; }
}
export function prefersReducedMotion(): boolean { return globalThis.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false; }
export function transitionFor(kind: "zone" | "settle" | "cancel" | "lift", reduced = prefersReducedMotion()): string {
  if (reduced) return "opacity var(--shell-motion-reduced-fade-ms) var(--shell-motion-ease)";
  const duration = kind === "zone" ? "zone-morph" : kind === "lift" ? "fade" : kind;
  return (kind === "lift" ? ["opacity"] : ["transform", "width", "height", "opacity"]).map(p => `${p} var(--shell-motion-${duration}-ms) var(--shell-motion-ease)`).join(", ");
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
    // The threshold event lifts the cap; target selection starts on the next move.
    if (!lifting) this.setZone(this.at(point));
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
  private setZone(zone: PaneZone | null, force = false) {
    if (!force && zoneKey(zone) === zoneKey(this.raw)) return;
    this.raw = zone;
    if (!zone || zone.kind === "into_tab" || zone.kind === "new_tab_in") { this.emit({ zone, ghost: null, exact: false, pending: false }); return; }
    if (zone.kind === "centre") { this.emit({ zone, ghost: zoneEstimateRect(this.area(), this.boxes, zone), exact: true, pending: false }); return; }
    const cached = this.cache.get(zoneKey(zone));
    if (cached) { this.answer(zone, cached); return; }
    this.emit({ zone, ghost: zoneEstimateRect(this.area(), this.boxes, zone), exact: false, pending: true }); this.pump();
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
    const generation = ++this.generation;
    this.emit({ phase: "dropped", pending: false, keyboard: false });
    this.timer = setTimeout(() => { if (generation === this.generation) this.cancel(); }, 4000);
    void bridge.api(this.machine, method, params).then(result => {
      if (generation !== this.generation) return;
      const answer = result as { place?: Placement; swap?: Placement; move_result?: Placement; changed?: boolean };
      if (answer?.place?.changed === false || answer?.swap?.changed === false || answer?.move_result?.changed === false || answer?.changed === false) this.cancel();
    }).catch(error => { if (generation === this.generation) { this.cancel(); this.options.onError?.(error); } });
    return "dropped";
  }
  cancel(): void { ++this.generation; clearTimeout(this.timer); this.cache.clear(); this.flight = false; this.raw = null; this.row = null; this.target = null; this.emit(idle()); }
  layoutChanged(layout: Layout | undefined): void {
    if (this.value.phase === "idle") return;
    if (!layout || layout.tab_id !== this.input!.layout.tab_id || !layout.panes.some(p => p.pane_id === this.value.source)) { this.cancel(); return; }
    if (JSON.stringify(layout.panes) === JSON.stringify(this.input!.layout.panes) && JSON.stringify(layout.area) === JSON.stringify(this.input!.layout.area) && layout.zoomed === this.input!.layout.zoomed) return;
    if (this.value.phase === "dropped" || layout.zoomed) { this.cancel(); return; }
    ++this.generation; this.flight = false; this.cache.clear(); this.input = { ...this.input!, layout }; this.rebuild();
    this.emit({ sourceRect: this.boxes.find(p => p.id === this.value.source)!.rect });
    this.setZone(this.value.keyboard ? this.raw : this.at(this.value.pointer!), true);
  }
}
