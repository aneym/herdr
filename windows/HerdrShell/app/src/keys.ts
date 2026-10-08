export type Action = "move_pane_mode" | "new_tab" | "close_pane" | "split_right" | "split_down"
  | "focus_pane_left" | "focus_pane_right" | "focus_pane_up" | "focus_pane_down"
  | "toggle_docs" | "zoom_pane" | "rename_tab" | "switcher" | "toggle_sidebar" | "next_attention"
  | "next_machine" | "prev_machine" | "next_tab" | "prev_tab" | "next_pane" | "prev_pane" | `select_tab_${number}` | "toggle_area_mode" | `filter_${number}` | `goto_space_${number}`
  | "agent_list_up" | "agent_list_down" | "attention_jump" | "search" | "goto";
export interface KeyEvent { key: string; code?: string; ctrlKey: boolean; shiftKey: boolean; altKey: boolean; metaKey: boolean }
export function actionFor(event: KeyEvent): Action | null {
  const { ctrlKey: ctrl, shiftKey: shift, altKey: alt, metaKey: meta } = event;
  const key = event.key.toLowerCase();
  if (meta) return null;
  if (ctrl && alt && !shift && event.key === "m") return "move_pane_mode";
  if (ctrl && alt && !shift) {
    if (/^Digit[1-6]$/.test(event.code ?? "")) return `filter_${Number(event.code!.slice(5))}`;
    return ({ a: "toggle_area_mode", j: "agent_list_up", k: "agent_list_down" } as const)[key as "a"] ?? null;
  }
  if (ctrl && !alt) {
    if (key === "tab") return shift ? "prev_tab" : "next_tab";
    if (!shift) {
      if (/^[1-9]$/.test(key)) return `select_tab_${Number(key)}`;
      return key === "b" ? "toggle_sidebar" : null;
    }
    if (/^Digit[1-9]$/.test(event.code ?? "")) return `goto_space_${Number(event.code!.slice(5))}`;
    if (event.code === "BracketLeft") return "prev_machine";
    if (event.code === "BracketRight") return "next_machine";
    if (key === "[" || key === "{") return "prev_machine";
    if (key === "]" || key === "}") return "next_machine";
    return ({ t: "new_tab", w: "close_pane", z: "zoom_pane", p: "switcher", a: "next_attention", d: "toggle_docs", o: "attention_jump", k: "search", g: "goto" } as const)[key as "t"] ?? null;
  }
  if (alt && !ctrl) {
    if (shift) return key === "=" || key === "+" ? "split_right" : key === "-" || key === "_" ? "split_down" : null;
    if (event.code === "BracketLeft") return "prev_pane";
    if (event.code === "BracketRight") return "next_pane";
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
export interface Copier { has: () => boolean; copy: () => Promise<boolean>; clear: () => void }
export interface KeyTarget { term: Terminal; machine?: string; mode: Mode; send: (text: string) => Promise<void>; shortcut: (event: KeyboardEvent) => boolean; copier: Copier }
export async function paste(term: Terminal, machine?: string) {
  // Mac reads text first, including an empty string. Only image-only user pastes
  // upload; OSC 52 clipboard reads continue to use the text-only native command.
  let text: string;
  try { text = await bridge.clipboardRead(); }
  catch (textError) {
    const image = await bridge.clipboardReadImage();
    if (image === null) throw textError;
    const bytes = image.length * 3 / 4 - (image.endsWith("==") ? 2 : image.endsWith("=") ? 1 : 0);
    if (!machine || bytes === 0 || bytes > 16 * 1024 * 1024) throw new Error("Clipboard image upload failed");
    const reply = await bridge.api(machine, "clipboard.image.write", { extension: "png", data_base64: image });
    if (!reply || typeof reply !== "object" || !("paste_text" in reply) || typeof reply.paste_text !== "string" || !reply.paste_text) throw new Error("Clipboard image upload failed");
    term.paste(reply.paste_text);
    return;
  }
  term.paste(text);
}
// Both xterm keydown and control-pipe key requests use this decision path.
export function handleKey(event: KeyboardEvent, target: KeyTarget): { handled: boolean; work?: Promise<void> } {
  if (event.type !== "keydown") return { handled: false };
  if (target.shortcut(event)) return { handled: true };
  const key = event.key.toLowerCase();
  // Ctrl+C copies a selection and otherwise interrupts; Ctrl+Shift+C only copies.
  if (event.ctrlKey && !event.altKey && key === "c" && (event.shiftKey || target.copier.has())) return { handled: true, work: target.copier.copy().then(() => {}) };
  if (!["control", "shift", "alt", "meta"].includes(key)) target.copier.clear();
  if (event.ctrlKey && key === "v") return { handled: true, work: paste(target.term, target.machine) };
  if (event.key === "Enter" && !event.ctrlKey && !event.altKey && !event.metaKey) return { handled: true, work: target.send(event.shiftKey ? encodeShiftEnter(target.mode) : "\r") };
  return { handled: false };
}
export function controlKey(value: string): KeyboardEvent {
  const parts = value.toLowerCase().split("+");
  const key = parts[parts.length - 1];
  return new KeyboardEvent("keydown", { code: /^[1-9]$/.test(key) ? `Digit${key}` : key === "[" ? "BracketLeft" : key === "]" ? "BracketRight" : "", key: ({ enter: "Enter", tab: "Tab", arrowleft: "ArrowLeft", arrowright: "ArrowRight", arrowup: "ArrowUp", arrowdown: "ArrowDown", f2: "F2" } as Record<string, string>)[key] ?? key, ctrlKey: parts.includes("ctrl"), shiftKey: parts.includes("shift"), altKey: parts.includes("alt"), metaKey: parts.includes("meta"), bubbles: true, cancelable: true });
}

import type { DropSide } from "./paneDrop";
export function moveModeKey(event: KeyEvent): { kind: "target"; side: DropSide; edge: boolean } | { kind: "drop" } | { kind: "cancel" } | null {
  if (event.ctrlKey || event.altKey || event.metaKey) return null;
  if (event.key === "Escape") return { kind: "cancel" };
  if (event.key === "Enter" || event.key === " ") return { kind: "drop" };
  const side = ({ arrowleft: "left", h: "left", arrowdown: "down", j: "down", arrowup: "up", k: "up", arrowright: "right", l: "right" } as Record<string, DropSide>)[event.key.toLowerCase()];
  return side ? { kind: "target", side, edge: event.shiftKey } : null;
}
