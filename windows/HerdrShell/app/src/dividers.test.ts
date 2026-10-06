import { describe, expect, it } from "vitest";
import type { Layout } from "./model";
import { dividers, Resizer } from "./dividers";
import type { ResizeAnswer } from "./dividers";
// Divider geometry and the resize chase are pure rules with edge cases (nested splits,
// stale ratios, clamped answers, abandoned drags) a live drag cannot pin down; each wrong
// answer resizes a real pane past where the pointer let go. The e2e check drives one drag.
const layout = (ratio: number): Layout => ({
  tab_id: "t", area: { x: 0, y: 0, width: 100, height: 40 },
  panes: [
    { pane_id: "a", rect: { x: 0, y: 0, width: 100 * ratio, height: 40 } },
    { pane_id: "b", rect: { x: 100 * ratio, y: 0, width: 100 - 100 * ratio, height: 20 } },
    { pane_id: "c", rect: { x: 100 * ratio, y: 20, width: 100 - 100 * ratio, height: 20 } },
  ],
  splits: [
    { id: "root", direction: "right", ratio, rect: { x: 0, y: 0, width: 100, height: 40 } },
    { id: "inner", direction: "down", ratio: .5, rect: { x: 100 * ratio, y: 0, width: 100 - 100 * ratio, height: 40 } },
  ],
});
// Herdr's side: apply a resize to the ratio it holds and answer, when flushed, with the new layout.
function rig(start: number, clampAt = .9) {
  const s = { ratio: start, calls: [] as [string, string, number][], hold: null as null | (() => void) };
  const shown = layout(start);
  let idle = 0;
  const request = (pane: string, direction: string, amount: number) => {
    s.calls.push([pane, direction, amount]);
    const next = Math.min(clampAt, s.ratio + (direction === "right" ? amount : -amount));
    const changed = next !== s.ratio;
    s.ratio = next;
    return new Promise<ResizeAnswer>(resolve => { s.hold = () => resolve({ changed, layout: layout(next) }); });
  };
  // The shown layout lags, as React state does until the next render.
  const r = new Resizer(request, id => shown.splits?.find(x => x.id === id)?.ratio, () => {}, () => { idle++; });
  return { s, r, idle: () => idle };
}
const flush = async (s: { hold: null | (() => void) }) => { while (s.hold) { const h = s.hold; s.hold = null; h(); await new Promise(r => setTimeout(r, 0)); } };
describe("dividers", () => {
  it("finds each split's line and the panes on either side", () => {
    expect(dividers(layout(.5)).map(d => [d.splitId, d.vertical, d.pos, d.firstPane, d.secondPane])).toEqual([["root", true, 50, "a", "b"], ["inner", false, 20, "b", "c"]]);
    expect(dividers({ ...layout(.5), splits: [] })).toEqual([]);
  });
  it("sends only the ratio still missing, one request at a time", async () => {
    const { s, r, idle } = rig(.5);
    const d = dividers(layout(.5))[0];
    r.began(d);
    r.moved(d, 50, 500);
    r.moved(d, 60, 500);
    expect(s.calls.length).toBe(1);
    r.ended(d, 50, 500);
    await flush(s);
    expect(s.ratio).toBeCloseTo(.6);
    expect(s.calls.length).toBe(1);
    expect(idle()).toBe(1);
    expect(r.busy).toBe(false);
  });
  it("shrinks from the second side and stops chasing a clamped answer", async () => {
    const left = rig(.5);
    const d = dividers(layout(.5))[0];
    left.r.began(d); left.r.ended(d, -100, 500); await flush(left.s);
    expect(left.s.calls[0].slice(0, 2)).toEqual(["b", "left"]);
    expect(left.s.ratio).toBeCloseTo(.3);
    const clamped = rig(.5, .55);
    clamped.r.began(d); clamped.r.ended(d, 200, 500); await flush(clamped.s);
    expect(clamped.s.ratio).toBeCloseTo(.55);
    expect(clamped.s.calls.length).toBe(2);
  });
  it("ignores the reply to a cancelled drag", async () => {
    const { s, r, idle } = rig(.5);
    const d = dividers(layout(.5))[0];
    r.began(d); r.moved(d, 50, 500);
    r.cancel();
    await flush(s);
    expect(s.calls.length).toBe(1);
    expect(idle()).toBe(0);
    expect(r.busy).toBe(false);
  });
});
