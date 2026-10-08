import type { Rect } from "./model";

/** The tab background is the split line, except along the tab's outer edges. */
export function clipRect(box: Rect, size: { width: number; height: number }): Rect {
  return { ...box, width: Math.max(0, box.width - (box.x + box.width < size.width ? 1 : 0)),
    height: Math.max(0, box.height - (box.y + box.height < size.height ? 1 : 0)) };
}

/** Right-click cancellation must outlive drag cleanup and the release's default menu. */
export function blockCancelContextMenu(target: Window): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const context = (event: Event) => event.preventDefault();
  const stop = () => {
    clearTimeout(timer);
    target.removeEventListener("contextmenu", context, true);
    target.removeEventListener("pointerup", release, true);
    target.removeEventListener("pointercancel", stop, true);
  };
  const release = (event: PointerEvent) => {
    if (event.button === 2) timer = setTimeout(stop, 0);
  };
  target.addEventListener("contextmenu", context, true);
  target.addEventListener("pointerup", release, true);
  target.addEventListener("pointercancel", stop, true);
  return stop;
}
