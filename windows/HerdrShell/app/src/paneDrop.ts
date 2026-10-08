import type { Rect } from "./model";
import { motion } from "./tokens";
export type DropSide = "left" | "right" | "up" | "down";
export type DropZone = { kind: "centre"; target: string } | { kind: "pane_edge"; target: string; side: DropSide } | { kind: "tab_edge"; side: DropSide };
export interface DropMetrics { tabEdge: number; bandMin: number; bandFraction: number; bandMaxFraction: number; gap: number }
export interface PaneBox { id: string; rect: Rect }
const contains = (r: Rect, p: { x: number; y: number }) => p.x >= r.x && p.x < r.x + r.width && p.y >= r.y && p.y < r.y + r.height;
function edge(r: Rect, p: { x: number; y: number }, w: number, h: number): DropSide | null {
  let best: DropSide | null = null, score = Infinity;
  for (const [side, distance, band] of [["left", p.x - r.x, w], ["right", r.x + r.width - p.x, w], ["up", p.y - r.y, h], ["down", r.y + r.height - p.y, h]] as [DropSide, number, number][]) {
    if (distance < band && distance / band < score) { best = side; score = distance / band; }
  }
  return best;
}
export function pixelMetrics(cell: { width: number; height: number }): DropMetrics {
  return { tabEdge: motion.tabEdgePx, bandMin: motion.edgeBandMin, bandFraction: motion.edgeBandFraction, bandMaxFraction: motion.edgeBandMaxFraction, gap: Math.max(cell.width, cell.height) };
}
export function dropZoneAt(area: Rect, panes: readonly PaneBox[], source: string | null, point: { x: number; y: number }, metrics: DropMetrics): DropZone | null {
  if (!contains(area, point)) return null;
  const tabSide = panes.length >= 2 ? edge(area, point, metrics.tabEdge, metrics.tabEdge) : null;
  if (tabSide) return { kind: "tab_edge", side: tabSide };
  let nearest: PaneBox | undefined, at = point, distance = Infinity;
  for (const pane of panes) {
    const r = pane.rect;
    if (!r.width || !r.height) continue;
    if (contains(r, point)) { nearest = pane; at = point; break; }
    const clamped = { x: Math.max(r.x, Math.min(r.x + r.width, point.x)), y: Math.max(r.y, Math.min(r.y + r.height, point.y)) };
    const d = Math.max(Math.abs(point.x - clamped.x), Math.abs(point.y - clamped.y));
    if (d <= metrics.gap && d < distance) { nearest = pane; at = clamped; distance = d; }
  }
  if (!nearest || nearest.id === source) return null;
  const band = (n: number) => Math.min(Math.max(n * metrics.bandFraction, metrics.bandMin), n * metrics.bandMaxFraction);
  const side = edge(nearest.rect, at, band(nearest.rect.width), band(nearest.rect.height));
  return side ? { kind: "pane_edge", target: nearest.id, side } : { kind: "centre", target: nearest.id };
}
export function zoneEstimateRect(area: Rect, panes: readonly PaneBox[], zone: DropZone): Rect {
  const r = zone.kind === "tab_edge" ? area : panes.find(p => p.id === zone.target)?.rect ?? area;
  if (zone.kind === "centre") return { ...r };
  const share = zone.kind === "tab_edge" ? 1 / 3 : .5;
  const horizontal = zone.side === "left" || zone.side === "right";
  const first = Math.round((horizontal ? r.width : r.height) * (zone.side === "left" || zone.side === "up" ? share : 1 - share));
  if (horizontal) return zone.side === "left" ? { ...r, width: first } : { ...r, x: r.x + first, width: r.width - first };
  return zone.side === "up" ? { ...r, height: first } : { ...r, y: r.y + first, height: r.height - first };
}
export function neighbour(panes: readonly PaneBox[], from: string, side: DropSide): string | null {
  const fr = panes.find(p => p.id === from)?.rect;
  if (!fr) return null;
  const horizontal = side === "left" || side === "right";
  let best: string | null = null, score: number[] = [Infinity];
  panes.forEach((p, index) => {
    if (p.id === from) return;
    const r = p.rect;
    const distance = side === "left" ? fr.x - r.x - r.width : side === "right" ? r.x - fr.x - fr.width : side === "up" ? fr.y - r.y - r.height : r.y - fr.y - fr.height;
    const a = horizontal ? r.y : r.x, b = horizontal ? fr.y : fr.x, al = horizontal ? r.height : r.width, bl = horizontal ? fr.height : fr.width;
    const overlap = Math.min(a + al, b + bl) - Math.max(a, b);
    if (distance < 0 || overlap <= 0) return;
    const next = [distance, -overlap, Math.abs(2 * a + al - 2 * b - bl), index];
    const difference = next.findIndex((n, i) => n !== score[i]);
    if (difference >= 0 && next[difference] < score[difference]) { best = p.id; score = next; }
  });
  return best;
}
