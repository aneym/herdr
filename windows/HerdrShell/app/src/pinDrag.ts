import type { Snapshot } from "./model";
// Drag an AGENTS or PINNED row to a new place in its section, as the Mac Shell's PinDrag does.
// The order is a herdr fact (`tab.pin_move`): a drop sends the move and every client follows the
// next snapshot. Until that snapshot shows it, the dropped order stands here as pending.
export type PinSection = "agent" | "pinned";
/** Pointer travel before a press on a pinned row becomes a drag; less is a click. */
export const DRAG_THRESHOLD = 4;
/** How long a dropped order waits for the server before the snapshot order shows again. */
export const PENDING_LIFETIME_MS = 4000;
export interface RowBox { top: number; bottom: number; left: number; right: number }
export type PendingOrders = Partial<Record<PinSection, { order: string[]; at: number }>>;
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
/** The `tab.pin_move` that puts `ids[from]` at slot `to`, and the order to show meanwhile.
 * The server numbers pins across both role blocks, so the slot maps to this section's own
 * pin_index values from the snapshot, even when `ids` is a pending permutation. */
export function pinMovePlan(snapshot: Snapshot, ids: string[], from: number, to: number): { tab: string; pinIndex: number; order: string[]; section: PinSection } | null {
  if (from === to || from < 0 || to < 0 || from >= ids.length || to >= ids.length) return null;
  const tabs = snapshot.tabs ?? [];
  const moving = tabs.find(t => t.tab_id === ids[from]);
  const destination = tabs.find(t => t.tab_id === ids[to]);
  if (!moving || !destination || (moving.role === "agent") !== (destination.role === "agent")) return null;
  const slots = tabs.filter(t => (t.role === "agent") === (moving.role === "agent") && t.pin_index != null).map(t => t.pin_index!).sort((a, b) => a - b);
  if (to >= slots.length) return null;
  const order = ids.filter((_, i) => i !== from);
  order.splice(to, 0, moving.tab_id);
  return { tab: moving.tab_id, pinIndex: slots[to], order, section: moving.role === "agent" ? "agent" : "pinned" };
}
/** `ids` (one section in snapshot order) in its dropped order while that stands. */
export function pendingOrder(ids: string[], entry: { order: string[]; at: number } | undefined, now: number): string[] {
  if (!entry || now - entry.at >= PENDING_LIFETIME_MS || entry.order.length !== ids.length || !ids.every(id => entry.order.includes(id))) return ids;
  return entry.order;
}
