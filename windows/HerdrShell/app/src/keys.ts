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
  return new KeyboardEvent("keydown", { key: key === "enter" ? "Enter" : key === "tab" ? "Tab" : key, ctrlKey: parts.includes("ctrl"), shiftKey: parts.includes("shift"), bubbles: true, cancelable: true });
}
