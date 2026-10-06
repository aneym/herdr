import type { Layout, Rect } from "./model";
// Split dividers, as the Mac Shell's PaneDivider and PaneResizeController: a divider comes from
// herdr's own layout (`layouts[].splits` and the pane rects on either side), and a drag turns
// into `pane.resize` calls. The app keeps no layout of its own.
export interface Divider {
  splitId: string;
  /** A vertical line (children side by side, split "right"), else horizontal (split "down"). */
  vertical: boolean;
  ratio: number;
  splitRect: Rect;
  /** A pane on each side of the line that touches it. */
  firstPane: string;
  secondPane: string;
  /** Line position in layout units, along the axis it moves on. */
  pos: number;
}
export const extent = (d: Divider) => d.vertical ? d.splitRect.width : d.splitRect.height;
/** The herdr pane and direction that move the line toward `delta` > 0 (right/down) or < 0:
 * growing the first child acts on a first-side pane, shrinking it on a second-side pane. */
export function resizeCall(d: Divider, delta: number): { pane: string; direction: "left" | "right" | "up" | "down" } {
  return delta > 0 ? { pane: d.firstPane, direction: d.vertical ? "right" : "down" } : { pane: d.secondPane, direction: d.vertical ? "left" : "up" };
}
export function dividers(layout: Layout): Divider[] {
  const tol = 1.5;
  const inside = (r: Rect, o: Rect) => r.x >= o.x - .5 && r.y >= o.y - .5 && r.x + r.width <= o.x + o.width + .5 && r.y + r.height <= o.y + o.height + .5;
  const overlap = (a0: number, al: number, b0: number, bl: number) => Math.min(a0 + al, b0 + bl) - Math.max(a0, b0) > 0;
  const out: Divider[] = [];
  for (const sp of layout.splits ?? []) {
    const vertical = sp.direction === "right";
    const pos = vertical ? sp.rect.x + sp.rect.width * sp.ratio : sp.rect.y + sp.rect.height * sp.ratio;
    const panes = layout.panes.filter(p => inside(p.rect, sp.rect));
    let found: Divider | undefined;
    for (const a of panes) {
      for (const b of panes) {
        if (a.pane_id === b.pane_id) continue;
        const adjacent = vertical
          ? Math.abs(a.rect.x + a.rect.width - b.rect.x) <= tol && overlap(a.rect.y, a.rect.height, b.rect.y, b.rect.height)
          : Math.abs(a.rect.y + a.rect.height - b.rect.y) <= tol && overlap(a.rect.x, a.rect.width, b.rect.x, b.rect.width);
        const line = vertical ? b.rect.x : b.rect.y;
        if (adjacent && Math.abs(line - pos) <= tol) { found = { splitId: sp.id, vertical, ratio: sp.ratio, splitRect: sp.rect, firstPane: a.pane_id, secondPane: b.pane_id, pos: line }; break; }
      }
      if (found) break;
    }
    if (found) out.push(found);
  }
  return out;
}
export type ResizeAnswer = { changed: boolean; layout: Layout } | null;
/** Turns a divider drag into `pane.resize` requests, one in flight at a time. The target ratio
 * is absolute (ratio at the press plus pixels moved over the split's pixel size), and each
 * request sends only what is still missing from the ratio herdr last reported, so rounding,
 * clamping and slow replies never accumulate. */
export class Resizer {
  private dragging = false;
  private inFlight = false;
  private startRatio = .5;
  private want: { divider: Divider; ratio: number } | null = null;
  constructor(
    private request: (pane: string, direction: string, amount: number) => Promise<ResizeAnswer>,
    /** Latest ratio herdr reported for a split id. */
    private currentRatio: (splitId: string) => number | undefined,
    /** A layout herdr answered with; panes are drawn from it while the drag holds snapshots. */
    private onLayout: (layout: Layout) => void,
    /** Once the drag has ended and the last request has come back. */
    private onIdle: () => void,
  ) {}
  get busy() { return this.dragging || this.inFlight; }
  began(d: Divider) { this.dragging = true; this.startRatio = this.currentRatio(d.splitId) ?? d.ratio; this.want = { divider: d, ratio: this.startRatio }; }
  moved(d: Divider, deltaPx: number, extentPx: number) {
    if (extentPx <= 0) return;
    this.want = { divider: d, ratio: Math.min(.9, Math.max(.1, this.startRatio + deltaPx / extentPx)) };
    this.pump();
  }
  ended(d: Divider, deltaPx: number, extentPx: number) {
    if (extentPx > 0) this.want = { divider: d, ratio: Math.min(.9, Math.max(.1, this.startRatio + deltaPx / extentPx)) };
    this.dragging = false;
    this.pump();
    if (!this.inFlight) this.finish();
  }
  private finish() { this.want = null; this.onIdle(); }
  private pump() {
    if (this.inFlight || !this.want) return;
    const { divider, ratio } = this.want;
    const delta = ratio - (this.currentRatio(divider.splitId) ?? divider.ratio);
    if (Math.abs(delta) < (this.dragging ? .004 : .0005)) return;
    const call = resizeCall(divider, delta);
    this.inFlight = true;
    void this.request(call.pane, call.direction, Math.abs(delta)).catch(() => null).then(answer => {
      this.inFlight = false;
      if (answer) {
        this.onLayout(answer.layout);
        // A resize herdr clamped or refused would repeat forever: stop chasing it.
        if (!answer.changed) this.want = null;
      } else this.want = null;
      this.pump();
      if (!this.inFlight && !this.dragging) this.finish();
    });
  }
}
