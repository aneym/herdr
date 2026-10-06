export type Action = "new_tab" | "close_pane" | "split_right" | "split_down"
  | "focus_pane_left" | "focus_pane_right" | "focus_pane_up" | "focus_pane_down"
  | "toggle_docs" | "zoom_pane" | "rename_tab" | "switcher" | "toggle_sidebar" | "next_attention"
  | "next_tab" | "prev_tab" | "next_pane" | "prev_pane" | `select_tab_${number}`;
export interface KeyEvent { key: string; ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean }
export function actionFor(event: KeyEvent): Action | null {
  const { ctrlKey: ctrl, shiftKey: shift, altKey: alt, metaKey: meta } = event;
  const key = event.key.toLowerCase();
  if (meta) return null;
  if (ctrl && !alt) {
    if (key === "tab") return shift ? "prev_tab" : "next_tab";
    if (!shift) {
      if (/^[1-9]$/.test(key)) return `select_tab_${Number(key)}`;
      return key === "b" ? "toggle_sidebar" : null;
    }
    return ({ t: "new_tab", w: "close_pane", z: "zoom_pane", p: "switcher", a: "next_attention", d: "toggle_docs" } as const)[key as "t"] ?? null;
  }
  if (alt && !ctrl) {
    if (shift) return key === "=" || key === "+" ? "split_right" : key === "-" || key === "_" ? "split_down" : null;
    return ({ arrowleft: "focus_pane_left", arrowright: "focus_pane_right", arrowup: "focus_pane_up", arrowdown: "focus_pane_down" } as const)[key as "arrowleft"] ?? null;
  }
  return !ctrl && !alt && !shift && key === "f2" ? "rename_tab" : null;
}

import type { Terminal } from "@xterm/xterm";
import { bridge } from "./bridge";
import type { Mode } from "./bridge";
export function encodeShiftEnter(mode: Pick<Mode, "kittyFlags" | "modifyOtherKeys">): string {
  return mode.kittyFlags > 0 ? "\x1b[13;2u" : mode.modifyOtherKeys >= 2 ? "\x1b[27;2;13~" : "\x1b\r";
}
export interface KeyTarget { term: Terminal; mode: Mode; send: (text: string) => Promise<void>; shortcut: (event: KeyboardEvent) => boolean }
export async function copy(term: Terminal) { await bridge.clipboardWrite(term.getSelection()); term.clearSelection(); }
export async function paste(term: Terminal) { term.paste(await bridge.clipboardRead()); }
// Both xterm keydown and control-pipe key requests use this decision path.
export function handleKey(event: KeyboardEvent, target: KeyTarget): { handled: boolean; work?: Promise<void> } {
  if (event.type !== "keydown") return { handled: false };
  if (target.shortcut(event)) return { handled: true };
  const key = event.key.toLowerCase();
  if (event.ctrlKey && key === "c" && (event.shiftKey || target.term.hasSelection())) return { handled: true, work: copy(target.term) };
  if (event.ctrlKey && key === "v") return { handled: true, work: paste(target.term) };
  if (event.key === "Enter" && !event.ctrlKey && !event.altKey && !event.metaKey) return { handled: true, work: target.send(event.shiftKey ? encodeShiftEnter(target.mode) : "\r") };
  return { handled: false };
}
export function controlKey(value: string): KeyboardEvent {
  const parts = value.toLowerCase().split("+");
  const key = parts[parts.length - 1];
  return new KeyboardEvent("keydown", { key: ({ enter: "Enter", tab: "Tab", arrowleft: "ArrowLeft", arrowright: "ArrowRight", arrowup: "ArrowUp", arrowdown: "ArrowDown", f2: "F2" } as Record<string, string>)[key] ?? key, ctrlKey: parts.includes("ctrl"), shiftKey: parts.includes("shift"), altKey: parts.includes("alt"), metaKey: parts.includes("meta"), bubbles: true, cancelable: true });
}
