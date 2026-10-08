import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { PaneController } from "./PaneTerm";
import { bridge } from "./bridge";

export interface DropPaths { paths: string[]; x: number; y: number }
export interface DropPane { id: string; rect: { left: number; top: number; right: number; bottom: number } }
export function formatDropPaths(paths: readonly string[]): string {
  return paths.length ? paths.map(path => /[\s&^|<>()%!;$`'"]/u.test(path) ? `"${path}"` : path).join(" ") + " " : "";
}
export function paneAtDrop(panes: readonly DropPane[], point: { x: number; y: number }): string | null {
  return panes.find(({ rect }) => point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom)?.id ?? null;
}
// Real webview drops and the isolated control hook share CSS client coordinates.
export async function handleDropPaths(drop: DropPaths, panes: readonly PaneController[]): Promise<{ ok: boolean; pane_id: string | null }> {
  if (!Array.isArray(drop.paths) || !drop.paths.every(path => typeof path === "string") || !Number.isFinite(drop.x) || !Number.isFinite(drop.y)) throw new Error("Invalid path drop");
  const boxes = [...document.querySelectorAll<HTMLElement>(".pane[data-pane-id]")].filter(node => !node.closest("[hidden]")).map(node => ({ id: node.dataset.paneId!, rect: node.getBoundingClientRect() }));
  const id = paneAtDrop(boxes, drop);
  const pane = panes.find(pane => pane.info().pane_id === id);
  // Also respect overlays: a desk, switcher or other surface is not a terminal.
  const surface = document.elementFromPoint(drop.x, drop.y)?.closest<HTMLElement>(".pane[data-pane-id]");
  if (!pane || surface?.dataset.paneId !== id || !drop.paths.length) return { ok: true, pane_id: null };
  if (!pane.dropPaths) throw new Error("Terminal does not accept path drops");
  await pane.dropPaths(drop.paths);
  return { ok: true, pane_id: id };
}
export function installPathDrops(getPanes: () => readonly PaneController[]): () => void {
  let disposed = false;
  let unlisten: (() => void) | undefined;
  void Promise.resolve().then(() => getCurrentWebview().onDragDropEvent(event => {
    if (event.payload.type !== "drop") return;
    const { paths, position } = event.payload;
    // Tauri provides physical webview pixels; DOM hit testing uses CSS pixels.
    void handleDropPaths({ paths, x: position.x / window.devicePixelRatio, y: position.y / window.devicePixelRatio }, getPanes()).catch(error => console.error("Path drop failed", error));
  })).then(stop => { if (disposed) stop(); else unlisten = stop; }).catch(error => console.error("Path drop listener failed", error));
  return () => { disposed = true; unlisten?.(); };
}
export async function droppedText(paths: readonly string[], machine: string): Promise<string> {
  const parts: string[] = [];
  for (const path of paths) {
    const image = await bridge.dropReadImage(path);
    if (image === null) parts.push(formatDropPaths([path]).trimEnd());
    else {
      const reply = await bridge.api(machine, "clipboard.image.write", { extension: "png", data_base64: image });
      if (!reply || typeof reply !== "object" || !("paste_text" in reply) || typeof reply.paste_text !== "string" || !reply.paste_text) throw new Error("Dropped image upload failed");
      parts.push(reply.paste_text);
    }
  }
  return parts.length ? parts.join(" ") + " " : "";
}
