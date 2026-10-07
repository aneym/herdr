import { describe, expect, it } from "vitest";
import type { Rect } from "./model";
import { dropZoneAt, zoneEstimateRect } from "./paneDrop";
import type { DropMetrics, DropZone } from "./paneDrop";
import raw from "../../../../shell/fixtures/pane-drop-zones.json";
// Owner-written scenario for pane drag S7 (spec pane-drag-rearrange-2026-10-07). The drop-zone
// rule is a pure geometry algorithm with many interacting edge cases (tab edge first, per-axis
// bands with a min and a max, corner ties, gap cells, the source). The TUI, the Mac and this port
// all run the one shared golden fixture, so a port that drifts from the others fails here.
// The fixture is in layout cells; the gap reach is 1 cell, as in src/client/shell/pane_drop.rs.
type Tuple = [number, number, number, number];
interface Fixture {
  metrics: Omit<DropMetrics, "gap">;
  layouts: Record<string, { area: Tuple; panes: { id: string; rect: Tuple }[] }>;
  cases: { name: string; layout: string; source: string | null; point: [number, number]; zone: { kind: DropZone["kind"]; target?: string; side?: string } | null; estimate?: Tuple }[];
}
const fixture = raw as unknown as Fixture;
const rect = ([x, y, width, height]: Tuple): Rect => ({ x, y, width, height });
const metrics: DropMetrics = { ...fixture.metrics, gap: 1 };

describe("pane drop zones (shared fixture)", () => {
  it.each(fixture.cases.map(c => [c.name, c] as const))("%s", (_, c) => {
    const layout = fixture.layouts[c.layout];
    const area = rect(layout.area);
    const panes = layout.panes.map(p => ({ id: p.id, rect: rect(p.rect) }));
    const zone = dropZoneAt(area, panes, c.source, { x: c.point[0], y: c.point[1] }, metrics);
    expect(zone).toEqual(c.zone);
    if (zone && c.estimate) expect(zoneEstimateRect(area, panes, zone)).toEqual(rect(c.estimate));
  });
});
