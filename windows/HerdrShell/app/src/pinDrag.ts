import type { Snapshot } from "./model";
export type PinSection = "agent" | "pinned";
/** Pointer travel before a press on a pinned row becomes a drag; less is a click. */
export const DRAG_THRESHOLD = 4;
export interface RowBox { top: number; bottom: number; left: number; right: number }
/** The slot under the pointer: a block row's own slot, the first or last one just past the
 * block's ends, null more than a row beyond them or outside the sidebar's column. */
export function slotAt(block: RowBox[], x: number, y: number): number | null {
  const first = block[0], last = block[block.length - 1];
  if (!first || !last) return null;
  const row = first.bottom - first.top;
  if (x < first.left - row || x > first.right + row || y < first.top - row || y > last.bottom + row) return null;
  if (y < first.top) return 0;
  const index = block.findIndex(box => y < box.bottom);
  return index < 0 ? block.length - 1 : index;
}
export function pinMovePlan(snapshot: Snapshot, ids: string[], from: number, to: number): { tab: string; pinIndex: number } | null {
  if (from === to || from < 0 || to < 0 || from >= ids.length || to >= ids.length) return null;
  const tabs = snapshot.tabs ?? [];
  const moving = tabs.find(t => t.tab_id === ids[from]);
  const destination = tabs.find(t => t.tab_id === ids[to]);
  if (!moving || !destination || moving.hidden || destination.hidden || destination.pin_index == null || (moving.role === "agent") !== (destination.role === "agent")) return null;
  return { tab: moving.tab_id, pinIndex: destination.pin_index };
}
