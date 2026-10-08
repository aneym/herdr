import type { Rect } from "./model";

/** The tab background is the split line, except along the tab's outer edges.
 * Scaled rects carry float error (912.9999999999999 for 913), so an edge within half a pixel is the outer edge. */
export function clipRect(box: Rect, size: { width: number; height: number }): Rect {
  return { ...box, width: Math.max(0, box.width - (box.x + box.width < size.width - .5 ? 1 : 0)),
    height: Math.max(0, box.height - (box.y + box.height < size.height - .5 ? 1 : 0)) };
}

/** Right-click cancellation must outlive drag cleanup and the release's default menu.
 * The right release is a pointerup when it is the last button up, else a chorded pointermove.
 * Pass `releasing` when the caller is handling that release itself: listeners added during an event miss it. */
export function blockCancelContextMenu(target: Window, releasing = false): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const context = (event: Event) => event.preventDefault();
  const stop = () => {
    clearTimeout(timer);
    target.removeEventListener("contextmenu", context, true);
    target.removeEventListener("pointerup", release, true);
    target.removeEventListener("pointermove", release, true);
    target.removeEventListener("pointercancel", stop, true);
  };
  const released = () => { clearTimeout(timer); timer = setTimeout(stop, 0); };
  const release = (event: PointerEvent) => {
    if (event.button === 2 && !(event.buttons & 2)) released();
  };
  target.addEventListener("contextmenu", context, true);
  target.addEventListener("pointerup", release, true);
  target.addEventListener("pointermove", release, true);
  target.addEventListener("pointercancel", stop, true);
  if (releasing) released();
  return stop;
}
